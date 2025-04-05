#![deny(clippy::unwrap_used)]

use std::path::{Path, PathBuf};

pub fn make_absolute(base: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        return path.to_path_buf();
    }
    base.join(path)
}

pub fn shorten_path(path: &Path) -> String {
    if let Some(path_name) = path.file_name() {
        path_name.to_string_lossy().to_string()
    } else {
        path.to_string_lossy().to_string()
    }
}

pub async fn ensure_directory_for_file(file_path: &Path) -> Result<(), std::io::Error> {
    if let Some(parent) = file_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    Ok(())
}

pub async fn ensure_directory_and_write(path: &Path, content: &[u8]) -> Result<(), std::io::Error> {
    ensure_directory_for_file(path).await?;
    tokio::fs::write(path, content).await?;
    Ok(())
}
