use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
};

use anyhow::Result;
use anyhow::anyhow;
use ccelerate_shared::WrappedBinary;

use crate::code_language::CodeLanguage;

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
            WrappedBinary::Gcc
            | WrappedBinary::Gxx
            | WrappedBinary::Clang
            | WrappedBinary::Clangxx => Self::from_gcc_args(cwd, args),
            _ => Err(anyhow!(
                "Cannot extract build object args for binary: {:?}",
                binary
            )),
        }
    }
}
