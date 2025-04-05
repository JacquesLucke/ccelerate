#![deny(clippy::unwrap_used)]

use std::{
    collections::{HashMap, HashSet},
    ffi::OsStr,
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::Result;
use bstr::{BStr, BString, ByteSlice, ByteVec};
use ccelerate_shared::WrappedBinary;
use futures::stream::FuturesUnordered;
use tokio::io::AsyncWriteExt;

use crate::{
    CommandOutput, ar_args,
    code_language::CodeLanguage,
    config::Config,
    database::{FileRecord, load_file_record},
    gcc_args,
    path_utils::shorten_path,
    source_file::SourceFile,
    state::State,
    task_periods::TaskPeriodInfo,
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

impl TaskPeriodInfo for FindLinkSourcesTaskInfo {
    fn category(&self) -> String {
        "Find Sources".to_string()
    }

    fn terminal_one_liner(&self) -> String {
        shorten_path(&self.output)
    }

    fn log_detailed(&self) {
        log::info!("Find link sources for {}", self.output.to_string_lossy());
    }
}

fn find_link_sources(
    args_info: &gcc_args::LinkFileInfo,
    conn: &rusqlite::Connection,
    state: &Arc<State>,
) -> Result<OriginalLinkSources> {
    let task_period = state.task_periods.start(FindLinkSourcesTaskInfo {
        output: args_info.output.clone(),
    });

    let mut link_sources = OriginalLinkSources::default();
    for source in args_info.sources.iter() {
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
    let ar_args = ar_args::BuildStaticArchiveInfo::from_args(&record.cwd, &record.args)?;
    for source in ar_args.member_paths {
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

struct GroupObjectsToChunksTaskInfo {
    num: usize,
}

impl TaskPeriodInfo for GroupObjectsToChunksTaskInfo {
    fn category(&self) -> String {
        "Group Chunks".to_string()
    }

    fn terminal_one_liner(&self) -> String {
        format!("Objects: {}", self.num)
    }

    fn log_detailed(&self) {
        log::info!("Group objects to chunks");
    }
}

fn known_object_files_to_chunks(
    original_object_records: &[FileRecord],
    state: &Arc<State>,
) -> Result<Vec<CompileChunk>> {
    let task_period = state.task_periods.start(GroupObjectsToChunksTaskInfo {
        num: original_object_records.len(),
    });

    let mut chunks: HashMap<BString, CompileChunk> = HashMap::new();
    for record in original_object_records {
        let info = gcc_args::BuildObjectFileInfo::from_args(&record.cwd, &record.args)?;

        let mut chunk_key = BString::new(Vec::new());
        chunk_key.push_str(record.binary.to_standard_binary_name().as_encoded_bytes());
        chunk_key.push_str(info.source_language.to_valid_ext());
        gcc_args::add_translation_unit_unspecific_args_to_key(&record.args, &mut chunk_key)?;
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
    state: &Arc<State>,
    config: &Arc<Config>,
) -> Result<Vec<PathBuf>> {
    if records.is_empty() {
        return Ok(vec![]);
    }
    if records.len() <= 10 {
        let result = compile_chunk_sources_in_pool(state, records, config).await;
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
        compile_chunk_in_chunks(left, state, config),
        compile_chunk_in_chunks(right, state, config)
    )?;
    Ok(left.into_iter().chain(right).collect())
}

struct CompileChunkTaskInfo {
    sources: Vec<PathBuf>,
}

impl TaskPeriodInfo for CompileChunkTaskInfo {
    fn category(&self) -> String {
        "Compile".to_string()
    }

    fn terminal_one_liner(&self) -> String {
        self.sources
            .iter()
            .map(|p| shorten_path(p))
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn log_detailed(&self) {
        let mut msg = "Compile chunk: ".to_string();
        for source in &self.sources {
            msg.push_str("  ");
            msg.push_str(&shorten_path(source));
            msg.push('\n');
        }
        log::info!("{}", msg);
    }
}

async fn compile_chunk_sources(
    state: &Arc<State>,
    records: &[FileRecord],
    config: &Config,
) -> Result<PathBuf> {
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

    let first_source_record = records
        .first()
        .expect("There has to be at least one record");
    let source_language = gcc_args::BuildObjectFileInfo::from_args(
        &first_source_record.cwd,
        &first_source_record.args,
    )?
    .source_language;
    let preprocessed_language = source_language.to_preprocessed()?;
    let preprocessed_headers =
        get_compile_chunk_preprocessed_headers(records, state, source_language, config).await?;

    let build_args = gcc_args::update_to_build_object_from_stdin(
        &first_record.args,
        &object_path,
        preprocessed_language,
    )?;

    let task_period = state.task_periods.start(CompileChunkTaskInfo {
        sources: sources.iter().map(|s| (*s).to_owned()).collect(),
    });

    let mut child = tokio::process::Command::new(first_record.binary.to_standard_binary_name())
        .args(build_args)
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
    state: &Arc<State>,
    records: &[FileRecord],
    config: &Arc<Config>,
) -> Result<PathBuf> {
    let state_clone = state.clone();
    let records = Arc::new(records.to_vec());
    let config = config.clone();
    state
        .pool
        .run(async move || compile_chunk_sources(&state_clone, &records, &config).await)
        .await?
}

struct GetPreprocessedHeadersTaskInfo {
    headers_num: usize,
}

impl TaskPeriodInfo for GetPreprocessedHeadersTaskInfo {
    fn category(&self) -> String {
        "Headers".to_string()
    }

    fn terminal_one_liner(&self) -> String {
        format!("Amount: {}", self.headers_num)
    }

    fn log_detailed(&self) {
        log::info!("Get preprocessed headers: {}", self.headers_num);
    }
}

async fn get_compile_chunk_preprocessed_headers(
    records: &[FileRecord],
    state: &Arc<State>,
    source_language: CodeLanguage,
    config: &Config,
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

    let task_period = state.task_periods.start(GetPreprocessedHeadersTaskInfo {
        headers_num: ordered_unique_includes.len(),
    });

    let headers_code = get_compile_chunk_header_code(
        &ordered_unique_includes,
        &include_defines,
        source_language,
        config,
    )?;

    let first_record = records
        .first()
        .expect("There has to be at least one record");
    let preprocess_args =
        gcc_args::update_build_object_args_to_just_output_preprocessed_from_stdin(
            &first_record.args,
            source_language,
        )?;
    let mut child = tokio::process::Command::new(first_record.binary.to_standard_binary_name())
        .args(preprocess_args)
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
    language: CodeLanguage,
    config: &Config,
) -> Result<BString> {
    let mut headers_code = BString::new(Vec::new());
    for define in defines {
        writeln!(headers_code, "{}", define)?;
    }
    for header in include_paths {
        let need_extern_c = language == CodeLanguage::Cxx && config.is_pure_c_header(header);
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

impl TaskPeriodInfo for CreateThinArchiveTaskInfo {
    fn category(&self) -> String {
        "Archive".to_string()
    }

    fn terminal_one_liner(&self) -> String {
        "Create thin archive".to_string()
    }

    fn log_detailed(&self) {
        log::info!("Create thin archive");
    }
}

pub async fn create_thin_archive_for_objects(
    objects: &[PathBuf],
    state: &Arc<State>,
) -> Result<PathBuf> {
    let task_period = state.task_periods.start(CreateThinArchiveTaskInfo {});

    let archive_name = format!("{}.a", uuid::Uuid::new_v4());
    let archive_dir = state.data_dir.join("archives").join(&archive_name[..2]);
    let archive_path = archive_dir.join(archive_name);
    tokio::fs::create_dir_all(&archive_dir).await?;

    let child = tokio::process::Command::new(WrappedBinary::Ar.to_standard_binary_name())
        .args(ar_args::make_args_to_build_thin_static_archive(
            &archive_path,
            objects,
        ))
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

impl TaskPeriodInfo for FinalLinkTaskInfo {
    fn category(&self) -> String {
        "Link".to_string()
    }

    fn terminal_one_liner(&self) -> String {
        shorten_path(&self.output)
    }

    fn log_detailed(&self) {
        log::info!("Final link for {}", self.output.to_string_lossy());
    }
}

pub async fn final_link<S: AsRef<OsStr>>(
    binary: WrappedBinary,
    original_gcc_args: &[S],
    args_info: &gcc_args::LinkFileInfo,
    cwd: &Path,
    state: &Arc<State>,
    sources: &[PathBuf],
) -> Result<CommandOutput> {
    let task_period = state.task_periods.start(FinalLinkTaskInfo {
        output: args_info.output.clone(),
    });

    let new_sources: Vec<_> = sources
        .iter()
        .map(|p| SourceFile {
            path: p.clone(),
            language_override: None,
        })
        .collect();
    let link_args = gcc_args::update_to_link_sources_as_group(original_gcc_args, &new_sources)?;

    let child = tokio::process::Command::new(binary.to_standard_binary_name())
        .args(link_args)
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
    Ok(CommandOutput::from_process_output(child_output))
}

pub async fn wrap_final_link<S: AsRef<OsStr>>(
    binary: WrappedBinary,
    original_args: &[S],
    cwd: &Path,
    state: &Arc<State>,
    config: &Arc<Config>,
) -> Result<CommandOutput> {
    let args_info = gcc_args::LinkFileInfo::from_args(cwd, original_args)?;
    let link_sources = find_link_sources(&args_info, &state.conn.lock(), state)?;
    let chunks = known_object_files_to_chunks(&link_sources.known_object_files, state)?;

    let handles = FuturesUnordered::new();
    for chunk in chunks {
        let state = state.clone();
        let config = config.clone();
        let handle = tokio::task::spawn(async move {
            compile_chunk_in_chunks(&chunk.records, &state, &config).await
        });
        handles.push(handle);
    }
    let mut objects = Vec::new();
    for handle in handles {
        objects.extend(handle.await??);
    }

    let archive_path = create_thin_archive_for_objects(&objects, state).await?;

    let mut all_link_sources = vec![archive_path];
    all_link_sources.extend(link_sources.unknown_sources.into_iter());

    final_link(
        binary,
        original_args,
        &args_info,
        cwd,
        state,
        &all_link_sources,
    )
    .await
}
