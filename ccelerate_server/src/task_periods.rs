#![deny(clippy::unwrap_used)]

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use parking_lot::Mutex;
use serde::Serialize;

pub struct TaskPeriods {
    tasks: Arc<Mutex<TaskPeriodsVec>>,
}

struct TaskPeriodsVec {
    tasks: Vec<TaskPeriodStorage>,
    final_sorted_num: usize,
}

struct TaskPeriodStorage {
    category: String,
    name: String,
    start_time: Instant,
    end_time: Arc<Mutex<Option<Instant>>>,
    finished_successfully: Arc<Mutex<bool>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskPeriod {
    pub category: String,
    pub name: String,
    pub duration: Duration,
    pub active: bool,
    pub finished_successfully: bool,
}

pub struct TaskPeriodScope {
    end_time: Arc<Mutex<Option<Instant>>>,
    finished_successfully: Arc<Mutex<bool>>,
}

impl TaskPeriods {
    pub fn new() -> Self {
        Self {
            tasks: Arc::new(Mutex::new(TaskPeriodsVec {
                tasks: vec![],
                final_sorted_num: 0,
            })),
        }
    }

    pub fn start(&self, category: &str, name: &str) -> TaskPeriodScope {
        let end_time = Arc::new(Mutex::new(None));
        let finished_successfully = Arc::new(Mutex::new(false));
        let task = TaskPeriodStorage {
            category: category.to_string(),
            name: name.to_string(),
            start_time: Instant::now(),
            end_time: end_time.clone(),
            finished_successfully: finished_successfully.clone(),
        };
        self.tasks.lock().tasks.push(task);
        TaskPeriodScope {
            end_time,
            finished_successfully,
        }
    }

    pub fn get_sorted_periods(&self) -> Vec<TaskPeriod> {
        let mut tasks = self.tasks.lock();
        let fixed_num = tasks.final_sorted_num;
        let tasks_to_sort = &mut tasks.tasks[fixed_num..];
        tasks_to_sort.sort_by_cached_key(|t| {
            let is_running = t.is_running();
            let duration = t.duration();
            (
                is_running,
                if is_running { duration } else { Duration::ZERO },
            )
        });
        tasks.final_sorted_num += tasks_to_sort
            .iter()
            .position(|t| t.is_running())
            .unwrap_or(0);
        tasks
            .tasks
            .iter()
            .map(|t| TaskPeriod {
                category: t.category.clone(),
                name: t.name.clone(),
                duration: t.duration(),
                active: t.is_running(),
                finished_successfully: *t.finished_successfully.lock(),
            })
            .collect()
    }

    pub fn tasks_num(&self) -> usize {
        self.tasks.lock().tasks.len()
    }
}

impl TaskPeriodStorage {
    fn is_running(&self) -> bool {
        self.end_time.lock().is_none()
    }

    fn duration(&self) -> Duration {
        self.end_time
            .lock()
            .unwrap_or_else(Instant::now)
            .duration_since(self.start_time)
    }
}

impl TaskPeriodScope {
    pub fn finished_successfully(&self) {
        *self.finished_successfully.lock() = true;
    }
}

impl Drop for TaskPeriodScope {
    fn drop(&mut self) {
        *self.end_time.lock() = Some(Instant::now());
    }
}
