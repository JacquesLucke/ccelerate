use std::path::PathBuf;

use crate::code_language::CodeLanguage;
use anyhow::Result;

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct SourceFile {
    pub path: PathBuf,
    pub language_override: Option<CodeLanguage>,
}

impl SourceFile {
    pub fn language(&self) -> Result<CodeLanguage> {
        if let Some(language) = self.language_override {
            return Ok(language);
        }
        let ext = self
            .path
            .extension()
            .and_then(|e| e.to_str())
            .ok_or_else(|| anyhow::anyhow!("Failed to get extension"))?;
        CodeLanguage::from_ext(ext)
    }
}
