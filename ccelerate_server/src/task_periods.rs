use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use parking_lot::Mutex;

pub struct TasksLogger {
    tasks: Arc<Mutex<Vec<TaskLog>>>,
}

impl TasksLogger {
    pub fn new() -> Self {
        Self {
            tasks: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn start_task(&self, name: &str) -> TaskLogHandle {
        let end_time = Arc::new(Mutex::new(None));
        let task = TaskLog {
            name: name.to_string(),
            start_time: Instant::now(),
            end_time: end_time.clone(),
        };
        self.tasks.lock().push(task);
        TaskLogHandle { end_time }
    }

    pub fn get_for_print(&self) -> Vec<TaskLogPrint> {
        self.tasks
            .lock()
            .iter()
            .map(|t| TaskLogPrint {
                name: t.name.clone(),
                duration: t.duration(),
                active: t.is_running(),
            })
            .collect()
    }
}

struct TaskLog {
    name: String,
    start_time: Instant,
    end_time: Arc<Mutex<Option<Instant>>>,
}

pub struct TaskLogPrint {
    pub name: String,
    pub duration: Duration,
    pub active: bool,
}

impl TaskLog {
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

pub struct TaskLogHandle {
    end_time: Arc<Mutex<Option<Instant>>>,
}

impl Drop for TaskLogHandle {
    fn drop(&mut self) {
        *self.end_time.lock() = Some(Instant::now());
    }
}
