use std::path::PathBuf;

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct RunRequestData {
    pub binary: String,
    pub args: Vec<String>,
    pub cwd: PathBuf,
}
