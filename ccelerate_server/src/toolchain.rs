use std::{ffi::OsStr, path::Path};

use ccelerate_shared::WrappedBinary;

pub struct Toolchain {}

pub struct RunResult {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub exit_code: i32,
}

impl Toolchain {
    pub fn new() -> Self {
        Self {}
    }

    pub fn run(&self, binary: WrappedBinary, cwd: &Path, args: &[&OsStr]) -> RunResult {
        RunResult::new_success()
    }
}

impl RunResult {
    pub fn new_success() -> RunResult {
        RunResult {
            stdout: Vec::new(),
            stderr: Vec::new(),
            exit_code: 0,
        }
    }
}
