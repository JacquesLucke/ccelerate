#![deny(clippy::unwrap_used)]

use std::{
    collections::{HashMap, HashSet},
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
};

use actix_web::{HttpResponse, web::Data};
use anyhow::Result;
use bstr::{BStr, BString, ByteSlice, ByteVec};
use ccelerate_shared::WrappedBinary;
use futures::stream::FuturesUnordered;
use tokio::io::AsyncWriteExt;

use crate::{
    database::{FileRecord, load_file_record},
    parse_ar::ArArgs,
    parse_gcc::{GCCArgs, Language, SourceFile},
    path_utils::shorten_path,
    state::State,
    task_log::{TaskInfo, log_task},
};

#[derive(Debug, Default)]
struct OriginalLinkSources {
    // These are link sources that were not compiled here, so they were probably
    // precompiled using a different system.
    unknown_sources: Vec<PathBuf>,
    // Those object files are compiled from source here, so we know how they are
    // compiled exactly and can optimize that process.
    known_object_files: Vec<FileRecord>,

    handled_paths: HashSet<PathBuf>,
}

struct FindLinkSourcesTaskInfo {
    output: PathBuf,
}

impl TaskInfo for FindLinkSourcesTaskInfo {
    fn short_name(&self) -> String {
        format!("Find link sources for {}", shorten_path(&self.output))
    }

    fn log(&self) {
        log::info!("Find link sources for {}", self.output.to_string_lossy());
    }
}

fn find_link_sources(
    root_args: &GCCArgs,
    conn: &rusqlite::Connection,
    state: &Data<State>,
) -> Result<OriginalLinkSources> {
    let task_period = log_task(
        &FindLinkSourcesTaskInfo {
            output: root_args
                .primary_output
                .clone()
                .unwrap_or(PathBuf::from("")),
        },
        state,
    );

    let mut link_sources = OriginalLinkSources::default();
    for source in root_args.sources.iter() {
        find_link_sources_for_file(&source.path, conn, &mut link_sources)?;
    }
    task_period.finished_successfully();
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
    if !link_sources.handled_paths.insert(library_path.to_owned()) {
        return Ok(());
    }
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
    if !link_sources.handled_paths.insert(object_path.to_owned()) {
        return Ok(());
    }
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
    records: Vec<FileRecord>,
}

struct GroupObjectsToChunksTaskInfo {}

impl TaskInfo for GroupObjectsToChunksTaskInfo {
    fn short_name(&self) -> String {
        "Group objects to chunks".to_string()
    }

    fn log(&self) {
        log::info!("Group objects to chunks");
    }
}

fn known_object_files_to_chunks(
    original_object_records: &[FileRecord],
    state: &Data<State>,
) -> Result<Vec<CompileChunk>> {
    let task_period = log_task(&GroupObjectsToChunksTaskInfo {}, state);

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
            records: Vec::new(),
        });
        chunk.records.push(record.clone());
    }
    task_period.finished_successfully();
    Ok(chunks.into_values().collect())
}

#[async_recursion::async_recursion]
async fn compile_chunk_in_chunks(
    records: &[FileRecord],
    state: &Data<State>,
) -> Result<Vec<PathBuf>> {
    if records.is_empty() {
        return Ok(vec![]);
    }
    if records.len() <= 10 {
        let result = compile_chunk_sources_in_pool(state, records).await;
        match result {
            Ok(object) => {
                return Ok(vec![object]);
            }
            Err(e) => {
                if records.len() == 1 {
                    return Err(e);
                }
            }
        }
    }
    let (left, right) = records.split_at(records.len() / 2);
    let (left, right) = tokio::try_join!(
        compile_chunk_in_chunks(left, state),
        compile_chunk_in_chunks(right, state)
    )?;
    Ok(left.into_iter().chain(right).collect())
}

struct CompileChunkTaskInfo<'a> {
    sources: &'a [&'a Path],
}

impl TaskInfo for CompileChunkTaskInfo<'_> {
    fn short_name(&self) -> String {
        let mut short_name = format!("Compile ({}): ", self.sources.len());
        for source in self.sources {
            short_name.push_str(&shorten_path(source));
            short_name.push(' ');
        }
        short_name
    }

    fn log(&self) {
        let mut msg = "Compile chunk: ".to_string();
        for source in self.sources {
            msg.push_str("  ");
            msg.push_str(&shorten_path(source));
            msg.push('\n');
        }
        log::info!("{}", msg);
    }
}

