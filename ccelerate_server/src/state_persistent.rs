use std::{
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::Result;
use bstr::{BStr, BString};
use ccelerate_shared::WrappedBinary;
use chrono::Utc;
use parking_lot::Mutex;

use crate::path_utils;

pub struct PersistentState {
    pub conn: Arc<Mutex<rusqlite::Connection>>,
}

impl PersistentState {
    pub async fn new(path: &Path) -> Result<Self> {
        path_utils::ensure_directory_for_file(path).await?;
        let db_migrations = rusqlite_migration::Migrations::new(vec![rusqlite_migration::M::up(
            "
            CREATE TABLE ObjectFiles(
                path TEXT NOT NULL PRIMARY KEY,
                build TEXT NOT NULL,
                build_debug TEXT NOT NULL,
                local_code TEXT,
                local_code_debug TEXT,
                last_build TEXT NOT NULL
            );
            CREATE TABLE ArchiveFiles(
                path TEXT NOT NULL PRIMARY KEY,
                build TEXT NOT NULL,
                build_debug TEXT NOT NULL
            );
            ",
        )]);
        let mut conn = rusqlite::Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        db_migrations.to_latest(&mut conn)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    pub fn update_object_file(
        &self,
        object_path: &Path,
        binary: WrappedBinary,
        cwd: &Path,
        args: impl IntoIterator<Item = impl AsRef<OsStr>>,
    ) -> Result<()> {
        let data = CompileObjectRecord {
            binary,
            cwd: cwd.to_path_buf(),
            args: args.into_iter().map(|s| s.as_ref().to_owned()).collect(),
        };
        self.conn.lock().execute(
            "INSERT OR REPLACE INTO ObjectFiles (path, build, build_debug, local_code, local_code_debug, last_build) VALUES (?1, ?2, ?3, NULL, NULL, ?4)",
            rusqlite::params![
                object_path.to_string_lossy(),
                serde_json::to_string(&data.to_raw())?,
                serde_json::to_string_pretty(&data.to_debug())?,
                Utc::now().to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn update_object_file_local_code(
        &self,
        object_path: &Path,
        local_code_file: &Path,
        global_includes: impl IntoIterator<Item = impl AsRef<Path>>,
        include_defines: impl IntoIterator<Item = impl AsRef<BStr>>,
        bad_includes: impl IntoIterator<Item = impl AsRef<Path>>,
    ) -> Result<()> {
        let data = ObjectLocalCodeRecord {
            local_code_file: local_code_file.to_path_buf(),
            global_includes: global_includes
                .into_iter()
                .map(|s| s.as_ref().to_path_buf())
                .collect(),
            include_defines: include_defines
                .into_iter()
                .map(|s| s.as_ref().to_owned())
                .collect(),
            bad_includes: bad_includes
                .into_iter()
                .map(|s| s.as_ref().to_path_buf())
                .collect(),
        };
        self.conn.lock().execute(
            "UPDATE ObjectFiles SET local_code = ?1, local_code_debug = ?2 WHERE path = ?3",
            rusqlite::params![
                serde_json::to_string(&data.to_raw())?,
                serde_json::to_string_pretty(&data.to_debug())?,
                object_path.to_string_lossy(),
            ],
        )?;
        Ok(())
    }

    pub fn update_archive_file(
        &self,
        archive_path: &Path,
        binary: WrappedBinary,
        cwd: &Path,
        args: impl IntoIterator<Item = impl AsRef<OsStr>>,
    ) -> Result<()> {
        let data = CreateArchiveRecord {
            binary,
            cwd: cwd.to_path_buf(),
            args: args.into_iter().map(|s| s.as_ref().to_owned()).collect(),
        };
        self.conn.lock().execute(
            "INSERT OR REPLACE INTO ArchiveFiles (path, build, build_debug) VALUES (?1, ?2, ?3)",
            rusqlite::params![
                archive_path.to_string_lossy(),
                serde_json::to_string(&data.to_raw())?,
                serde_json::to_string_pretty(&data.to_debug())?,
            ],
        )?;
        Ok(())
    }

    pub fn get_object_file(&self, path: &Path) -> Option<Arc<ObjectData>> {
        self.conn
            .lock()
            .query_row(
                "SELECT build, local_code, last_build FROM ObjectFiles WHERE path = ?",
                rusqlite::params!(path.to_string_lossy()),
                |row| {
                    let build: String = row.get(0)?;
                    let local_code: Option<String> = row.get(1)?;
                    let build = serde_json::from_str::<CompileObjectRecordRaw>(&build)
                        .map_err(|_| rusqlite::Error::InvalidQuery)?;
                    let local_code = serde_json::from_str::<ObjectLocalCodeRecordRaw>(
                        &local_code.unwrap_or_default(),
                    )
                    .map_err(|_| rusqlite::Error::InvalidQuery)
                    .map(|c| ObjectLocalCodeRecord::from_raw(&c))?;
                    let last_build: String = row.get(2)?;
                    let last_build = chrono::DateTime::parse_from_rfc3339(&last_build)
                        .map_err(|_| rusqlite::Error::InvalidQuery)?;
                    Ok(Arc::new(ObjectData {
                        path: path.to_owned(),
                        create: CompileObjectRecord::from_raw(&build),
                        local_code,
                        last_build,
                    }))
                },
            )
            .ok()
    }

    pub fn get_archive_file(&self, path: &Path) -> Option<CreateArchiveRecord> {
        self.conn
            .lock()
            .query_row(
                "SELECT build FROM ArchiveFiles WHERE path = ?",
                rusqlite::params!(path.to_string_lossy()),
                |row| {
                    let build: String = row.get(0)?;
                    let build = serde_json::from_str::<CreateArchiveRecordRaw>(&build)
                        .map_err(|_| rusqlite::Error::InvalidQuery)?;
                    Ok(CreateArchiveRecord::from_raw(&build))
                },
            )
            .ok()
    }
}

#[derive(Debug, Clone)]
pub struct ObjectData {
    pub path: PathBuf,
    pub create: CompileObjectRecord,
    pub local_code: ObjectLocalCodeRecord,
    pub last_build: chrono::DateTime<chrono::FixedOffset>,
}

#[derive(Debug, Clone)]
pub struct CompileObjectRecord {
    pub binary: WrappedBinary,
    pub cwd: PathBuf,
    pub args: Vec<OsString>,
}
#[derive(serde::Serialize, serde::Deserialize)]
struct CompileObjectRecordRaw {
    binary: WrappedBinary,
    cwd: OsString,
    args: Vec<OsString>,
}
#[derive(serde::Serialize)]
struct CompileObjectRecordDebug {
    binary: WrappedBinary,
    cwd: String,
    args: Vec<String>,
}

impl CompileObjectRecord {
    fn from_raw(raw: &CompileObjectRecordRaw) -> Self {
        Self {
            binary: raw.binary,
            cwd: raw.cwd.clone().into(),
            args: raw.args.clone(),
        }
    }

    fn to_raw(&self) -> CompileObjectRecordRaw {
        CompileObjectRecordRaw {
            binary: self.binary,
            cwd: self.cwd.clone().into(),
            args: self.args.clone(),
        }
    }

    fn to_debug(&self) -> CompileObjectRecordDebug {
        CompileObjectRecordDebug {
            binary: self.binary,
            cwd: self.cwd.to_string_lossy().to_string(),
            args: self
                .args
                .iter()
                .map(|s| s.to_string_lossy().to_string())
                .collect(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ObjectLocalCodeRecord {
    pub local_code_file: PathBuf,
    pub global_includes: Vec<PathBuf>,
    pub include_defines: Vec<BString>,
    pub bad_includes: Vec<PathBuf>,
}
#[derive(serde::Serialize, serde::Deserialize)]
struct ObjectLocalCodeRecordRaw {
    local_code_file: OsString,
    global_includes: Vec<OsString>,
    include_defines: Vec<BString>,
    bad_includes: Vec<OsString>,
}
#[derive(serde::Serialize)]
struct ObjectLocalCodeRecordDebug {
    local_code_file: String,
    global_includes: Vec<String>,
    include_defines: Vec<String>,
    bad_includes: Vec<String>,
}

impl ObjectLocalCodeRecord {
    fn from_raw(raw: &ObjectLocalCodeRecordRaw) -> Self {
        Self {
            local_code_file: raw.local_code_file.clone().into(),
            global_includes: raw
                .global_includes
                .iter()
                .map(|s| s.clone().into())
                .collect(),
            include_defines: raw.include_defines.to_vec(),
            bad_includes: raw.bad_includes.iter().map(|s| s.clone().into()).collect(),
        }
    }

    fn to_raw(&self) -> ObjectLocalCodeRecordRaw {
        ObjectLocalCodeRecordRaw {
            local_code_file: self.local_code_file.clone().into(),
            global_includes: self
                .global_includes
                .iter()
                .map(|s| s.clone().into())
                .collect(),
            include_defines: self.include_defines.to_vec(),
            bad_includes: self.bad_includes.iter().map(|s| s.clone().into()).collect(),
        }
    }

    fn to_debug(&self) -> ObjectLocalCodeRecordDebug {
        ObjectLocalCodeRecordDebug {
            local_code_file: self.local_code_file.to_string_lossy().to_string(),
            global_includes: self
                .global_includes
                .iter()
                .map(|s| s.to_string_lossy().to_string())
                .collect(),
            include_defines: self.include_defines.iter().map(|s| s.to_string()).collect(),
            bad_includes: self
                .bad_includes
                .iter()
                .map(|s| s.to_string_lossy().to_string())
                .collect(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct CreateArchiveRecord {
    pub cwd: PathBuf,
    pub binary: WrappedBinary,
    pub args: Vec<OsString>,
}
#[derive(serde::Serialize, serde::Deserialize)]
struct CreateArchiveRecordRaw {
    cwd: OsString,
    binary: WrappedBinary,
    args: Vec<OsString>,
}
#[derive(serde::Serialize)]
struct CreateArchiveRecordDebug {
    cwd: String,
    binary: WrappedBinary,
    args: Vec<String>,
}

impl CreateArchiveRecord {
    fn from_raw(raw: &CreateArchiveRecordRaw) -> Self {
        Self {
            cwd: raw.cwd.clone().into(),
            binary: raw.binary,
            args: raw.args.clone(),
        }
    }

    fn to_raw(&self) -> CreateArchiveRecordRaw {
        CreateArchiveRecordRaw {
            cwd: self.cwd.clone().into(),
            binary: self.binary,
            args: self.args.clone(),
        }
    }

    fn to_debug(&self) -> CreateArchiveRecordDebug {
        CreateArchiveRecordDebug {
            cwd: self.cwd.to_string_lossy().to_string(),
            binary: self.binary,
            args: self
                .args
                .iter()
                .map(|s| s.to_string_lossy().to_string())
                .collect(),
        }
    }
}
