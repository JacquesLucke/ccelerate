use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use crate::parse_ar::ArArgs;
use crate::parse_gcc::GCCArgs;
use anyhow::Result;
use ccelerate_shared::WrappedBinary;

#[derive(Debug, Clone)]
pub struct Command {
    pub binary: WrappedBinary,
    pub cwd: PathBuf,
    pub args: CommandArgs,
}

#[derive(Debug, Clone)]
pub enum CommandArgs {
    Ar(ArArgs),
    Gcc(GCCArgs),
}

impl Command {
    pub fn new(binary: WrappedBinary, cwd: &Path, raw_args: &[&OsStr]) -> Result<Self> {
        match binary {
            WrappedBinary::Ar => Ok(Command {
                binary: binary,
                cwd: cwd.to_path_buf(),
                args: CommandArgs::Ar(ArArgs::parse(cwd, raw_args)?),
            }),
            WrappedBinary::Gcc
            | WrappedBinary::Gxx
            | WrappedBinary::Clang
            | WrappedBinary::Clangxx => Ok(Command {
                binary: binary,
                cwd: cwd.to_path_buf(),
                args: CommandArgs::Gcc(GCCArgs::parse(cwd, raw_args)?),
            }),
        }
    }
}
