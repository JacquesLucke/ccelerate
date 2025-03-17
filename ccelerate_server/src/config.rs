use anyhow::Result;
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
