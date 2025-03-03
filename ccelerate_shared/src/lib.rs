use std::path::PathBuf;

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct RunRequestData {
    pub binary: String,
    pub args: Vec<String>,
    pub cwd: PathBuf,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct RunResponseData {
    pub stdout: String,
    pub stderr: String,
    pub status: i32,
}

pub const DEFAULT_PORT: u16 = 6235;
