use actix_web::{HttpResponse, web::Data};
use anyhow::Result;
use bstr::BString;
use ccelerate_shared::WrappedBinary;
use std::{
    collections::HashSet,
    ffi::{OsStr, OsString},
    io::Write,
    path::{Path, PathBuf},
};
use tokio::io::AsyncWriteExt;

use crate::{
    State, load_db_file,
    parse_ar::ArArgs,
    parse_gcc::{GCCArgs, SourceFile},
};

fn find_smallest_link_units(
    link_args: &GCCArgs,
    conn: &rusqlite::Connection,
) -> Result<Vec<PathBuf>> {
    let mut final_sources = HashSet::new();
    let mut remaining_paths = vec![];
    for arg in &link_args.sources {
        remaining_paths.push(arg.path.clone());
    }
    while let Some(current_path) = remaining_paths.pop() {
        match current_path.to_string_lossy().to_string() {
            p if p.ends_with(".o") => {
                final_sources.insert(current_path.clone());
            }
            p if p.ends_with(".a") => {
                let file_row = load_db_file(conn, &current_path);
                if let Some(file_row) = file_row {
                    match file_row.data.binary {
                        binary if binary.is_gcc_compatible() => {
                            let args = GCCArgs::parse_owned(&file_row.data.cwd, file_row.data.args)
                                .unwrap();
                            remaining_paths.extend(args.sources.iter().map(|s| s.path.clone()));
                        }
                        binary if binary.is_ar_compatible() => {
                            let args = ArArgs::parse_owned(&file_row.data.cwd, file_row.data.args)
                                .unwrap();
                            remaining_paths.extend(args.sources.iter().cloned());
                        }
                        binary => {
                            panic!("Cannot handle binary: {:?}", binary);
                        }
                    }
                } else {
                    final_sources.insert(current_path.clone());
                }
            }
            p if p.ends_with(".so") || p.contains(".so.") => {
                final_sources.insert(current_path.clone());
            }
            _ => {
                panic!("unhandled extension: {:?}", current_path);
            }
        };
    }
    Ok(final_sources.into_iter().collect())
}

#[derive(Debug, Clone)]
struct WrappedLinkUnit {
    original_object_path: PathBuf,
    wrapped_object_path: PathBuf,
}

fn osstring_to_osstr_vec(s: &[OsString]) -> Vec<&OsStr> {
    s.iter().map(|s| s.as_ref()).collect()
}

