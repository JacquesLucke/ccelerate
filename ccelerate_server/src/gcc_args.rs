use std::{
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
};

use anyhow::Result;
use anyhow::anyhow;
use bstr::{BString, ByteVec};
use smallvec::SmallVec;

use crate::{
    code_language::CodeLanguage,
    parse_gcc::{self, SourceFile},
};

pub struct BuildObjectFileInfo {
    pub source_path: PathBuf,
    pub source_language: CodeLanguage,
    pub object_path: PathBuf,
}

impl BuildObjectFileInfo {
    pub fn from_args<S: AsRef<OsStr>>(cwd: &Path, args: &[S]) -> Result<Self> {
        let args = parse_gcc::GCCArgs::parse(cwd, args)?;
        let [first_source] = args.sources.as_slice() else {
            return Err(anyhow!("There has to be exactly one source"));
        };
        let source_language = first_source.language()?;
        let Some(object_path) = args.primary_output else {
            return Err(anyhow!("There has to be an output"));
        };
        Ok(Self {
            source_path: first_source.path.clone(),
            source_language,
            object_path,
        })
    }
}

pub struct LinkFileInfo {
    pub sources: SmallVec<[SourceFile; 16]>,
    pub output: PathBuf,
}

impl LinkFileInfo {
    pub fn from_args<S: AsRef<OsStr>>(cwd: &Path, args: &[S]) -> Result<Self> {
        let args = parse_gcc::GCCArgs::parse(cwd, args)?;
        let Some(output) = args.primary_output else {
            return Err(anyhow!("There has to be an output"));
        };
        Ok(Self {
            sources: args.sources.into(),
            output,
        })
    }
}

pub struct BuildFilesInfo {
    pub sources: SmallVec<[SourceFile; 16]>,
    pub output: Option<PathBuf>,
}

impl BuildFilesInfo {
    pub fn from_args<S: AsRef<OsStr>>(cwd: &Path, args: &[S]) -> Result<Self> {
        let args = parse_gcc::GCCArgs::parse(cwd, args)?;
        Ok(Self {
            sources: args.sources.into(),
            output: args.primary_output,
        })
    }
}

pub fn is_build_object_file<S: AsRef<OsStr>>(args: &[S]) -> bool {
    let dummy_cwd = Path::new("");
    match parse_gcc::GCCArgs::parse(dummy_cwd, args) {
        Ok(args) => {
            args.sources.len() == 1 && args.primary_output.is_some() && args.stop_before_link
        }
        Err(_) => false,
    }
}

/// Takes arguments that would build one object file and changes it so that it instead
/// outputs the preprocessed code for the source file.
pub fn update_build_object_args_to_output_preprocessed_with_defines<S: AsRef<OsStr>>(
    build_object_args: &[S],
    cwd: &Path,
) -> Vec<OsString> {
    let mut args = parse_gcc::GCCArgs::parse(cwd, build_object_args).expect("should be valid");
    args.primary_output = None;
    args.stop_before_link = false;
    args.stop_after_preprocessing = true;
    args.preprocess_keep_defines = true;
    args.to_args()
}

pub fn update_build_object_args_to_just_output_preprocessed_from_stdin<S: AsRef<OsStr>>(
    args: &[S],
    cwd: &Path,
    source_language: CodeLanguage,
) -> Vec<OsString> {
    let mut args = parse_gcc::GCCArgs::parse(cwd, args).expect("should be valid");
    args.sources = vec![];
    args.primary_output = None;
    args.depfile_target_name = None;
    args.depfile_output_path = None;
    args.depfile_generate = false;
    args.stop_after_preprocessing = true;
    args.stop_before_link = false;
    args.stop_before_assemble = false;
    let mut args = args.to_args();
    args.push("-x".into());
    args.push(source_language.to_gcc_x_arg().into());
    args.push("-".into());
    args
}

pub fn update_to_build_object_from_stdin<S: AsRef<OsStr>>(
    args: &[S],
    cwd: &Path,
    output_path: &Path,
    language: CodeLanguage,
) -> Vec<OsString> {
    let mut args = parse_gcc::GCCArgs::parse(cwd, args).expect("should be valid");
    args.stop_before_link = true;
    args.stop_after_preprocessing = false;
    args.stop_before_assemble = false;
    args.primary_output = Some(output_path.to_owned());
    args.sources = vec![];
    let mut args = args.to_args();
    args.push("-x".into());
    args.push(language.to_gcc_x_arg().into());
    args.push("-".into());
    args
}

pub fn update_to_link_sources_as_group<S: AsRef<OsStr>>(
    args: &[S],
    sources: &[SourceFile],
    cwd: &Path,
) -> Vec<OsString> {
    let mut args = parse_gcc::GCCArgs::parse(cwd, args).expect("should be valid");
    args.sources = sources.to_vec();
    args.use_link_group = true;
    args.to_args()
}

pub fn add_translation_unit_unspecific_args_to_key<S: AsRef<OsStr>>(
    args: &[S],
    cwd: &Path,
    key: &mut BString,
) {
    let mut args = parse_gcc::GCCArgs::parse(cwd, args).expect("should be valid");
    args.sources.clear();
    args.primary_output = None;
    args.depfile_target_name = None;
    args.depfile_output_path = None;
    args.depfile_generate = false;
    for arg in args.to_args() {
        key.push_str(arg.as_encoded_bytes());
    }
}
