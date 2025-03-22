use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

use bstr::BString;
use ccelerate_shared::WrappedBinary;

#[derive(Debug)]
pub struct DbFilesRowData {
    pub cwd: PathBuf,
    pub binary: WrappedBinary,
    pub args: Vec<OsString>,
    pub local_code_file: Option<PathBuf>,
    pub global_includes: Option<Vec<PathBuf>>,
    pub include_defines: Option<Vec<BString>>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct DbFilesRowDataStorage {
    cwd: OsString,
    binary: WrappedBinary,
    args: Vec<OsString>,
    local_code_file: Option<OsString>,
    global_includes: Option<Vec<OsString>>,
    include_defines: Option<Vec<BString>>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct DbFilesRowDataDebug {
    cwd: String,
    binary: WrappedBinary,
    args: Vec<String>,
    local_code_file: Option<String>,
    global_includes: Option<Vec<String>>,
    include_defines: Option<Vec<String>>,
}

impl DbFilesRowDataStorage {
    fn from_data(data: &DbFilesRowData) -> Self {
        Self {
            cwd: data.cwd.clone().into(),
            binary: data.binary,
            args: data.args.clone(),
            local_code_file: data.local_code_file.clone().map(|s| s.into()),
            global_includes: data
                .global_includes
                .clone()
                .map(|h| h.iter().map(|s| s.clone().into()).collect()),
            include_defines: data.include_defines.clone(),
        }
    }

    fn to_data(&self) -> DbFilesRowData {
        DbFilesRowData {
            cwd: self.cwd.clone().into(),
            binary: self.binary,
            args: self.args.clone(),
            local_code_file: self.local_code_file.clone().map(|s| s.into()),
            global_includes: self
                .global_includes
                .clone()
                .map(|h| h.iter().map(|s| s.clone().into()).collect()),
            include_defines: self.include_defines.clone(),
        }
    }
}

impl DbFilesRowDataDebug {
    fn from_data(data: &DbFilesRowData) -> Self {
        Self {
            cwd: data.cwd.to_string_lossy().to_string(),
            binary: data.binary,
            args: data
                .args
                .iter()
                .map(|s| s.to_string_lossy().to_string())
                .collect(),
            local_code_file: data
                .local_code_file
                .as_ref()
                .map(|s| s.to_string_lossy().to_string()),
            global_includes: data
                .global_includes
                .as_ref()
                .map(|h| h.iter().map(|s| s.to_string_lossy().to_string()).collect()),
            include_defines: data
                .include_defines
                .as_ref()
                .map(|h| h.iter().map(|s| s.to_string()).collect()),
        }
    }
}

pub fn store_db_file(
    conn: &rusqlite::Connection,
    path: &Path,
    data: &DbFilesRowData,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO Files (path, data_debug, data) VALUES (?1, ?2, ?3)",
        rusqlite::params![
            path.to_string_lossy(),
            serde_json::to_string_pretty(&DbFilesRowDataDebug::from_data(data)).unwrap(),
            serde_json::to_string(&DbFilesRowDataStorage::from_data(data)).unwrap(),
        ],
    )?;
    Ok(())
}

pub fn load_db_file(conn: &rusqlite::Connection, path: &Path) -> Option<DbFilesRowData> {
    conn.query_row(
        "SELECT data FROM Files WHERE path = ?",
        rusqlite::params![path.to_string_lossy().to_string()],
        |row| {
            // TODO: Support OsStr in the database.
            let data = row.get::<usize, String>(0).unwrap();
            Ok(serde_json::from_str::<DbFilesRowDataStorage>(&data)
                .unwrap()
                .to_data())
        },
    )
    .ok()
}
