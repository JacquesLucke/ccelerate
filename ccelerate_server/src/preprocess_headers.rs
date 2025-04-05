use std::{io::Write, path::Path, sync::Arc};

use anyhow::Result;
use bstr::{BStr, BString, ByteSlice};
use nunny::NonEmpty;
use tokio::io::AsyncWriteExt;

use crate::{
    code_language::CodeLanguage, config::Config, gcc_args, state::State,
    state_persistent::ObjectData, task_periods::TaskPeriodInfo,
};

pub async fn get_preprocessed_headers(
    objects: &NonEmpty<[ObjectData]>,
    state: &Arc<State>,
    config: &Config,
) -> Result<BString> {
    let any_object = objects.first();
    let source_language =
        CodeLanguage::from_path(&any_object.local_code.local_code_file)?.to_non_preprocessed()?;

    let task_period = state.task_periods.start(GetPreprocessedHeadersTaskInfo {});

    let include_code = get_include_code_for_objects(objects, config)?;

    let preprocess_args =
        gcc_args::update_build_object_args_to_just_output_preprocessed_from_stdin(
            &any_object.create.args,
            source_language,
        )?;
    let mut child =
        tokio::process::Command::new(any_object.create.binary.to_standard_binary_name())
            .args(preprocess_args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(&include_code).await?;
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

fn get_include_code_for_objects(
    objects: &NonEmpty<[ObjectData]>,
    config: &Config,
) -> Result<BString> {
    let mut ordered_unique_includes: Vec<&Path> = vec![];
    let mut include_defines: Vec<&BStr> = vec![];
    for object in objects {
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
        source_language,
        config,
    )
}

fn get_include_code(
    include_paths: &[impl AsRef<Path>],
    defines: &[impl AsRef<BStr>],
    language: CodeLanguage,
    config: &Config,
) -> Result<BString> {
    let mut headers_code = BString::new(Vec::new());
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
