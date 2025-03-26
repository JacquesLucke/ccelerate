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
