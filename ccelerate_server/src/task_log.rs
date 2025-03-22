use crate::{state::State, task_periods::TaskPeriodScope};

pub trait TaskInfo {
    fn short_name(&self) -> String;
    fn log(&self);
}

pub fn log_task(task: &dyn TaskInfo, state: &State) -> TaskPeriodScope {
    let task_period = state.task_periods.start(&task.short_name());
    task.log();
    task_period
}
