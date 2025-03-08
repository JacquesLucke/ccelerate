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

    pub fn primary_output_path(&self) -> Option<PathBuf> {
        match &self.args {
            CommandArgs::Ar(args) => args.output.clone(),
            CommandArgs::Gcc(args) => args.primary_output.clone(),
        }
    }

    pub fn to_args(&self) -> Vec<OsString> {
        match &self.args {
            CommandArgs::Ar(args) => args.to_args(),
            CommandArgs::Gcc(args) => args.to_args(),
        }
    }

    pub fn run(&self) -> Result<tokio::process::Child> {
        let args = self.to_args();
        Ok(
            tokio::process::Command::new(&self.binary.to_standard_binary_name())
                .args(args)
                .current_dir(&self.cwd)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()?,
        )
    }
}
