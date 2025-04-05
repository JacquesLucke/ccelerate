#![deny(clippy::unwrap_used)]

use std::{path::PathBuf, sync::Arc};

use parking_lot::Mutex;

use crate::{
    Cli, config::ConfigManager, parallel_pool::ParallelPool, state_persistent::PersistentState,
    task_periods::TaskPeriods,
};

pub struct State {
    pub address: String,
    pub persistent: PersistentState,
    pub task_periods: TaskPeriods,
    pub tasks_table_state: Arc<Mutex<ratatui::widgets::TableState>>,
    pub auto_scroll: Arc<Mutex<bool>>,
    pub pool: ParallelPool,
    pub cli: Cli,
    pub data_dir: PathBuf,
    pub config_manager: ConfigManager,
}
