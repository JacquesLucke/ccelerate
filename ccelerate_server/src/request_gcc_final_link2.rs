#![deny(clippy::unwrap_used)]

use std::{
    collections::HashMap,
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
};

use actix_web::{HttpResponse, web::Data};
use anyhow::Result;
use ccelerate_shared::WrappedBinary;

use crate::{
    database::{FileRecord, load_file_record},
    parse_ar::ArArgs,
    parse_gcc::{GCCArgs, SourceFile},
    state::State,
};

#[derive(Debug)]
struct OriginalLinkSource {
    path: PathBuf,
    record: Option<FileRecord>,
}

fn find_smallest_link_sources(
    root_args: &GCCArgs,
    conn: &rusqlite::Connection,
) -> Result<Vec<OriginalLinkSource>> {
    let mut smallest_link_sources = Vec::new();
    let mut remaining_sources = Vec::new();
    for arg in &root_args.sources {
        remaining_sources.push(arg.path.clone());
    }
    while let Some(current_source) = remaining_sources.pop() {
        let record = load_file_record(conn, &current_source);
        if let Some(extension) = current_source.extension() {
            if extension == "a" {
                if let Some(record) = record {
                    if !record.binary.is_ar_compatible() {
                        return Err(anyhow::anyhow!(
                            "Archive not created by ar: {}",
                            current_source.display()
                        ));
                    }
                    let ar_args = ArArgs::parse_owned(&record.cwd, record.args)?;
                    remaining_sources.extend(ar_args.sources.iter().cloned());
                    continue;
                }
            } else if extension == "o" {
                if let Some(record) = record {
                    if !record.binary.is_gcc_compatible() {
                        return Err(anyhow::anyhow!(
                            "Object file not created by gcc: {}",
                            current_source.display()
                        ));
                    }
                    smallest_link_sources.push(OriginalLinkSource {
                        path: current_source,
                        record: Some(record),
                    });
                    continue;
                }
            }
        }
        smallest_link_sources.push(OriginalLinkSource {
            path: current_source,
            record: None,
        });
    }
    Ok(smallest_link_sources)
}

async fn build_final_link_sources(
    state: &Data<State>,
    original_sources: &[OriginalLinkSource],
) -> Result<Vec<PathBuf>> {
    let mut final_link_sources = Vec::new();
    let mut remaining_source_args = Vec::new();
    for original_source in original_sources {
        if let Some(record) = &original_source.record {
            if original_source.path.extension() == Some(OsStr::new("o")) {
                if let Ok(gcc_args) =
                    GCCArgs::parse_owned(&original_source.path, record.args.clone())
                {
                    remaining_source_args.push(gcc_args);
                }
                continue;
            }
        }
        final_link_sources.push(original_source.path.clone());
    }

    let mut possible_compile_groups: HashMap<OsString, Vec<GCCArgs>> = HashMap::new();
    for source_args in remaining_source_args {
        // Todo: Take header defines into account for grouping.
        let mut stripped_source_args = source_args.clone();
        stripped_source_args.sources.clear();
        stripped_source_args.primary_output = None;
        stripped_source_args.depfile_output_path = None;
        stripped_source_args.depfile_target_name = None;
        stripped_source_args.depfile_generate = false;
        let stripped_args = stripped_source_args.to_args().join(OsStr::new(" "));
        possible_compile_groups
            .entry(stripped_args)
            .or_default()
            .push(source_args);
    }

    for (stripped_args, compile_group) in possible_compile_groups {
        println!("Compile group: {:?}", stripped_args);
        for compile_args in compile_group {
            println!("  {:?}", compile_args.sources);
        }
    }

    Ok(final_link_sources)
}

pub async fn handle_gcc_final_link_request2(
    binary: WrappedBinary,
    request_gcc_args: &GCCArgs,
    cwd: &Path,
    state: &Data<State>,
) -> HttpResponse {
    let Ok(original_link_sources) =
        find_smallest_link_sources(request_gcc_args, &state.conn.lock())
    else {
        return HttpResponse::BadRequest().body("Error finding smallest link sources");
    };
    let Ok(final_link_sources) = build_final_link_sources(state, &original_link_sources).await
    else {
        return HttpResponse::BadRequest().body("Error building final link sources");
    };

    let mut final_link_args = request_gcc_args.clone();
    final_link_args.sources = final_link_sources
        .iter()
        .map(|s| SourceFile {
            path: s.clone(),
            language_override: None,
        })
        .collect();
    final_link_args.use_link_group = true;
    HttpResponse::Ok().body("todo")
}
