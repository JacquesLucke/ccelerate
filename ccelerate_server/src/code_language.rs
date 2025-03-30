#![deny(clippy::unwrap_used)]

use anyhow::*;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Language {
    // C code.
    C,
    // C++ code.
    Cxx,
    // Preprocessed C code.
    I,
    // Preprocessed C++ code.
    II,
}

impl Language {
    pub fn from_ext(ext: &str) -> Result<Self> {
        match ext {
            "c" => Ok(Self::C),
            "cc" | "cp" | "cpp" | "cxx" | "c++" => Ok(Self::Cxx),
            "i" => Ok(Self::I),
            "ii" => Ok(Self::II),
            _ => Err(anyhow!("Unknown language extension: {}", ext)),
        }
    }

    pub fn to_valid_ext(self) -> &'static str {
        match self {
            Self::C => "c",
            Self::Cxx => "cc",
            Self::I => "i",
            Self::II => "ii",
        }
    }

    pub fn from_x_arg(arg: &str) -> Result<Option<Self>> {
        match arg {
            "c" => Ok(Some(Self::C)),
            "c++" => Ok(Some(Self::Cxx)),
            "cpp-output" => Ok(Some(Self::I)),
            "c++-cpp-output" => Ok(Some(Self::II)),
            "none" => Ok(None),
            _ => Err(anyhow!("Unknown language {}", arg)),
        }
    }

    pub fn to_x_arg(self) -> &'static str {
        match self {
            Self::C => "c",
            Self::Cxx => "c++",
            Self::I => "cpp-output",
            Self::II => "c++-cpp-output",
        }
    }

    pub fn to_preprocessed(self) -> Result<Language> {
        match self {
            Self::C => Ok(Self::I),
            Self::Cxx => Ok(Self::II),
            _ => Err(anyhow!("Cannot preprocess language {:?}", self)),
        }
    }
}
