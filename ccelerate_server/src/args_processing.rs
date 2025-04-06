use std::{
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
};

use anyhow::Result;
use anyhow::anyhow;
use bstr::BString;
use ccelerate_shared::WrappedBinary;
use smallvec::SmallVec;

use crate::{code_language::CodeLanguage, gcc_args, source_file::SourceFile};

pub struct BuildObjectFileInfo {
    pub source_path: PathBuf,
    pub source_language: CodeLanguage,
    pub object_path: PathBuf,
}

impl BuildObjectFileInfo {
    pub fn from_args(
        binary: WrappedBinary,
        cwd: &Path,
        args: &[impl AsRef<OsStr>],
    ) -> Result<Self> {
        match binary {
            binary if binary.is_gcc_compatible() => Self::from_gcc_args(cwd, args),
            _ => Err(anyhow!(
                "Cannot extract build object args for binary: {:?}",
                binary
            )),
        }
    }
}

pub fn rewrite_to_extract_local_code(
    binary: WrappedBinary,
    args: &[impl AsRef<OsStr>],
) -> Result<Vec<OsString>> {
    match binary {
        binary if binary.is_gcc_compatible() => gcc_args::rewrite_to_extract_local_code(args),
        _ => Err(anyhow!("Cannot rewrite args for binary: {:?}", binary)),
    }
}

pub fn rewrite_to_get_preprocessed_headers(
    binary: WrappedBinary,
    args: &[impl AsRef<OsStr>],
    include_code_path: &Path,
    output_path: &Path,
) -> Result<Vec<OsString>> {
    match binary {
        binary if binary.is_gcc_compatible() => {
            gcc_args::rewrite_to_get_preprocessed_headers(args, include_code_path, output_path)
        }
        _ => Err(anyhow!("Cannot rewrite args for binary: {:?}", binary)),
    }
}

pub fn rewrite_to_link_sources(
    binary: WrappedBinary,
    args: &[impl AsRef<OsStr>],
    new_sources: &[impl AsRef<Path>],
) -> Result<Vec<OsString>> {
    match binary {
        binary if binary.is_gcc_compatible() => gcc_args::rewrite_to_link_sources(
            args,
            &new_sources
                .iter()
                .map(|s| SourceFile {
                    path: s.as_ref().to_owned(),
                    language_override: None,
                })
                .collect::<Vec<_>>(),
        ),
        _ => Err(anyhow!("Cannot rewrite args for binary: {:?}", binary)),
    }
}

pub struct LinkFileInfo {
    pub sources: SmallVec<[SourceFile; 16]>,
    pub output: PathBuf,
}

impl LinkFileInfo {
    pub fn from_args(
        binary: WrappedBinary,
        cwd: &Path,
        args: &[impl AsRef<OsStr>],
    ) -> Result<Self> {
        match binary {
            binary if binary.is_gcc_compatible() => Self::from_gcc_args(cwd, args),
            _ => Err(anyhow!(
                "Cannot extract build object args for binary: {:?}",
                binary
            )),
        }
    }
}

pub fn add_object_compatibility_args_to_key(
    binary: WrappedBinary,
    args: &[impl AsRef<OsStr>],
    key: &mut BString,
) -> Result<()> {
    match binary {
        binary if binary.is_gcc_compatible() => {
            gcc_args::add_translation_unit_unspecific_args_to_key(args, key)
        }
        _ => Err(anyhow!(
            "Cannot add object compatibility args for binary: {:?}",
            binary
        )),
    }
}
