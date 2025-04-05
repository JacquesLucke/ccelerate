#![deny(clippy::unwrap_used)]

use std::ffi::OsStr;
use std::ffi::OsString;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Result;
use anyhow::anyhow;
use os_str_bytes::OsStrBytesExt;
use smallvec::SmallVec;

use crate::path_utils::make_absolute;

pub struct BuildStaticArchiveInfo {
    pub archive_path: PathBuf,
    pub archive_name: OsString,
    pub member_paths: SmallVec<[PathBuf; 16]>,
}

impl BuildStaticArchiveInfo {
    pub fn from_args(cwd: &Path, args: &[impl AsRef<OsStr>]) -> Result<BuildStaticArchiveInfo> {
        let args = parse_ar_args(args)?;
        if !args.operation.contains("c") {
            return Err(anyhow!("arguments don't create archive, no 'c' flag"));
        }
        let archive_path = make_absolute(cwd, Path::new(args.archive));
        let Some(archive_name) = archive_path.file_name() else {
            return Err(anyhow!("archive path has no file name"));
        };
        Ok(BuildStaticArchiveInfo {
            archive_name: archive_name.to_owned(),
            archive_path,
            member_paths: args
                .members
                .iter()
                .map(|s| make_absolute(cwd, Path::new(s)))
                .collect(),
        })
    }
}

pub fn make_args_to_build_thin_static_archive(
    archive_path: &Path,
    member_paths: &[impl AsRef<Path>],
) -> Vec<OsString> {
    let mut args: Vec<OsString> = vec![];
    args.push("qc".into());
    args.push("--thin".into());
    args.push(archive_path.into());
    for member_path in member_paths {
        args.push(member_path.as_ref().into());
    }
    args
}

// https://sourceware.org/binutils/docs/binutils/ar-cmdline.html
struct ArArgs<'a> {
    operation: &'a OsStr,
    archive: &'a OsStr,
    members: SmallVec<[&'a OsStr; 16]>,
}

fn parse_ar_args(args: &[impl AsRef<OsStr>]) -> Result<ArArgs> {
    let mut operation = None;
    let mut archive = None;
    let mut members = SmallVec::new();

    for arg in args {
        let arg = arg.as_ref();
        if arg == "-X32_64" {
            continue;
        }
        if operation.is_none() {
            operation = Some(arg);
            continue;
        }
        if arg.starts_with("--") {
            continue;
        }
        if archive.is_none() {
            archive = Some(arg);
            continue;
        }
        members.push(arg);
    }
    Ok(ArArgs {
        operation: operation.ok_or_else(|| anyhow!("missing operation"))?,
        archive: archive.ok_or_else(|| anyhow!("missing archive"))?,
        members,
    })
}
