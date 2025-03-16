use actix_web::{HttpResponse, web::Data};
use anyhow::Result;
use ccelerate_shared::WrappedBinary;
use parking_lot::Mutex;
use std::{
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
};

use crate::{DbFilesRow, DbFilesRowData, HeaderType, State, parse_gcc::GCCArgs};

#[derive(Debug, Clone, Default)]
struct SourceCodeLine<'a> {
    line_number: usize,
    line: &'a str,
}

#[derive(Debug, Clone, Default)]
struct AnalysePreprocessResult<'a> {
    global_headers: Vec<PathBuf>,
    local_code: Vec<SourceCodeLine<'a>>,
}

fn find_global_include_extra_defines(global_headers: &[PathBuf], code: &str) -> Vec<String> {
    let code = code.replace("\\\n", " ");

    static INCLUDE_RE: once_cell::sync::Lazy<regex::Regex> = once_cell::sync::Lazy::new(|| {
        regex::Regex::new(r#"(?m)^#include[ \t]+["<](.*)[">]"#).expect("should be valid")
    });
    let mut last_global_include_index = 0;
    for captures in INCLUDE_RE.captures_iter(&code) {
        let Some(header_name) = captures.get(1).map(|m| m.as_str()) else {
            continue;
        };
        if !global_headers.iter().any(|h| h.ends_with(header_name)) {
            continue;
        }
        last_global_include_index = captures.get(0).expect("group should exist").start();
    }

    static DEFINE_RE: once_cell::sync::Lazy<regex::Regex> = once_cell::sync::Lazy::new(|| {
        regex::Regex::new(r#"(?m)^#define.*$"#).expect("should be valid")
    });

    let mut result = vec![];
    for captures in DEFINE_RE.captures_iter(&code[..last_global_include_index]) {
        result.push(
            captures
                .get(0)
                .expect("group should exist")
                .as_str()
                .to_string(),
        );
    }
    if code.contains("#define NPY_NO_DEPRECATED_API NPY_1_7_API_VERSION") {
        result.push("#define NPY_NO_DEPRECATED_API NPY_1_7_API_VERSION".to_string());
    }
    if code.contains("#  define NANOVDB_USE_OPENVDB") {
        result.push("#define NANOVDB_USE_OPENVDB".to_string());
    }
    result
}

#[derive(Debug, Clone, Default)]
struct GccPreprocessLine<'a> {
    line_number: usize,
    header_name: &'a str,
    is_start_of_new_file: bool,
    is_return_to_file: bool,
    _next_is_system_header: bool,
    _next_is_extern_c: bool,
}

impl<'a> GccPreprocessLine<'a> {
    fn parse(line: &'a [u8]) -> Result<Self> {
        let line = std::str::from_utf8(line)?;
        let err = || anyhow::anyhow!("Failed to parse line: {:?}", line);
        static RE: once_cell::sync::Lazy<regex::Regex> = once_cell::sync::Lazy::new(|| {
            regex::Regex::new(r#"# (\d+) "(.*)"\s*(\d?)\s*(\d?)\s*(\d?)\s*(\d?)"#)
                .expect("should be valid")
        });
        let Some(captures) = RE.captures(line) else {
            return Err(err());
        };
        let Some(line_number) = captures
            .get(1)
            .expect("group should exist")
            .as_str()
            .parse::<usize>()
            .ok()
        else {
            return Err(err());
        };
        let name = captures.get(2).expect("group should exist").as_str();
        let mut numbers = vec![];
        for i in 3..=6 {
            let number_str = captures.get(i).expect("group should exist").as_str();
            if number_str.is_empty() {
                continue;
            }
            let Some(number) = number_str.parse::<i32>().ok() else {
                return Err(err());
            };
            numbers.push(number);
        }

        Ok(GccPreprocessLine {
            line_number,
            header_name: name,
            is_start_of_new_file: numbers.contains(&1),
            is_return_to_file: numbers.contains(&2),
            _next_is_system_header: numbers.contains(&3),
            _next_is_extern_c: numbers.contains(&4),
        })
    }
}

fn _has_known_include_guard(code: &[u8]) -> bool {
    let mut i = 0;
    while i < code.len() {
        let rest = &code[i..];
        if rest.starts_with(b"#pragma once") {
            return true;
        }
        if rest.starts_with(b"#ifndef") {
            return true;
        }
        if rest.starts_with(b"#include") {
            /* The include guard has to come before the first include. */
            return false;
        }
        if rest.starts_with(b"//") {
            i += rest.iter().position(|&b| b == b'\n').unwrap_or(rest.len()) + 1;
            continue;
        }
        if rest.starts_with(b"/*") {
            i += rest
                .windows(2)
                .position(|w| w == b"*/")
                .unwrap_or(rest.len())
                + 2;
            continue;
        }
        if b" \t\n".contains(&rest[0]) {
            i += 1;
            continue;
        }
        return false;
    }
    false
}

async fn is_local_header(header_path: &Path) -> bool {
    // if header_path.starts_with("/usr") {
    //     return false;
    // }
    // let Ok(code) = tokio::fs::read_to_string(header_path).await else {
    //     return false;
    // };
    // !has_known_include_guard(&code.as_bytes())
    header_path.ends_with("list_sort_impl.h")
        || header_path.ends_with("dna_rename_defs.h")
        || header_path.ends_with("dna_includes_as_strings.h")
        || header_path.ends_with("BLI_strict_flags.h")
        || header_path.ends_with("RNA_enum_items.hh")
        || header_path.ends_with("UI_icons.hh")
        || header_path.extension().is_some_and(|ext| ext == "cc")
        || header_path.extension().is_some_and(|ext| ext == "c")
        || header_path.ends_with("glsl_compositor_source_list.h")
        || header_path.ends_with("BLI_kdtree_impl.h")
        || header_path.ends_with("kdtree_impl.h")
        || header_path.ends_with("state_template.h")
        || header_path.ends_with("shadow_state_template.h")
        || header_path.ends_with("gpu_shader_create_info_list.hh")
        || header_path.ends_with("generic_alloc_impl.h")
        || header_path.ends_with("glsl_draw_source_list.h")
        || header_path.ends_with("compositor_shader_create_info_list.hh")
        || header_path.ends_with("glsl_gpu_source_list.h")
        || header_path.ends_with("glsl_osd_source_list.h")
        || header_path.ends_with("glsl_ocio_source_list.h")
        || header_path.ends_with("draw_debug_info.hh")
        || header_path.ends_with("draw_fullscreen_info.hh")
        || header_path.ends_with("draw_hair_refine_info.hh")
        || header_path.ends_with("draw_object_infos_info.hh")
        || header_path.ends_with("draw_view_info.hh")
        || header_path.ends_with("subdiv_info.hh")
        || header_path
            .as_os_str()
            .to_string_lossy()
            .contains("shaders/infos")
}

async fn is_local_header_with_cache(header_path: &Path, state: &Data<State>) -> bool {
    if let Some(header_type) = state.header_type_cache.lock().get(header_path) {
        return *header_type == HeaderType::Local;
    }
    let result = is_local_header(header_path).await;
    state.header_type_cache.lock().insert(
        header_path.to_owned(),
        if result {
            HeaderType::Local
        } else {
            HeaderType::Global
        },
    );
    result
}

#[tokio::test]
async fn test_is_local_header() {
    assert!(
        is_local_header(Path::new(
            "/home/jacques/blender/blender/source/blender/blenlib/intern/list_sort_impl.h"
        ))
        .await
    );
    assert!(
        is_local_header(Path::new(
            "/home/jacques/blender/blender/source/blender/gpu/intern/gpu_shader_create_info_list.hh"
        ))
        .await
    );
    assert!(
        !is_local_header(Path::new(
            "/home/jacques/blender/blender/source/blender/blenlib/BLI_path_utils.hh"
        ))
        .await
    );
    assert!(!is_local_header(Path::new("/usr/include/c++/14/cstddef")).await);
}

async fn analyse_preprocessed_file<'a>(
    code: &'a [u8],
    state: &Data<State>,
) -> Result<AnalysePreprocessResult<'a>> {
    let mut result = AnalysePreprocessResult::default();
    let mut header_stack: Vec<&str> = vec![];
    let mut local_depth = 0;
    let mut next_line = 0;

    for line in code.split(|&b| b == b'\n') {
        let is_local = header_stack.len() == local_depth;
        if line.starts_with(b"# ") {
            let preprocessor_line = GccPreprocessLine::parse(line)?;
            next_line = preprocessor_line.line_number;
            if preprocessor_line.is_start_of_new_file {
                if is_local {
                    if !is_local_header_with_cache(Path::new(preprocessor_line.header_name), state)
                        .await
                    {
                        let header_path = PathBuf::from(preprocessor_line.header_name);
                        if !result.global_headers.contains(&header_path) {
                            result.global_headers.push(header_path);
                        }
                    } else {
                        local_depth += 1;
                    }
                }
                header_stack.push(preprocessor_line.header_name);
            } else if preprocessor_line.is_return_to_file {
                header_stack.pop();
                local_depth = local_depth.min(header_stack.len());
            }
        } else {
            if !line.is_empty() && is_local {
                result.local_code.push(SourceCodeLine {
                    line_number: next_line,
                    line: std::str::from_utf8(line)?,
                });
            }
            next_line += 1;
        }
    }
    Ok(result)
}