async fn compile_chunk_sources(state: &Data<State>, records: &[FileRecord]) -> Result<PathBuf> {
    let sources = records
        .iter()
        .flat_map(|r| &r.local_code_file)
        .map(|p| p.as_path())
        .collect::<Vec<_>>();
    let first_record = records
        .first()
        .expect("There has to be at least one record");

    let object_name = format!("{}.o", uuid::Uuid::new_v4());
    let object_dir = state.data_dir.join("objects").join(&object_name[..2]);
    let object_path = object_dir.join(object_name);
    tokio::fs::create_dir_all(&object_dir).await?;

    let task_period = log_task(&CompileChunkTaskInfo { sources: &sources }, state);
    let mut gcc_args = GCCArgs::parse_owned(&first_record.cwd, &first_record.args)?;
    let first_source = gcc_args
        .sources
        .first()
        .expect("There has to be at least one source");
    let source_language = first_source.language()?;
    let preprocessed_language = source_language.to_preprocessed()?;
    let preprocessed_headers =
        get_compile_chunk_preprocessed_headers(records, state, source_language).await?;

    gcc_args.stop_before_link = true;
    gcc_args.stop_after_preprocessing = false;
    gcc_args.stop_before_assemble = false;
    gcc_args.primary_output = Some(object_path.clone());
    gcc_args.sources = vec![];

    let mut gcc_args = gcc_args.to_args();
    gcc_args.push("-x".into());
    gcc_args.push(preprocessed_language.to_x_arg().into());
    gcc_args.push("-".into());

    let mut child = tokio::process::Command::new(first_record.binary.to_standard_binary_name())
        .args(&gcc_args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(&preprocessed_headers).await?;
        for record in records {
            if let Some(local_code_path) = &record.local_code_file {
                let local_source_code = tokio::fs::read(local_code_path).await?;
                stdin.write_all(&local_source_code).await?;
            }
        }
    } else {
        return Err(anyhow::anyhow!("Failed to open stdin for child process"));
    }
    let child_output = child.wait_with_output().await?;
    if !child_output.status.success() {
        return Err(anyhow::anyhow!(
            "Compilation failed failed: {}",
            String::from_utf8_lossy(&child_output.stderr)
        ));
    }
    task_period.finished_successfully();
    Ok(object_path)
}

async fn compile_chunk_sources_in_pool(
    state: &Data<State>,
    records: &[FileRecord],
) -> Result<PathBuf> {
    let state_clone = state.clone();
    let records = Arc::new(records.to_vec());
    state
        .pool
        .run(async move || compile_chunk_sources(&state_clone, &records).await)
        .await?
}

struct GetPreprocessedHeadersTaskInfo {
    headers_num: usize,
}

impl TaskInfo for GetPreprocessedHeadersTaskInfo {
    fn short_name(&self) -> String {
        format!("Get preprocessed headers: {}", self.headers_num)
    }

    fn log(&self) {
        log::info!("Get preprocessed headers: {}", self.headers_num);
    }
}

async fn get_compile_chunk_preprocessed_headers(
    records: &[FileRecord],
    state: &Data<State>,
    source_language: Language,
) -> Result<BString> {
    let mut ordered_unique_includes: Vec<&Path> = vec![];
    let mut include_defines: Vec<&BStr> = vec![];
    for record in records {
        for include in record.global_includes.iter().flatten() {
            if ordered_unique_includes.contains(&include.as_path()) {
                continue;
            }
            ordered_unique_includes.push(include.as_path());
        }
        for define in record.include_defines.iter().flatten() {
            if include_defines.contains(&define.as_bstr()) {
                continue;
            }
            include_defines.push(define.as_bstr());
        }
    }

    let task_period = log_task(
        &GetPreprocessedHeadersTaskInfo {
            headers_num: ordered_unique_includes.len(),
        },
        state,
    );

    let headers_code = get_compile_chunk_header_code(
        &ordered_unique_includes,
        &include_defines,
        source_language,
        state,
    )?;

    let first_record = records
        .first()
        .expect("There has to be at least one record");
    let mut gcc_args = GCCArgs::parse_owned(&first_record.cwd, &first_record.args)?;
    gcc_args.sources = vec![];
    gcc_args.primary_output = None;
    gcc_args.depfile_target_name = None;
    gcc_args.depfile_output_path = None;
    gcc_args.depfile_generate = false;
    gcc_args.stop_after_preprocessing = true;
    gcc_args.stop_before_link = false;
    gcc_args.stop_before_assemble = false;
    let mut gcc_args = gcc_args.to_args();
    gcc_args.push("-x".into());
    gcc_args.push(source_language.to_x_arg().into());
    gcc_args.push("-".into());
    let mut child = tokio::process::Command::new(first_record.binary.to_standard_binary_name())
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
    task_period.finished_successfully();
    Ok(preprocessed_headers)
}

