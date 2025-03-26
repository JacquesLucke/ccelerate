#![deny(clippy::unwrap_used)]

use std::{path::PathBuf, sync::Arc};

use parking_lot::Mutex;

use crate::{Cli, config::Config, parallel_pool::ParallelPool, task_periods::TaskPeriods};

pub struct State {
    pub address: String,
    pub conn: Arc<Mutex<rusqlite::Connection>>,
    pub task_periods: TaskPeriods,
    pub tasks_table_state: Arc<Mutex<ratatui::widgets::TableState>>,
    pub pool: ParallelPool,
    pub cli: Cli,
    pub data_dir: PathBuf,
    pub config: Arc<Mutex<Config>>,
}
