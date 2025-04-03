use ccelerate_shared::RunRequestData;
use uuid::Uuid;

#[derive(Debug)]
pub struct LogEvent {
    pub info: LogEventInfo,
    pub time: std::time::Instant,
    pub parent: Option<LogScopeId>,
}

#[derive(Debug)]
pub enum LogEventInfo {
    RunRequestStart {
        id: LogScopeId,
        request: RunRequestData,
    },
    RunRequestEnd {
        id: LogScopeId,
        success: bool,
    },
}

#[derive(Debug, Clone)]
pub struct LogScopeId {
    id: Uuid,
}

impl LogScopeId {
    pub fn new() -> Self {
        Self { id: Uuid::new_v4() }
    }
}

pub fn log(info: LogEventInfo, parent: Option<LogScopeId>) {
    let event = LogEvent {
        info,
        time: std::time::Instant::now(),
        parent,
    };
    log::info!("{:?}", event);
}