pub async fn handle_gcc_without_link_request(
    binary: WrappedBinary,
    request_gcc_args: &GCCArgs,
    cwd: &Path,
    state: &Data<State>,
) -> HttpResponse {
    let Some(request_output_path) = request_gcc_args.primary_output.as_ref() else {
        return HttpResponse::NotImplemented().body("Expected output path");
    };
    let Some(request_output_name) = request_output_path.file_name() else {
        return HttpResponse::BadRequest().body("Expected output path to have a file name");
    };

    let _log_handle = state.tasks_logger.start_task(&format!(
        "Prepare: {:?}",
        request_output_name.to_string_lossy()
    ));

    // Do preprocessing on the provided files. This also generates a depfile that e.g. ninja will use
    // to know which headers an object file depends on.
    let mut modified_gcc_args = request_gcc_args.clone();
    modified_gcc_args.primary_output = None;
    modified_gcc_args.stop_before_link = false;
    modified_gcc_args.stop_after_preprocessing = true;

    let realized_args = modified_gcc_args.to_args();
    let realized_args_buffer = realized_args
        .iter()
        .flat_map(|s| s.as_encoded_bytes())
        .copied()
        .collect::<Vec<_>>();
    let realized_args_hash = twox_hash::XxHash64::oneshot(0, &realized_args_buffer);
    let realized_args_hash_str = format!("{:x}", realized_args_hash);
    let preprocess_file_name = format!(
        "{}_{}.ii",
        &realized_args_hash_str,
        request_output_name.to_string_lossy()
    );
    let preprocess_file_dir = state
        .data_dir
        .join("preprocessed")
        .join(&realized_args_hash_str[..2]);
    let preprocess_file_path = preprocess_file_dir.join(preprocess_file_name);
    match std::fs::create_dir_all(preprocess_file_dir) {
        Ok(()) => {}
        Err(e) => {
            return HttpResponse::InternalServerError()
                .body(format!("Failed to create preprocess file directory: {e}"));
        }
    }
    let headers = Arc::new(Mutex::new(Vec::new()));
    let global_defines = Arc::new(Mutex::new(Vec::new()));

    log::info!("Preprocess: {:#?}", modified_gcc_args.to_args());
    {
        let preprocess_file_path = preprocess_file_path.clone();
        let headers = headers.clone();
        let global_defines = global_defines.clone();
        let state_clone = state.clone();
        state
            .pool
            .run(async move || {
                let result = tokio::process::Command::new(binary.to_standard_binary_name())
                    .args(&realized_args)
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped())
                    .spawn()
                    .unwrap()
                    .wait_with_output()
                    .await
                    .unwrap();
                if !result.stderr.is_empty() {
                    log::error!(
                        "Preprocess failed: {}",
                        std::str::from_utf8(&result.stderr).unwrap()
                    );
                    std::process::exit(1);
                }
                let analysis = analyse_preprocessed_file(&result.stdout, &state_clone)
                    .await
                    .unwrap();
                let source_file_path = modified_gcc_args.sources.first();
                if let Some(source_file_path) = source_file_path {
                    if let Ok(source_code) = tokio::fs::read_to_string(&source_file_path.path).await
                    {
                        global_defines
                            .lock()
                            .extend(find_global_include_extra_defines(
                                &analysis.global_headers,
                                &source_code,
                            ));
                    }
                }

                headers.lock().extend(analysis.global_headers);

                let mut file = std::fs::File::create(preprocess_file_path).unwrap();
                for line in analysis.local_code {
                    if let Some(source_file_path) = source_file_path {
                        writeln!(
                            file,
                            "# {} \"{}\"",
                            line.line_number,
                            source_file_path.path.display()
                        )
                        .unwrap();
                    }
                    writeln!(file, "{}", line.line).unwrap();
                }
            })
            .await
            .unwrap();
    }

    let dummy_object = crate::ASSETS_DIR
        .get_file("dummy_object.o")
        .expect("file should exist");
    match tokio::fs::write(request_output_path, dummy_object.contents()).await {
        Ok(_) => {}
        Err(e) => {
            return HttpResponse::InternalServerError().body(format!(
                "Failed to write dummy object to {}: {}",
                request_output_path.display(),
                e
            ));
        }
    }

    let Ok(_) = crate::store_db_file(
        &state.conn.lock(),
        &DbFilesRow {
            path: request_output_path.clone(),
            data: DbFilesRowData {
                cwd: cwd.to_path_buf(),
                binary,
                args: request_gcc_args.to_args(),
                local_code_file: Some(preprocess_file_path),
                headers: Some(headers.lock().clone()),
                global_defines: Some(global_defines.lock().clone()),
            },
        },
    ) else {
        return HttpResponse::InternalServerError().body("Failed to store db file");
    };

    HttpResponse::Ok().json(&ccelerate_shared::RunResponseDataWire {
        ..Default::default()
    })
}