async fn build_combined_translation_unit(
    original_object_files: &[&Path],
    dst_object_file: &Path,
    state: &Data<State>,
) {
    let mut headers: Vec<PathBuf> = Vec::new();
    let mut preprocess_paths: Vec<PathBuf> = Vec::new();

    let mut unit_binary = WrappedBinary::Gcc;
    let mut preprocess_headers_gcc_args = GCCArgs {
        ..Default::default()
    };
    let mut compile_gcc_args = GCCArgs {
        ..Default::default()
    };

    let mut global_defines = Vec::new();

    for original_object_file in original_object_files {
        let Some(info) = load_db_file(&state.conn.lock(), original_object_file) else {
            assert!(original_object_files.len() == 1);
            tokio::fs::copy(original_object_file, dst_object_file)
                .await
                .unwrap();
            return;
        };

        unit_binary = info.data.binary;
        let original_gcc_args = GCCArgs::parse(
            original_object_file,
            &osstring_to_osstr_vec(&info.data.args),
        )
        .unwrap();
        for header in &info.data.global_includes.unwrap() {
            if headers.contains(header) {
                continue;
            }
            headers.push(header.clone());
        }
        preprocess_paths.push(info.data.local_code_file.unwrap());

        global_defines.extend(info.data.include_defines.unwrap_or_default());

        preprocess_headers_gcc_args
            .user_includes
            .extend(original_gcc_args.user_includes.clone());
        compile_gcc_args
            .user_includes
            .extend(original_gcc_args.user_includes);

        preprocess_headers_gcc_args
            .system_includes
            .extend(original_gcc_args.system_includes.clone());
        compile_gcc_args
            .system_includes
            .extend(original_gcc_args.system_includes);

        preprocess_headers_gcc_args
            .f_flags
            .extend(original_gcc_args.f_flags.clone());
        compile_gcc_args.f_flags.extend(original_gcc_args.f_flags);

        preprocess_headers_gcc_args
            .g_flags
            .extend(original_gcc_args.g_flags.clone());
        compile_gcc_args.g_flags.extend(original_gcc_args.g_flags);

        preprocess_headers_gcc_args
            .opt_flags
            .extend(original_gcc_args.opt_flags.clone());
        compile_gcc_args
            .opt_flags
            .extend(original_gcc_args.opt_flags);

        preprocess_headers_gcc_args
            .defines
            .extend(original_gcc_args.defines.clone());
        compile_gcc_args.defines.extend(original_gcc_args.defines);

        preprocess_headers_gcc_args
            .machine_args
            .extend(original_gcc_args.machine_args.clone());
        compile_gcc_args
            .machine_args
            .extend(original_gcc_args.machine_args);
    }

    let (ext, lang) = if unit_binary == WrappedBinary::Gxx {
        ("ii", "c++")
    } else {
        ("i", "c")
    };

    let mut headers_code = BString::new(Vec::new());
    for define in global_defines {
        writeln!(headers_code, "{}", define).unwrap();
    }
    for header in headers {
        let needs_extern_c = lang == "c++" && state.config.lock().is_pure_c_header(&header);
        if needs_extern_c {
            writeln!(headers_code, "extern \"C\" {{").unwrap();
        }
        writeln!(headers_code, "#include \"{}\"", header.display()).unwrap();
        if needs_extern_c {
            writeln!(headers_code, "}}").unwrap();
        }
    }

    preprocess_headers_gcc_args.stop_after_preprocessing = true;
    let mut preprocess_headers_raw_args = preprocess_headers_gcc_args.to_args();
    preprocess_headers_raw_args.push("-x".into());
    preprocess_headers_raw_args.push(lang.into());
    preprocess_headers_raw_args.push("-".into());
    let mut child = tokio::process::Command::new(unit_binary.to_standard_binary_name())
        .args(&preprocess_headers_raw_args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(&headers_code).await.unwrap();
    }
    let child_result = {
        log::info!("Preprocess Header: {:?}", dst_object_file.file_name());
        let _log_handle = state.tasks_logger.start_task(&format!(
            "Preprocess Header: {:?}",
            dst_object_file.file_name()
        ));
        child.wait_with_output().await.unwrap()
    };

    let mut combined_code: String = "".to_string();
    combined_code.push_str(std::str::from_utf8(&child_result.stdout).unwrap());
    combined_code.push_str("\n\n");
    for path in preprocess_paths {
        let preprocessed_code = std::fs::read_to_string(&path).unwrap();
        combined_code.push_str(&preprocessed_code);
    }

    let unit_file = tempfile::Builder::new()
        .suffix(&format!(".{}", ext))
        .tempfile()
        .unwrap();
    let unit_file_path = unit_file.path();
    tokio::fs::write(unit_file_path, combined_code)
        .await
        .unwrap();

    compile_gcc_args.primary_output = Some(dst_object_file.to_owned());
    compile_gcc_args.sources.push(SourceFile {
        path: unit_file_path.to_owned(),
        language_override: None,
    });
    compile_gcc_args.stop_before_link = true;

    let child = tokio::process::Command::new(unit_binary.to_standard_binary_name())
        .args(compile_gcc_args.to_args())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let result = {
        log::info!("Compile unit: {:#?}", compile_gcc_args.to_args());
        let _log_handle = state
            .tasks_logger
            .start_task(&format!("Compile unit: {:?}", dst_object_file.file_name()));
        child.wait_with_output().await.unwrap()
    };
    if !result.status.success() {
        log::error!(
            "Compile unit failed: {}",
            std::str::from_utf8(&result.stderr).unwrap()
        );
        std::process::exit(1);
    }
}

