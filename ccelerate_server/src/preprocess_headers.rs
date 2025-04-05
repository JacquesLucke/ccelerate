use std::{io::Write, path::Path, sync::Arc};

use anyhow::Result;
use bstr::{BStr, BString, ByteSlice};
use tokio::io::AsyncWriteExt;

use crate::{
    code_language::CodeLanguage, config::Config, gcc_args, state::State,
    state_persistent::ObjectData, task_periods::TaskPeriodInfo,
};

pub async fn get_preprocessed_headers(
    records: &[ObjectData],
    state: &Arc<State>,
    config: &Config,
) -> Result<BString> {
    let any_object = records
        .first()
        .expect("There has to be at least one record");
    let source_language =
        CodeLanguage::from_path(&any_object.local_code.local_code_file)?.to_non_preprocessed()?;

    let mut ordered_unique_includes: Vec<&Path> = vec![];
    let mut include_defines: Vec<&BStr> = vec![];
    for record in records {
        for include in &record.local_code.global_includes {
            if ordered_unique_includes.contains(&include.as_path()) {
                continue;
            }
            ordered_unique_includes.push(include.as_path());
        }
        for define in &record.local_code.include_defines {
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
            &first_record.create.args,
            source_language,
        )?;
    let mut child =
        tokio::process::Command::new(first_record.create.binary.to_standard_binary_name())
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