fn get_compile_chunk_header_code(
    include_paths: &[&Path],
    defines: &[&BStr],
    language: Language,
    state: &Data<State>,
) -> Result<BString> {
    let mut headers_code = BString::new(Vec::new());
    for define in defines {
        writeln!(headers_code, "{}", define)?;
    }
    for header in include_paths {
        let need_extern_c =
            language == Language::Cxx && state.config.lock().is_pure_c_header(header);
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

struct CreateThinArchiveTaskInfo {}

impl TaskInfo for CreateThinArchiveTaskInfo {
    fn short_name(&self) -> String {
        "Create thin archive".to_string()
    }

    fn log(&self) {
        log::info!("Create thin archive");
    }
}

pub async fn create_thin_archive_for_objects(
    objects: &[PathBuf],
    state: &Data<State>,
) -> Result<PathBuf> {
    let task_period = log_task(&CreateThinArchiveTaskInfo {}, state);

    let archive_name = format!("{}.a", uuid::Uuid::new_v4());
    let archive_dir = state.data_dir.join("archives").join(&archive_name[..2]);
    let archive_path = archive_dir.join(archive_name);
    tokio::fs::create_dir_all(&archive_dir).await?;

    let ar_args = ArArgs {
        flag_c: true,
        flag_q: true,
        flag_s: false,
        thin_archive: true,
        sources: objects.to_vec(),
        output: Some(archive_path.clone()),
    };

    let child = tokio::process::Command::new(WrappedBinary::Ar.to_standard_binary_name())
        .args(ar_args.to_args())
        .spawn()?;
    let child_output = child.wait_with_output().await?;
    if !child_output.status.success() {
        return Err(anyhow::anyhow!(
            "Archive creation failed: {}",
            String::from_utf8_lossy(&child_output.stderr)
        ));
    }

    task_period.finished_successfully();
    Ok(archive_path)
}

struct FinalLinkTaskInfo {
    output: PathBuf,
}

impl TaskInfo for FinalLinkTaskInfo {
    fn short_name(&self) -> String {
        format!("Final link for {}", shorten_path(&self.output))
    }

    fn log(&self) {
        log::info!("Final link for {}", self.output.to_string_lossy());
    }
}

pub async fn final_link(
    binary: WrappedBinary,
    original_gcc_args: &GCCArgs,
    cwd: &Path,
    state: &Data<State>,
    sources: &[PathBuf],
) -> Result<std::process::Output> {
    let task_period = log_task(
        &FinalLinkTaskInfo {
            output: original_gcc_args
                .primary_output
                .clone()
                .unwrap_or(PathBuf::from("")),
        },
        state,
    );

    let mut args = original_gcc_args.clone();
    args.sources = sources
        .iter()
        .map(|p| SourceFile {
            path: p.clone(),
            language_override: None,
        })
        .collect();
    args.use_link_group = true;
    let child = tokio::process::Command::new(binary.to_standard_binary_name())
        .args(args.to_args())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .current_dir(cwd)
        .spawn()?;
    let child_output = child.wait_with_output().await?;
    if !child_output.status.success() {
        return Err(anyhow::anyhow!(
            "Linking failed: {}",
            String::from_utf8_lossy(&child_output.stderr)
        ));
    }
    task_period.finished_successfully();
    Ok(child_output)
}

pub async fn handle_gcc_final_link_request(
    binary: WrappedBinary,
    request_gcc_args: &GCCArgs,
    cwd: &Path,
    state: &Data<State>,
) -> HttpResponse {
    let Ok(link_sources) = find_link_sources(request_gcc_args, &state.conn.lock(), state) else {
        return HttpResponse::BadRequest().body("Error finding link sources");
    };
    let Ok(chunks) = known_object_files_to_chunks(&link_sources.known_object_files, state) else {
        return HttpResponse::BadRequest().body("Error chunking objects");
    };

    let handles = FuturesUnordered::new();
    for chunk in chunks {
        let state = state.clone();
        let handle =
            tokio::task::spawn(
                async move { compile_chunk_in_chunks(&chunk.records, &state).await },
            );
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
                return HttpResponse::BadRequest().body(e.to_string());
            }
            Err(e) => {
                log::error!("Error compiling chunk: {:?}", e);
                return HttpResponse::InternalServerError().body(e.to_string());
            }
        }
    }

    let Ok(archive_path) = create_thin_archive_for_objects(&objects, state).await else {
        return HttpResponse::BadRequest().body("Error creating thin archive");
    };

    let mut all_link_sources = vec![archive_path];
    all_link_sources.extend(link_sources.unknown_sources.into_iter());

    let link_output =
        match final_link(binary, request_gcc_args, cwd, state, &all_link_sources).await {
            Ok(output) => output,
            Err(e) => {
                log::error!("Error linking thin archive: {:?}", e);
                return HttpResponse::BadRequest().body("Error linking thin archive");
            }
        };

    HttpResponse::Ok().json(
        ccelerate_shared::RunResponseData {
            stdout: link_output.stdout,
            stderr: link_output.stderr,
            status: link_output.status.code().unwrap_or(1),
        }
        .to_wire(),
    )
}
