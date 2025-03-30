use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::Result;
use bstr::{BStr, BString};
use parking_lot::Mutex;
use serde::Deserialize;

pub struct ConfigManager {
    state: Mutex<ConfigState>,
}

struct ConfigState {
    config: Arc<Config>,
    config_files: Vec<PathBuf>,
    included_dirs: HashSet<PathBuf>,
    dirs_without_config: HashSet<PathBuf>,
}

pub struct Config {
    eager_patterns: Vec<glob::Pattern>,
    local_header_patterns: Vec<glob::Pattern>,
    include_defines: Vec<BString>,
    pure_c_header_patterns: Vec<glob::Pattern>,
    bad_global_symbols_patterns: Vec<glob::Pattern>,
}

#[derive(Debug, Deserialize)]
struct ConfigFile {
    eager_patterns: Vec<String>,
    local_header_patterns: Vec<String>,
    include_defines: Vec<String>,
    pure_c_header_patterns: Vec<String>,
    bad_global_symbols_patterns: Vec<String>,
}

impl ConfigManager {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(ConfigState {
                config: Arc::new(Config::new()),
                config_files: Vec::new(),
                included_dirs: HashSet::new(),
                dirs_without_config: HashSet::new(),
            }),
        }
    }

    pub fn config_for_paths<P: AsRef<std::path::Path>>(&self, paths: &[P]) -> Result<Arc<Config>> {
        let mut state = self.state.lock();
        let mut missing_config_dirs = vec![];
        let mut missing_config_files = vec![];
        for path in paths {
            let path = path.as_ref();
            if state.included_dirs.iter().any(|dir| path.starts_with(dir)) {
                continue;
            }
            if path
                .ancestors()
                .all(|a| state.dirs_without_config.contains(a))
            {
                continue;
            }
            for ancestor in path.ancestors().collect::<Vec<_>>().into_iter().rev() {
                let config_path = ancestor.join("ccelerate.toml");
                if !config_path.exists() {
                    state.dirs_without_config.insert(ancestor.to_owned());
                    continue;
                }
                missing_config_dirs.push(ancestor.to_owned());
                missing_config_files.push(config_path);
            }
        }
        if missing_config_files.is_empty() {
            return Ok(state.config.clone());
        }
        let mut config_files = missing_config_files;
        config_files.extend(state.config_files.iter().cloned());
        let new_config = Config::new_from_files(&config_files)?;
        *state = ConfigState {
            config: Arc::new(new_config),
            config_files,
            included_dirs: state.included_dirs.clone(),
            dirs_without_config: state.dirs_without_config.clone(),
        };
        Ok(state.config.clone())
    }
}

impl Config {
    fn new() -> Self {
        Self {
            eager_patterns: Vec::new(),
            local_header_patterns: Vec::new(),
            include_defines: Vec::new(),
            pure_c_header_patterns: Vec::new(),
            bad_global_symbols_patterns: Vec::new(),
        }
    }

    fn new_from_files<P: AsRef<Path>>(config_files: &[P]) -> Result<Self> {
        let mut config = Self::new();
        for path in config_files {
            let config_file = std::fs::read_to_string(path)?;
            let config_file: ConfigFile = toml::from_str(config_file.as_str())?;

            macro_rules! add_patterns {
                ($field:ident) => {
                    for pattern in config_file.$field.iter() {
                        config.$field.push(glob::Pattern::new(pattern)?);
                    }
                };
            }

            add_patterns!(eager_patterns);
            add_patterns!(local_header_patterns);
            add_patterns!(pure_c_header_patterns);
            add_patterns!(bad_global_symbols_patterns);

            config
                .include_defines
                .extend(config_file.include_defines.into_iter().map(BString::from));
        }

        Ok(config)
    }

    pub fn is_eager_path(&self, path: &Path) -> bool {
        self.eager_patterns
            .iter()
            .any(|pattern| pattern.matches_path(path))
    }

    pub fn is_local_header(&self, path: &Path) -> bool {
        self.local_header_patterns
            .iter()
            .any(|pattern| pattern.matches_path(path))
    }

    pub fn is_pure_c_header(&self, path: &Path) -> bool {
        self.pure_c_header_patterns
            .iter()
            .any(|pattern| pattern.matches_path(path))
    }

    pub fn has_bad_global_symbol(&self, path: &Path) -> bool {
        self.bad_global_symbols_patterns
            .iter()
            .any(|pattern| pattern.matches_path(path))
    }

    pub fn is_include_define(&self, name: &BStr) -> bool {
        self.include_defines.iter().any(|define| define == name)
    }
}
