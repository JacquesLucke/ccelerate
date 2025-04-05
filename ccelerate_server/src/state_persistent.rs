use std::{path::Path, sync::Arc};

use crate::database::{self, FileRecord};
use anyhow::Result;
use parking_lot::Mutex;

pub struct PersistentState {
    pub conn: Arc<Mutex<rusqlite::Connection>>,
}

impl PersistentState {
    pub fn new(path: &Path) -> Result<Self> {
        Ok(Self {
            conn: Arc::new(Mutex::new(database::load_or_create_db(path)?)),
        })
    }

    pub fn store(&self, path: &Path, data: &FileRecord) -> Result<()> {
        database::store_file_record(&self.conn.lock(), path, data)
    }

    pub fn load(&self, path: &Path) -> Option<FileRecord> {
        database::load_file_record(&self.conn.lock(), path)
    }
}
