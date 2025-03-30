#![deny(clippy::unwrap_used)]

use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::Result;
use ccelerate_shared::WrappedBinary;
use parking_lot::Mutex;
use serde::Deserialize;

use crate::{parse_ar::ArArgs, parse_gcc::GCCArgs};

pub struct Toolchain {
    config: Mutex<Arc<ToolchainConfig>>,
}

#[derive(Debug)]
pub struct RunResult {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub exit_code: i32,
}

#[derive(Debug)]
struct ToolchainConfig {
    config_files: Vec<PathBuf>,
    configured_dirs: Vec<PathBuf>,
    eager_patterns: Vec<regex::bytes::Regex>,
    local_header_patterns: Vec<regex::bytes::Regex>,
    include_defines: Vec<regex::bytes::Regex>,
    pure_c_header_patterns: Vec<regex::bytes::Regex>,
    bad_global_symbols_patterns: Vec<regex::bytes::Regex>,
}

#[derive(Debug, Deserialize)]
struct ToolchainConfigFile {
    eager_patterns: Vec<String>,
    local_header_patterns: Vec<String>,
    include_defines: Vec<String>,
    pure_c_header_patterns: Vec<String>,
    bad_global_symbols_patterns: Vec<String>,
}

impl Toolchain {
    pub fn new() -> Self {
        Self {
            config: Mutex::new(Arc::new(ToolchainConfig::new())),
        }
    }

    pub async fn run<S: AsRef<OsStr>>(
        &self,
        binary: WrappedBinary,
        cwd: &Path,
        args: &[S],
    ) -> RunResult {
        match self.run_error_handling(binary, cwd, args).await {
            Ok(result) => result,
            Err(e) => RunResult::new_error(e),
        }
    }

    pub async fn run_error_handling<S: AsRef<OsStr>>(
        &self,
        binary: WrappedBinary,
        cwd: &Path,
        args: &[S],
    ) -> Result<RunResult> {
        match binary {
            WrappedBinary::Ar => self.run_ar(cwd, args).await,
            WrappedBinary::Gcc
            | WrappedBinary::Gxx
            | WrappedBinary::Clang
            | WrappedBinary::Clangxx => self.run_gcc(cwd, args).await,
        }
    }

    async fn run_ar<S: AsRef<OsStr>>(&self, cwd: &Path, args: &[S]) -> Result<RunResult> {
        let args = ArArgs::parse(cwd, args)?;
        let mut paths_for_config = vec![];
        paths_for_config.push(cwd);
        for path in &args.sources {
            paths_for_config.push(path.as_path());
        }
        let Some(output) = &args.output else {
            return Ok(RunResult::new_error_str("Missing output"));
        };
        paths_for_config.push(output.as_path());
        let config = self.get_config_for_paths(&paths_for_config).await?;
        Ok(RunResult::new_success())
    }

    async fn run_gcc<S: AsRef<OsStr>>(&self, cwd: &Path, args: &[S]) -> Result<RunResult> {
        let args = GCCArgs::parse(cwd, args)?;
        Ok(RunResult::new_success())
    }

    async fn get_config_for_paths<P: AsRef<Path>>(
        &self,
        paths: &[P],
    ) -> Result<Arc<ToolchainConfig>> {
        let mut config = self.config.lock();
        for path in paths {
            if config.contains_path(path.as_ref()) {
                return Ok(config.clone());
            }
        }
        let Some(config_path) = ToolchainConfig::try_find_config_file_for_path(paths[0].as_ref())
        else {
            return Ok(config.clone());
        };
        let new_config = config.copy_with_new_config(&config_path).await?;
        *config = Arc::new(new_config);
        Ok(config.clone())
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

    pub fn new_error(error: anyhow::Error) -> RunResult {
        RunResult {
            stdout: Vec::new(),
            stderr: format!("{error}").into_bytes(),
            exit_code: 1,
        }
    }

    pub fn new_error_str(error: &str) -> RunResult {
        RunResult {
            stdout: Vec::new(),
            stderr: error.into(),
            exit_code: 1,
        }
    }
}

impl ToolchainConfig {
    fn new() -> Self {
        ToolchainConfig {
            config_files: Vec::new(),
            configured_dirs: Vec::new(),
            eager_patterns: Vec::new(),
            local_header_patterns: Vec::new(),
            include_defines: Vec::new(),
            pure_c_header_patterns: Vec::new(),
            bad_global_symbols_patterns: Vec::new(),
        }
    }

    async fn new_from_files<P: AsRef<Path>>(config_files: &[P]) -> Result<Self> {
        let mut config = ToolchainConfig::new();
        for path in config_files {
            let config_file = tokio::fs::read_to_string(path).await?;
            let config_file: ToolchainConfigFile = toml::from_str(config_file.as_str())?;
            config.config_files.push(path.as_ref().to_owned());
            config.configured_dirs.push(
                path.as_ref()
                    .parent()
                    .expect("files should always have a parent")
                    .to_owned(),
            );

            macro_rules! add_patterns {
                ($field:ident) => {
                    for pattern in config_file.$field.iter() {
                        let regex = regex::bytes::Regex::new(pattern)?;
                        config.$field.push(regex);
                    }
                };
            }

            add_patterns!(eager_patterns);
            add_patterns!(local_header_patterns);
            add_patterns!(include_defines);
            add_patterns!(pure_c_header_patterns);
            add_patterns!(bad_global_symbols_patterns);
        }

        Ok(config)
    }

    async fn copy_with_new_config(&self, new_config_path: &Path) -> Result<Self> {
        let mut config_paths = self.config_files.clone();
        config_paths.push(new_config_path.into());
        ToolchainConfig::new_from_files(&config_paths).await
    }

    fn contains_path(&self, path: &Path) -> bool {
        self.configured_dirs.iter().any(|dir| path.starts_with(dir))
    }

    fn try_find_config_file_for_path(path: &Path) -> Option<PathBuf> {
        let ancestors: Vec<_> = path.ancestors().collect();
        for ancestor in ancestors.into_iter().rev() {
            let config_path = ancestor.join("ccelerate.toml");
            if config_path.exists() {
                return Some(config_path);
            }
        }
        None
    }
}
