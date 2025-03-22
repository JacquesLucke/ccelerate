use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use parking_lot::Mutex;

pub struct TaskPeriods {
    tasks: Arc<Mutex<Vec<TaskPeriodStorage>>>,
}

struct TaskPeriodStorage {
    name: String,
    start_time: Instant,
    end_time: Arc<Mutex<Option<Instant>>>,
}

pub struct TaskPeriod {
    pub name: String,
    pub duration: Duration,
    pub active: bool,
}

pub struct TaskPeriodScope {
    end_time: Arc<Mutex<Option<Instant>>>,
}

impl TaskPeriods {
    pub fn new() -> Self {
        Self {
            tasks: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn start(&self, name: &str) -> TaskPeriodScope {
        let end_time = Arc::new(Mutex::new(None));
        let task = TaskPeriodStorage {
            name: name.to_string(),
            start_time: Instant::now(),
            end_time: end_time.clone(),
        };
        self.tasks.lock().push(task);
        TaskPeriodScope { end_time }
    }

    pub fn get_periods(&self) -> Vec<TaskPeriod> {
        self.tasks
            .lock()
            .iter()
            .map(|t| TaskPeriod {
                name: t.name.clone(),
                duration: t.duration(),
                active: t.is_running(),
            })
            .collect()
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

impl Drop for TaskPeriodScope {
    fn drop(&mut self) {
        *self.end_time.lock() = Some(Instant::now());
    }
}
