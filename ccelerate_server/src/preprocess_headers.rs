use std::{io::Write, path::Path, sync::Arc};

use anyhow::Result;
use bstr::{BStr, BString, ByteSlice};
use nunny::NonEmpty;

use crate::{
    CommandOutput, args_processing, code_language::CodeLanguage, config::Config, path_utils,
    state::State, state_persistent::ObjectData, task_periods::TaskPeriodInfo,
};

pub async fn get_preprocessed_headers(
    objects: &NonEmpty<[Arc<ObjectData>]>,
    state: &Arc<State>,
    config: &Config,
    output_path: &Path,
) -> Result<()> {
    let any_object = objects.first();
    let source_language =
        CodeLanguage::from_path(&any_object.local_code.local_code_file)?.to_non_preprocessed()?;
    let include_code = get_include_code_for_objects(objects, config)?;
    let include_code_file =
        tempfile::NamedTempFile::with_suffix(format!(".{}", source_language.valid_ext()))?;
    path_utils::ensure_directory_and_write(include_code_file.path(), &include_code).await?;
    let task_period = state.task_periods.start(GetPreprocessedHeadersTaskInfo {});
    let preprocess_args = args_processing::rewrite_to_get_preprocessed_headers(
        any_object.create.binary,
        &any_object.create.args,
        include_code_file.path(),
        output_path,
    )?;
    let child = tokio::process::Command::new(any_object.create.binary.to_standard_binary_name())
        .args(preprocess_args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;
    let child_output = child.wait_with_output().await?;
    if !child_output.status.success() {
        return Err(CommandOutput::from_process_output(child_output).into());
    }
    task_period.finished_successfully();
    Ok(())
}

fn get_include_code_for_objects(
    objects: &NonEmpty<[Arc<ObjectData>]>,
    config: &Config,
) -> Result<BString> {
    let mut comment_lines = vec!["Include code for the following files:".into()];
    let mut ordered_unique_includes: Vec<&Path> = vec![];
    let mut include_defines: Vec<&BStr> = vec![];
    for object in objects {
        comment_lines.push(object.local_code.local_code_file.to_string_lossy());
        for include in &object.local_code.global_includes {
            if ordered_unique_includes.contains(&include.as_path()) {
                continue;
            }
            ordered_unique_includes.push(include.as_path());
        }
        for define in &object.local_code.include_defines {
            if include_defines.contains(&define.as_bstr()) {
                continue;
            }
            include_defines.push(define.as_bstr());
        }
    }
    let any_object = objects.first();
    let source_language =
        CodeLanguage::from_path(&any_object.local_code.local_code_file)?.to_non_preprocessed()?;

    get_include_code(
        &ordered_unique_includes,
        &include_defines,
        &comment_lines,
        source_language,
        config,
    )
}

fn get_include_code(
    include_paths: &[impl AsRef<Path>],
    defines: &[impl AsRef<BStr>],
    comment_lines: &[impl AsRef<str>],
    language: CodeLanguage,
    config: &Config,
) -> Result<BString> {
    let mut headers_code = BString::new(Vec::new());
    for line in comment_lines {
        writeln!(headers_code, "// {}", line.as_ref())?;
    }
    for define in defines {
        writeln!(headers_code, "{}", define.as_ref())?;
    }
    for header in include_paths {
        let header = header.as_ref();
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

struct GetPreprocessedHeadersTaskInfo {}

impl TaskPeriodInfo for GetPreprocessedHeadersTaskInfo {
    fn category(&self) -> String {
        "Headers".to_string()
    }

    fn terminal_one_liner(&self) -> String {
        "Headers".into()
    }

    fn log_detailed(&self) {
        log::info!("Get preprocessed headers");
    }
}
