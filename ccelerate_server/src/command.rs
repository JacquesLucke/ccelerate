use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use crate::parse_ar::ArArgs;
use crate::parse_gcc::GCCArgs;
use anyhow::Result;

#[derive(Debug, Clone)]
pub struct Command {
    pub binary: String,
    pub cwd: PathBuf,
    pub args: CommandArgs,
}

#[derive(Debug, Clone)]
pub enum CommandArgs {
    Ar(ArArgs),
    Gcc(GCCArgs),
}

impl Command {
    pub fn new(binary: &str, cwd: &Path, raw_args: &[&OsStr]) -> Result<Self> {
        match binary {
            "ar" => Ok(Command {
                binary: binary.to_string(),
                cwd: cwd.to_path_buf(),
                args: CommandArgs::Ar(ArArgs::parse(cwd, raw_args)?),
            }),
            "gcc" | "g++" | "clang" | "clang++" => Ok(Command {
                binary: binary.to_string(),
                cwd: cwd.to_path_buf(),
                args: CommandArgs::Gcc(GCCArgs::parse(cwd, raw_args)?),
            }),
            _ => Err(anyhow::anyhow!("Unknown binary: {}", binary)),
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
        Ok(tokio::process::Command::new(&self.binary)
            .args(args)
            .current_dir(&self.cwd)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()?)
    }
}
