#![deny(clippy::unwrap_used)]

use std::path::PathBuf;

use anyhow::Result;

use crate::{
    database::{FileRecord, load_file_record},
    parse_ar::ArArgs,
    parse_gcc::GCCArgs,
};

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
            if extension == ".a" {
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
            } else if extension == ".o" {
                if let Some(record) = record {
                    if !record.binary.is_gcc_compatible() {
                        return Err(anyhow::anyhow!(
                            "Object file not created by gcc: {}",
                            current_source.display()
                        ));
                    }
                    let gcc_args = GCCArgs::parse_owned(&record.cwd, record.args)?;
                    if gcc_args.sources.len() != 1 {
                        return Err(anyhow::anyhow!(
                            "Object file has more than one source: {}",
                            current_source.display()
                        ));
                    }
                    remaining_sources.push(gcc_args.sources[0].path.clone());
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
