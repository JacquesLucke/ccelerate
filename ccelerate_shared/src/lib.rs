use base64::prelude::*;
use std::{
    ffi::{OsStr, OsString},
    path::PathBuf,
};

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct RunRequestDataWire {
    pub binary: WrappedBinary,
    pub args: Vec<String>,
    pub cwd: String,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize, Default)]
pub struct RunResponseDataWire {
    pub stdout: String,
    pub stderr: String,
    pub status: i32,
}

pub const DEFAULT_PORT: u16 = 6235;

#[derive(Debug, Clone)]
pub struct RunRequestData {
    pub binary: WrappedBinary,
    pub args: Vec<OsString>,
    pub cwd: PathBuf,
}

#[derive(Debug, Clone)]
pub struct RunResponseData {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub status: i32,
}

#[derive(Debug, Clone, Copy, serde::Deserialize, serde::Serialize)]
pub enum WrappedBinary {
    Gcc,
    Gxx,
    Clang,
    Clangxx,
    Ar,
}

impl WrappedBinary {
    pub fn to_standard_binary_name(&self) -> OsString {
        match self {
            WrappedBinary::Gcc => "gcc".into(),
            WrappedBinary::Gxx => "g++".into(),
            WrappedBinary::Clang => "clang".into(),
            WrappedBinary::Clangxx => "clang++".into(),
            WrappedBinary::Ar => "ar".into(),
        }
    }

    pub fn from_standard_binary_name(binary_name: &OsStr) -> Option<Self> {
        match binary_name.to_str() {
            Some("gcc") => Some(WrappedBinary::Gcc),
            Some("g++") => Some(WrappedBinary::Gxx),
            Some("clang") => Some(WrappedBinary::Clang),
            Some("clang++") => Some(WrappedBinary::Clangxx),
            Some("ar") => Some(WrappedBinary::Ar),
            _ => None,
        }
    }

    pub fn is_gcc_compatible(&self) -> bool {
        match self {
            WrappedBinary::Gcc
            | WrappedBinary::Gxx
            | WrappedBinary::Clang
            | WrappedBinary::Clangxx => true,
            _ => false,
        }
    }

    pub fn is_ar_compatible(&self) -> bool {
        match self {
            WrappedBinary::Ar => true,
            _ => false,
        }
    }
}

impl RunRequestData {
    pub fn to_wire(self) -> RunRequestDataWire {
        RunRequestDataWire {
            binary: self.binary,
            cwd: encode_osstr(self.cwd.into_os_string()),
            args: self.args.into_iter().map(encode_osstr).collect(),
        }
    }

    pub fn from_wire(wire: &RunRequestDataWire) -> Result<Self, base64::DecodeError> {
        Ok(Self {
            binary: wire.binary,
            cwd: decode_osstr(&wire.cwd)?.into(),
            args: wire
                .args
                .iter()
                .map(|s| decode_osstr(s))
                .collect::<Result<_, _>>()?,
        })
    }
}

impl RunResponseData {
    pub fn to_wire(self) -> RunResponseDataWire {
        RunResponseDataWire {
            stdout: BASE64_STANDARD.encode(&self.stdout),
            stderr: BASE64_STANDARD.encode(&self.stderr),
            status: self.status,
        }
    }

    pub fn from_wire(wire: RunResponseDataWire) -> Result<Self, base64::DecodeError> {
        Ok(Self {
            stdout: BASE64_STANDARD.decode(wire.stdout)?,
            stderr: BASE64_STANDARD.decode(wire.stderr)?,
            status: wire.status,
        })
    }
}

fn encode_osstr(s: OsString) -> String {
    BASE64_STANDARD.encode(s.as_encoded_bytes())
}

fn decode_osstr(s: &str) -> Result<OsString, base64::DecodeError> {
    // SAFETY: It is expected that the string had been encoded on the same system.
    Ok(unsafe { OsString::from_encoded_bytes_unchecked(BASE64_STANDARD.decode(s)?) })
}
