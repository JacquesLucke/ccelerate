use std::{
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
};

use anyhow::Result;
use anyhow::anyhow;
use ccelerate_shared::WrappedBinary;

use crate::{code_language::CodeLanguage, gcc_args};

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
