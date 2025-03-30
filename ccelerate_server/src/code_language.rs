#![deny(clippy::unwrap_used)]

use std::path::Path;

use anyhow::*;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum CodeLanguage {
    // C code.
    C,
    // C++ code.
    Cxx,
    // Preprocessed C code.
    I,
    // Preprocessed C++ code.
    II,
}

impl CodeLanguage {
    pub fn from_ext(ext: &str) -> Result<Self> {
        match ext {
            "c" => Ok(Self::C),
            "cc" | "cp" | "cpp" | "cxx" | "c++" => Ok(Self::Cxx),
            "i" => Ok(Self::I),
            "ii" => Ok(Self::II),
            _ => Err(anyhow!("Unknown language extension: {}", ext)),
        }
    }

    pub fn from_path(path: &Path) -> Result<Self> {
        Self::from_ext(path.extension().and_then(|e| e.to_str()).unwrap_or(""))
    }

    pub fn to_valid_ext(self) -> &'static str {
        match self {
            Self::C => "c",
            Self::Cxx => "cc",
            Self::I => "i",
            Self::II => "ii",
        }
    }

    pub fn from_gcc_x_arg(arg: &str) -> Result<Option<Self>> {
        match arg {
            "c" => Ok(Some(Self::C)),
            "c++" => Ok(Some(Self::Cxx)),
            "cpp-output" => Ok(Some(Self::I)),
            "c++-cpp-output" => Ok(Some(Self::II)),
            "none" => Ok(None),
            _ => Err(anyhow!("Unknown language {}", arg)),
        }
    }

    pub fn to_gcc_x_arg(self) -> &'static str {
        match self {
            Self::C => "c",
            Self::Cxx => "c++",
            Self::I => "cpp-output",
            Self::II => "c++-cpp-output",
        }
    }

    pub fn to_preprocessed(self) -> Result<CodeLanguage> {
        match self {
            Self::C => Ok(Self::I),
            Self::Cxx => Ok(Self::II),
            _ => Err(anyhow!("Cannot preprocess language {:?}", self)),
        }
    }
}
