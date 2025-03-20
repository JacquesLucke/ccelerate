use anyhow::Result;
use bstr::BStr;
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

use serde::Deserialize;

#[derive(Debug, Default)]
pub struct Config {
    folder_configs: Vec<FolderConfig>,
    scanned_folders: HashSet<PathBuf>,
}

#[derive(Debug)]
struct FolderConfig {
    dir: PathBuf,
    config: ConfigFile,
}

#[derive(Deserialize, Debug)]
struct ConfigFile {
    eager_patterns: Vec<String>,
    local_header_patterns: Vec<String>,
    include_defines: Vec<String>,
    pure_c_header_patterns: Vec<String>,
}

impl Config {
    pub fn is_eager_path(&self, path: &Path) -> bool {
        for folder_config in self.folder_configs.iter() {
            if !path.starts_with(&folder_config.dir) {
                continue;
            }
            for pattern in folder_config.config.eager_patterns.iter() {
                if path.to_string_lossy().contains(pattern) {
                    return true;
                }
            }
        }
        false
    }

    pub fn is_local_header(&self, path: &Path) -> bool {
        if matches!(path.extension(), Some(ext) if ext == "cc" || ext == "c") {
            return true;
        }
        if path.as_os_str().to_string_lossy().contains("shaders/infos") {
            return true;
        }
        for folder_config in self.folder_configs.iter() {
            for pattern in folder_config.config.local_header_patterns.iter() {
                if path.ends_with(pattern) {
                    return true;
                }
            }
        }
        false
    }

    pub fn is_include_define(&self, name: &BStr) -> bool {
        for folder_config in self.folder_configs.iter() {
            for pattern in folder_config.config.include_defines.iter() {
                if name == pattern {
                    return true;
                }
            }
        }
        false
    }

    pub fn is_pure_c_header(&self, path: &Path) -> bool {
        for folder_config in self.folder_configs.iter() {
            for pattern in folder_config.config.pure_c_header_patterns.iter() {
                if path.to_string_lossy().contains(pattern) {
                    return true;
                }
            }
        }
        false
    }

    pub fn ensure_configs(&mut self, path: &Path) -> Result<()> {
        let ancestors = path.ancestors().collect::<Vec<_>>();
        for ancestor in ancestors.into_iter().rev() {
            if self.scanned_folders.contains(ancestor) {
                return Ok(());
            }
            self.scanned_folders.insert(ancestor.to_owned());
            let config_path = ancestor.join("ccelerate.toml");
            if !config_path.exists() {
                continue;
            }
            self.load_config_file(&config_path)?;
            break;
        }
        Ok(())
    }

    fn load_config_file(&mut self, path: &Path) -> Result<()> {
        let config: ConfigFile = toml::from_str(std::fs::read_to_string(path)?.as_str())?;
        log::info!("Loaded: {:#?}", config);
        let folder_config = FolderConfig {
            dir: path
                .canonicalize()?
                .parent()
                .ok_or_else(|| anyhow::anyhow!("Could not find parent"))?
                .to_owned(),
            config,
        };
        self.folder_configs.push(folder_config);
        Ok(())
    }
}
