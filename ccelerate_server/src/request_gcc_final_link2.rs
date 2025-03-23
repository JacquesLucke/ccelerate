#![deny(clippy::unwrap_used)]

use std::{
    any,
    collections::HashMap,
    ffi::{OsStr, OsString},
    io::{Read, Write},
    ops::Deref,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use actix_web::{HttpResponse, web::Data};
use anyhow::Result;
use bstr::{BStr, BString, ByteSlice, ByteVec};
use ccelerate_shared::WrappedBinary;
use futures::stream::FuturesUnordered;
use tokio::io::AsyncWriteExt;

use crate::{
    database::{FileRecord, load_file_record},
    log_file,
    parse_ar::ArArgs,
    parse_gcc::{GCCArgs, Language, SourceFile},
    state::{self, State},
};

#[derive(Debug, Default)]
struct OriginalLinkSources {
    // These are link sources that were not compiled here, so they were probably
    // precompiled using a different system.
    unknown_sources: Vec<PathBuf>,
    // Those object files are compiled from source here, so we know how they are
    // compiled exactly and can optimize that process.
    known_object_files: Vec<FileRecord>,
}

fn find_link_sources(
    root_args: &GCCArgs,
    conn: &rusqlite::Connection,
) -> Result<OriginalLinkSources> {
    let mut link_sources = OriginalLinkSources::default();
    for source in root_args.sources.iter() {
        find_link_sources_for_file(&source.path, conn, &mut link_sources)?;
    }
    Ok(link_sources)
}

fn find_link_sources_for_file(
    path: &Path,
    conn: &rusqlite::Connection,
    link_sources: &mut OriginalLinkSources,
) -> Result<()> {
    match path.extension() {
        Some(extension) if extension == "a" => {
            find_link_sources_for_static_library(path, conn, link_sources)?;
        }
        Some(extension) if extension == "o" => {
            find_link_sources_for_object_file(path, conn, link_sources)?;
        }
        _ => {
            link_sources.unknown_sources.push(path.to_owned());
        }
    }
    Ok(())
}

fn find_link_sources_for_static_library(
    library_path: &Path,
    conn: &rusqlite::Connection,
    link_sources: &mut OriginalLinkSources,
) -> Result<()> {
    let Some(record) = load_file_record(conn, library_path) else {
        link_sources.unknown_sources.push(library_path.to_owned());
        return Ok(());
    };
    if !record.binary.is_ar_compatible() {
        return Err(anyhow::anyhow!(
            "Archive not created by ar: {}",
            library_path.display()
        ));
    }
    let ar_args = ArArgs::parse_owned(&record.cwd, record.args)?;
    for source in ar_args.sources {
        find_link_sources_for_file(&source, conn, link_sources)?;
    }
    Ok(())
}

fn find_link_sources_for_object_file(
    object_path: &Path,
    conn: &rusqlite::Connection,
    link_sources: &mut OriginalLinkSources,
) -> Result<()> {
    let Some(record) = load_file_record(conn, object_path) else {
        link_sources.unknown_sources.push(object_path.to_owned());
        return Ok(());
    };
    if !record.binary.is_gcc_compatible() {
        return Err(anyhow::anyhow!(
            "Object file not created by gcc compatible: {}",
            object_path.display()
        ));
    }
    link_sources.known_object_files.push(record);
    Ok(())
}

#[derive(Debug, Clone)]
struct CompileChunk {
    // Only contains the general flags using for the compilation but no mention of the
    // source or output files.
    reduced_args: GCCArgs,
    global_includes: Vec<PathBuf>,
    include_defines: Vec<BString>,
    preprocessed_sources: Vec<PathBuf>,
    source_language: Language,
    binary: WrappedBinary,
}

fn known_object_files_to_chunks(
    original_object_records: &[FileRecord],
) -> Result<Vec<CompileChunk>> {
    let mut chunks: HashMap<BString, CompileChunk> = HashMap::new();
    for record in original_object_records {
        let gcc_args = GCCArgs::parse_owned(&record.cwd, &record.args)?;

        let mut chunk_key = BString::new(Vec::new());
        chunk_key.push_str(record.binary.to_standard_binary_name().as_encoded_bytes());
        let source_language = gcc_args
            .sources
            .first()
            .map(|s| s.language())
            .ok_or_else(|| anyhow::anyhow!("Cannot determine language of source file"))??;
        chunk_key.push_str(source_language.to_valid_ext());

        // Remove data the is specific to a single translation unit.
        let mut reduced_args = gcc_args;
        reduced_args.sources.clear();
        reduced_args.primary_output = None;
        reduced_args.depfile_target_name = None;
        reduced_args.depfile_output_path = None;
        reduced_args.depfile_generate = false;

        for arg in reduced_args.to_args() {
            chunk_key.push_str(arg.as_encoded_bytes());
        }
        chunk_key.push_str(record.cwd.as_os_str().as_encoded_bytes());
        for include_define in record.include_defines.iter().flatten() {
            chunk_key.push_str(include_define);
        }
        for bad_include in record.bad_includes.iter().flatten() {
            chunk_key.push_str(bad_include.as_os_str().as_encoded_bytes());
        }
        let chunk = chunks.entry(chunk_key).or_insert_with(|| CompileChunk {
            reduced_args,
            global_includes: Vec::new(),
            include_defines: Vec::new(),
            preprocessed_sources: Vec::new(),
            source_language,
            binary: record.binary,
        });
        for define in record.include_defines.iter().flatten() {
            if chunk.include_defines.contains(define) {
                continue;
            }
            chunk.include_defines.push(define.clone());
        }
        for include in record.global_includes.iter().flatten() {
            if chunk.global_includes.contains(include) {
                continue;
            }
            chunk.global_includes.push(include.clone());
        }
        let Some(local_code_file) = &record.local_code_file else {
            return Err(anyhow::anyhow!("Missing local code file"));
        };
        chunk.preprocessed_sources.push(local_code_file.clone());
    }
    Ok(chunks.into_values().collect())
}

async fn compile_chunk(chunk: &CompileChunk, state: &Data<State>) -> Result<Vec<PathBuf>> {
    let chunk = Arc::new(chunk.clone());

    let mut gcc_args = chunk.reduced_args.clone();
    gcc_args.stop_before_link = true;
    gcc_args.stop_after_preprocessing = false;
    gcc_args.stop_before_assemble = false;

    let all_preprocessed_headers = get_compile_chunk_preprocessed_headers(&chunk, state).await?;

    let handles = FuturesUnordered::new();
    for source in &chunk.preprocessed_sources {
        let state_clone = state.clone();
        let source = source.clone();
        let headers = all_preprocessed_headers.clone();
        let chunk = chunk.clone();
        let handle = state.pool.run(async move || {
            return compile_chunk_sources(&chunk, &state_clone, headers.as_bstr(), &[&source])
                .await;
        });
        handles.push(handle);
    }
    let mut objects = vec![];
    for handle in handles {
        objects.push(handle.await??);
    }
    Ok(objects)
}

async fn compile_chunk_sources(
    chunk: &CompileChunk,
    state: &Data<State>,
    all_preprocessed_headers: &BStr,
    local_code_files: &[&Path],
) -> Result<PathBuf> {
    let mut gcc_args = chunk.reduced_args.clone();
    gcc_args.stop_before_link = true;
    gcc_args.stop_after_preprocessing = false;
    gcc_args.stop_before_assemble = false;

    println!("Compiling: {:?}", local_code_files);

    let mut full_preprocessed = all_preprocessed_headers.to_owned();
    for local_code_path in local_code_files {
        let local_source_code = tokio::fs::read(local_code_path).await?;
        full_preprocessed.push_str(&local_source_code);
    }

    if state.cli.log_files {
        let mut identifier = String::new();
        identifier.push_str("Full preprocessed for:\n");
        for local_code_path in local_code_files {
            identifier.push_str("  ");
            identifier.push_str(&local_code_path.to_string_lossy());
            identifier.push('\n');
        }
        log_file(
            state,
            &identifier,
            &full_preprocessed,
            chunk.source_language.to_preprocessed()?.to_valid_ext(),
        )
        .await?;
    }

    let object_name = uuid::Uuid::new_v4().to_string();
    let object_dir = state.data_dir.join("objects").join(&object_name[..2]);
    let object_path = object_dir.join(object_name);
    tokio::fs::create_dir_all(&object_dir).await?;

    let mut local_gcc_args = gcc_args.clone();
    local_gcc_args.primary_output = Some(object_path.clone());
    let mut gcc_args = local_gcc_args.to_args();
    gcc_args.push("-x".into());
    gcc_args.push(chunk.source_language.to_preprocessed()?.to_x_arg().into());
    gcc_args.push("-".into());

    let mut child = tokio::process::Command::new(chunk.binary.to_standard_binary_name())
        .args(&gcc_args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(&full_preprocessed).await?;
    }
    let child_output = child.wait_with_output().await?;
    if !child_output.status.success() {
        return Err(anyhow::anyhow!(
            "Compilation failed failed: {}",
            String::from_utf8_lossy(&child_output.stderr)
        ));
    }
    Ok(object_path)
}

async fn get_compile_chunk_preprocessed_headers(
    chunk: &CompileChunk,
    state: &Data<State>,
) -> Result<BString> {
    let headers_code = get_compile_chunk_header_code(chunk, state)?;
    if state.cli.log_files {
        let mut identifier = String::new();
        identifier.push_str("Headers for:\n");
        for source_file in &chunk.preprocessed_sources {
            identifier.push_str("  ");
            identifier.push_str(&source_file.to_string_lossy());
            identifier.push('\n');
        }
        log_file(
            state,
            &identifier,
            &headers_code,
            chunk.source_language.to_valid_ext(),
        )
        .await?;
    }
    let mut gcc_args = chunk.reduced_args.clone();
    gcc_args.stop_after_preprocessing = true;
    gcc_args.stop_before_link = false;
    gcc_args.stop_before_assemble = false;
    let mut gcc_args = gcc_args.to_args();
    gcc_args.push("-x".into());
    gcc_args.push(chunk.source_language.to_x_arg().into());
    gcc_args.push("-".into());
    let mut child = tokio::process::Command::new(chunk.binary.to_standard_binary_name())
        .args(&gcc_args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(&headers_code).await?;
    }
    let child_output = child.wait_with_output().await?;
    if !child_output.status.success() {
        return Err(anyhow::anyhow!(
            "Preprocessing failed: {}",
            String::from_utf8_lossy(&child_output.stderr)
        ));
    }
    let preprocessed_headers = BString::from(child_output.stdout);
    if state.cli.log_files {
        let mut identifier = String::new();
        identifier.push_str("Preprocessed headers for:\n");
        for source_file in &chunk.preprocessed_sources {
            identifier.push_str("  ");
            identifier.push_str(&source_file.to_string_lossy());
            identifier.push('\n');
        }
        log_file(
            state,
            &identifier,
            &preprocessed_headers,
            chunk.source_language.to_preprocessed()?.to_valid_ext(),
        )
        .await?;
    }
    Ok(preprocessed_headers)
}

fn get_compile_chunk_header_code(chunk: &CompileChunk, state: &Data<State>) -> Result<BString> {
    let mut headers_code = BString::new(Vec::new());
    for define in &chunk.include_defines {
        writeln!(headers_code, "{}", define)?;
    }
    for header in &chunk.global_includes {
        let need_extern_c =
            chunk.source_language == Language::Cxx && state.config.lock().is_pure_c_header(header);
        if need_extern_c {
            writeln!(headers_code, "extern \"C\" {{")?;
        }
        writeln!(headers_code, "#include <{}>", header.display())?;
        if need_extern_c {
            writeln!(headers_code, "}}")?;
        }
    }
    Ok(headers_code)
}

pub async fn handle_gcc_final_link_request2(
    binary: WrappedBinary,
    request_gcc_args: &GCCArgs,
    cwd: &Path,
    state: &Data<State>,
) -> HttpResponse {
    let Ok(link_sources) = find_link_sources(request_gcc_args, &state.conn.lock()) else {
        return HttpResponse::BadRequest().body("Error finding link sources");
    };
    let Ok(chunks) = known_object_files_to_chunks(&link_sources.known_object_files) else {
        return HttpResponse::BadRequest().body("Error chunking objects");
    };

    let handles = FuturesUnordered::new();
    for chunk in chunks {
        let state = state.clone();
        let handle = tokio::task::spawn(async move { compile_chunk(&chunk, &state).await });
        handles.push(handle);
    }
    let mut objects = Vec::new();
    for handle in handles {
        match handle.await {
            Ok(Ok(chunk_objects)) => {
                objects.extend(chunk_objects);
            }
            Ok(Err(e)) => {
                log::error!("Error compiling chunk: {:?}", e);
                return HttpResponse::BadRequest().body("Error compiling chunk");
            }
            Err(e) => {
                log::error!("Error compiling chunk: {:?}", e);
                return HttpResponse::BadRequest().body("Error compiling chunk");
            }
        }
    }

    // let mut final_link_args = request_gcc_args.clone();
    // final_link_args.sources = final_link_sources
    //     .iter()
    //     .map(|s| SourceFile {
    //         path: s.clone(),
    //         language_override: None,
    //     })
    //     .collect();
    // final_link_args.use_link_group = true;
    HttpResponse::Ok().body("todo")
}
