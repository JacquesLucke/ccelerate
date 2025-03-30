use std::{
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
};

use anyhow::*;
use smallvec::SmallVec;

use crate::{code_language::CodeLanguage, parse_gcc::SourceFile};

pub struct BuildObjectFileInfo {
    pub source_path: PathBuf,
    pub source_language: CodeLanguage,
    pub object_path: PathBuf,
}

impl BuildObjectFileInfo {
    pub fn from_args<S: AsRef<OsStr>>(cwd: &Path, args: &[S]) -> Result<Self> {
        todo!();
    }
}

pub struct LinkFileInfo {
    pub sources: SmallVec<[SourceFile; 16]>,
    pub output: PathBuf,
}

impl LinkFileInfo {
    pub fn from_args<S: AsRef<OsStr>>(cwd: &Path, args: &[S]) -> Result<Self> {
        todo!();
    }
}

pub struct BuildFilesInfo {
    pub sources: SmallVec<[SourceFile; 16]>,
    pub output: Option<PathBuf>,
}

impl BuildFilesInfo {
    pub fn from_args<S: AsRef<OsStr>>(cwd: &Path, args: &[S]) -> Result<Self> {
        todo!();
    }
}

pub fn is_build_object_file<S: AsRef<OsStr>>(args: &[S]) -> bool {
    todo!();
}

/// Takes arguments that would build one object file and changes it so that it instead
/// outputs the preprocessed code for the source file.
pub fn update_build_object_args_to_output_preprocessed_with_defines<S: AsRef<OsStr>>(
    build_object_args: &[S],
) -> Vec<OsString> {
    todo!();
}

pub fn update_build_object_args_to_just_output_preprocessed_from_stdin<S: AsRef<OsStr>>(
    build_object_args: &[S],
) -> Vec<OsString> {
    todo!();
}

pub fn update_to_build_object_from_stdin<S: AsRef<OsStr>>(
    args: &[S],
    output_path: &Path,
) -> Vec<OsString> {
    todo!();
}

pub fn update_to_link_sources_as_group<S: AsRef<OsStr>>(
    args: &[S],
    sources: &[SourceFile],
) -> Vec<OsString> {
    todo!();
}

pub fn remove_translation_unit_specific_args<S: AsRef<OsStr>>(args: &[S]) -> Vec<OsString> {
    todo!();
}