async fn build_wrapped_link_units(link_units: &[WrappedLinkUnit], state: &Data<State>) {
    let link_units = link_units.to_vec();
    let handles = link_units
        .into_iter()
        .map(|unit| {
            let state_clone = state.clone();
            state.pool.run(async move || {
                build_combined_translation_unit(
                    &[&unit.original_object_path],
                    &unit.wrapped_object_path,
                    &state_clone,
                )
                .await;
            })
        })
        .collect::<Vec<_>>();
    for handle in handles {
        handle.await.unwrap();
    }
}

pub async fn handle_gcc_final_link_request(
    binary: WrappedBinary,
    request_gcc_args: &GCCArgs,
    cwd: &Path,
    state: &Data<State>,
) -> HttpResponse {
    let tmp_dir = tempfile::tempdir().unwrap();
    let Ok(smallest_link_units) = find_smallest_link_units(request_gcc_args, &state.conn.lock())
    else {
        return HttpResponse::InternalServerError().body("Failed to find link sources");
    };
    let mut wrapped_link_units = Vec::new();
    let mut unmodified_link_units = Vec::new();
    for link_unit in &smallest_link_units {
        if link_unit.extension() == Some(OsStr::new("o")) {
            wrapped_link_units.push(WrappedLinkUnit {
                original_object_path: link_unit.clone(),
                wrapped_object_path: tmp_dir.path().join(format!(
                    "{}_{}",
                    uuid::Uuid::new_v4(),
                    link_unit.file_name().unwrap().to_string_lossy()
                )),
            });
        } else {
            unmodified_link_units.push(link_unit.clone());
        }
    }
    log::info!("Building wrapped link units: {:#?}", wrapped_link_units);

    build_wrapped_link_units(&wrapped_link_units, state).await;

    let wrapped_units_archive_path = tmp_dir.path().join("wrapped_units.a");
    let wrapped_units_archive_args = ArArgs {
        flag_c: true,
        flag_q: true,
        flag_s: true,
        thin_archive: true,
        output: Some(wrapped_units_archive_path.clone()),
        sources: wrapped_link_units
            .iter()
            .map(|u| u.wrapped_object_path.clone())
            .collect(),
    };
    {
        let _task_handle = state.tasks_logger.start_task(&format!(
            "Build thin archive: {}",
            wrapped_units_archive_path.to_string_lossy()
        ));
        tokio::process::Command::new(WrappedBinary::Ar.to_standard_binary_name())
            .args(wrapped_units_archive_args.to_args())
            .current_dir(cwd)
            .spawn()
            .unwrap()
            .wait_with_output()
            .await
            .unwrap();
    }

    let mut modified_gcc_args = request_gcc_args.clone();
    modified_gcc_args.sources = vec![];
    modified_gcc_args.sources.push(SourceFile {
        path: wrapped_units_archive_path.clone(),
        language_override: None,
    });
    modified_gcc_args.sources.extend(
        unmodified_link_units
            .iter()
            .map(|w| SourceFile {
                path: w.clone(),
                language_override: None,
            })
            .collect::<Vec<_>>(),
    );

    let _link_task_handle = state.tasks_logger.start_task(&format!(
        "Link: {}",
        modified_gcc_args
            .primary_output
            .as_ref()
            .unwrap_or(&PathBuf::from(""))
            .to_string_lossy()
    ));

    modified_gcc_args.use_link_group = true;
    log::info!("Link: {:#?}", modified_gcc_args.to_args());
    let child = tokio::process::Command::new(binary.to_standard_binary_name())
        .args(modified_gcc_args.to_args())
        .current_dir(cwd)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn();
    let Ok(child) = child else {
        return HttpResponse::InternalServerError().body("Failed to spawn child");
    };
    let Ok(child_result) = child.wait_with_output().await else {
        return HttpResponse::InternalServerError().body("Failed to wait on child");
    };
    HttpResponse::Ok().json(
        ccelerate_shared::RunResponseData {
            stdout: child_result.stdout,
            stderr: child_result.stderr,
            status: child_result.status.code().unwrap_or(1),
        }
        .to_wire(),
    )
}
