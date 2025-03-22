use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

use anyhow::Result;
use bstr::BString;
use ccelerate_shared::WrappedBinary;

#[derive(Debug, Clone)]
pub struct FileRecord {
    pub cwd: PathBuf,
    pub binary: WrappedBinary,
    pub args: Vec<OsString>,
    pub local_code_file: Option<PathBuf>,
    pub global_includes: Option<Vec<PathBuf>>,
    pub include_defines: Option<Vec<BString>>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct FileRecordStorage {
    cwd: OsString,
    binary: WrappedBinary,
    args: Vec<OsString>,
    local_code_file: Option<OsString>,
    global_includes: Option<Vec<OsString>>,
    include_defines: Option<Vec<BString>>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct FileRecordDebug {
    cwd: String,
    binary: WrappedBinary,
    args: Vec<String>,
    local_code_file: Option<String>,
    global_includes: Option<Vec<String>>,
    include_defines: Option<Vec<String>>,
}

impl FileRecordStorage {
    fn from_record(data: &FileRecord) -> Self {
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

    fn to_record(&self) -> FileRecord {
        FileRecord {
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

impl FileRecordDebug {
    fn from_record(data: &FileRecord) -> Self {
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

pub fn store_file_record(
    conn: &rusqlite::Connection,
    path: &Path,
    data: &FileRecord,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO Files (path, data_debug, data) VALUES (?1, ?2, ?3)",
        rusqlite::params![
            path.to_string_lossy(),
            serde_json::to_string_pretty(&FileRecordDebug::from_record(data)).unwrap(),
            serde_json::to_string(&FileRecordStorage::from_record(data)).unwrap(),
        ],
    )?;
    Ok(())
}

pub fn load_file_record(conn: &rusqlite::Connection, path: &Path) -> Option<FileRecord> {
    conn.query_row(
        "SELECT data FROM Files WHERE path = ?",
        rusqlite::params![path.to_string_lossy().to_string()],
        |row| {
            let data = row.get::<usize, String>(0).unwrap();
            Ok(serde_json::from_str::<FileRecordStorage>(&data)
                .unwrap()
                .to_record())
        },
    )
    .ok()
}

pub fn load_or_create_db(path: &Path) -> Result<rusqlite::Connection> {
    let db_migrations = rusqlite_migration::Migrations::new(vec![rusqlite_migration::M::up(
        "
        CREATE TABLE Files(
            path TEXT NOT NULL PRIMARY KEY,
            data TEXT NOT NULL,
            data_debug TEXT NOT NULL
        );
        CREATE TABLE LogFiles(
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            path TEXT NOT NULL,
            time TEXT NOT NULL
        );
        ",
    )]);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut conn = rusqlite::Connection::open(path)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    db_migrations.to_latest(&mut conn)?;
    Ok(conn)
}
