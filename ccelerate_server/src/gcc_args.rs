use std::{
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
};

use anyhow::Result;
use anyhow::anyhow;
use bstr::{BString, ByteVec};
use os_str_bytes::OsStrBytesExt;
use smallvec::{SmallVec, smallvec};

use crate::args_processing::{BuildObjectFileInfo, LinkFileInfo};
use crate::{code_language::CodeLanguage, path_utils::make_absolute, source_file::SourceFile};

impl BuildObjectFileInfo {
    pub fn from_gcc_args(cwd: &Path, args: &[impl AsRef<OsStr>]) -> Result<Self> {
        let args = GccArgsInfo::from_args(args)?;
        let Some(output) = args.get_single_output() else {
            return Err(anyhow!("There has to be one output"));
        };
        if output.extension() != Some(OsStr::new("o")) {
            return Err(anyhow!("Output must have .o extension"));
        }
        let sources = match args.get_sources() {
            Ok(sources) => sources,
            Err(e) => {
                return Err(anyhow!("Error parsing sources: {}", e));
            }
        };
        let [source] = sources.as_slice() else {
            return Err(anyhow!("There has to be exactly one source"));
        };
        let source_language = match source.language {
            Some(language) => language,
            None => CodeLanguage::from_path(source.path)?,
        };
        Ok(Self {
            source_path: make_absolute(cwd, source.path),
            source_language,
            object_path: make_absolute(cwd, Path::new(output)),
        })
    }
}

impl LinkFileInfo {
    pub fn from_gcc_args(cwd: &Path, args: &[impl AsRef<OsStr>]) -> Result<Self> {
        let args = GccArgsInfo::from_args(args)?;
        Ok(Self {
            sources: args.get_absolute_sources(cwd)?,
            output: args.get_absolute_single_output(cwd)?,
        })
    }
}

pub struct BuildFilesInfo {
    pub sources: SmallVec<[SourceFile; 16]>,
    pub output: Option<PathBuf>,
}

impl BuildFilesInfo {
    pub fn from_args(cwd: &Path, args: &[impl AsRef<OsStr>]) -> Result<Self> {
        let args = GccArgsInfo::from_args(args)?;
        Ok(Self {
            sources: args.get_absolute_sources(cwd)?,
            output: args.get_absolute_single_output(cwd).ok(),
        })
    }
}

pub fn is_build_object_file(args: &[impl AsRef<OsStr>]) -> Result<bool> {
    let args = GccArgsInfo::from_args(args)?;
    Ok(args.has_single_arg_str("-c"))
}

/// Takes arguments that would build one object file and changes it so that it instead
/// outputs the preprocessed code for the source file.
pub fn rewrite_to_extract_local_code(args: &[impl AsRef<OsStr>]) -> Result<Vec<OsString>> {
    let mut args = GccArgsInfo::from_args(args)?;
    args.args.retain(|arg| match arg {
        GccArg::Single(arg) => {
            if *arg == "-c" {
                // Remove -c, it is replaced by -E below to stop after preprocessing.
                false
            } else {
                true
            }
        }
        GccArg::Dual(first, _) => {
            if *first == "-o" {
                // Remove output file so that output is written to stdout.
                false
            } else {
                true
            }
        }
        _ => true,
    });
    // Stop after preprocessing.
    args.push_single_arg_str("-E");
    // Keep defines in preprocessed output.
    args.push_single_arg_str("-dD");
    Ok(args.to_args_owned_vec())
}

pub fn update_build_object_args_to_just_output_preprocessed_from_stdin(
    args: &[impl AsRef<OsStr>],
    source_language: CodeLanguage,
) -> Result<Vec<OsString>> {
    let mut args = GccArgsInfo::from_args(args)?;
    args.args.retain(|arg| match arg {
        GccArg::Single(arg) => {
            if *arg == "-c" {
                // Remove -c, it is replaced by -E below to stop after preprocessing.
                false
            } else if *arg == "-MD" {
                // Disable depsfile generation.
                false
            } else {
                true
            }
        }
        GccArg::Dual(first, _) => {
            if *first == "-o" {
                // Remove output file so that output is written to stdout.
                false
            } else if *first == "-MT" || *first == "-MF" {
                // Remove some depsfile generation arguments.
                false
            } else {
                true
            }
        }
        // Remove all sources, stdin is used instead.
        GccArg::Source(_) => false,
    });
    // Stop after preprocessing.
    args.push_single_arg_str("-E");
    // Set language for code going in stdin.
    args.push_dual_arg_str("-x", source_language.to_gcc_x_arg());
    // Tell gcc that code passed into stdin. This has to be the last argument.
    args.push_single_arg_str("-");
    Ok(args.to_args_owned_vec())
}

pub fn update_to_build_object_from_stdin(
    args: &[impl AsRef<OsStr>],
    output_path: &Path,
    language: CodeLanguage,
) -> Result<Vec<OsString>> {
    let mut args = GccArgsInfo::from_args(args)?;
    args.args.retain(|arg| match arg {
        GccArg::Single(_) => true,
        GccArg::Dual(first, _) => {
            if *first == "-o" {
                // Remove output file because it's replaced below.
                false
            } else {
                true
            }
        }
        // Remove all sources, stdin is used instead.
        GccArg::Source(_) => false,
    });
    // Set output file.
    args.push_dual_arg(OsStr::new("-o"), output_path.as_os_str());
    // Set language for code going in stdin.
    args.push_dual_arg_str("-x", language.to_gcc_x_arg());
    // Tell gcc that code passed into stdin. This has to be the last argument.
    args.push_single_arg_str("-");
    Ok(args.to_args_owned_vec())
}

pub fn update_to_link_sources_as_group(
    args: &[impl AsRef<OsStr>],
    sources: &[SourceFile],
) -> Result<Vec<OsString>> {
    let mut args = GccArgsInfo::from_args(args)?;
    args.args.retain(|arg| match arg {
        GccArg::Single(_) => true,
        GccArg::Dual(_, _) => true,
        // Remove all sources, they are added again below.
        GccArg::Source(_) => false,
    });

    // Add all sources to a link group so that their order does not matter.
    args.push_single_arg_str("-Wl,--start-group");
    for source in sources {
        match source.language() {
            Ok(language) => {
                args.push_dual_arg_str("-x", language.to_gcc_x_arg());
            }
            Err(_) => {
                args.push_single_arg_str("-x");
                args.push_single_arg_str("none");
            }
        }
        args.push_source_arg(&source.path);
    }
    args.push_single_arg_str("-Wl,--end-group");

    Ok(args.to_args_owned_vec())
}

pub fn add_translation_unit_unspecific_args_to_key(
    args: &[impl AsRef<OsStr>],
    key: &mut BString,
) -> Result<()> {
    let args = GccArgsInfo::from_args(args)?;
    for arg in args.args.iter() {
        match arg {
            GccArg::Single(arg) => {
                key.push_str(arg.as_encoded_bytes());
            }
            GccArg::Dual(first, second) => {
                if *first == "-o" {
                    // Don't add output file.
                    continue;
                }
                if *first == "-MT" || *first == "-MF" {
                    // Don't add depsfile generation arguments.
                    continue;
                }
                key.push_str(first.as_encoded_bytes());
                key.push_str(second.as_encoded_bytes());
            }
            // Don't add source file.
            GccArg::Source(_) => {}
        }
    }
    Ok(())
}

enum GccArg<'a> {
    Single(&'a OsStr),
    Dual(&'a OsStr, &'a OsStr),
    Source(&'a OsStr),
}

struct GccArgsInfo<'a> {
    args: SmallVec<[GccArg<'a>; 32]>,
}

struct SourceArgWithLanguage<'a> {
    path: &'a Path,
    language: Option<CodeLanguage>,
}

impl<'a> GccArgsInfo<'a> {
    fn from_args<S: AsRef<OsStr> + 'a>(args: &'a [S]) -> Result<GccArgsInfo<'a>> {
        let mut result = Self {
            args: SmallVec::with_capacity(args.len()),
        };
        let mut args_iter = args.iter();
        while let Some(arg) = args_iter.next() {
            let arg = arg.as_ref();
            if arg == "-isystem"
                || arg == "-include"
                || arg == "-o"
                || arg == "-MF"
                || arg == "-MT"
                || arg == "-x"
            {
                let next = args_iter
                    .next()
                    .ok_or_else(|| anyhow!("argument after {:?} is missing", arg))?
                    .as_ref();
                result.args.push(GccArg::Dual(arg, next));
            } else if arg.starts_with("-") {
                result.args.push(GccArg::Single(arg));
            } else {
                result.args.push(GccArg::Source(arg));
            }
        }
        Ok(result)
    }

    fn to_args(&self) -> SmallVec<[&'a OsStr; 32]> {
        let mut result = smallvec![];
        for arg in &self.args {
            match arg {
                GccArg::Single(arg) => result.push(*arg),
                GccArg::Dual(arg1, arg2) => {
                    result.push(*arg1);
                    result.push(*arg2);
                }
                GccArg::Source(arg) => result.push(*arg),
            }
        }
        result
    }

    fn push_single_arg_str(&mut self, arg: &'a str) {
        self.args.push(GccArg::Single(OsStr::new(arg)));
    }

    fn push_dual_arg_str(&mut self, first: &'a str, second: &'a str) {
        self.args
            .push(GccArg::Dual(OsStr::new(first), OsStr::new(second)));
    }

    fn push_dual_arg(&mut self, first: &'a OsStr, second: &'a OsStr) {
        self.args.push(GccArg::Dual(first, second));
    }

    fn push_source_arg(&mut self, path: &'a Path) {
        self.args.push(GccArg::Source(path.as_os_str()));
    }

    fn to_args_owned_vec(&self) -> Vec<OsString> {
        self.to_args().iter().map(|s| (*s).to_owned()).collect()
    }

    fn get_single_output(&self) -> Option<&'a Path> {
        for arg in &self.args {
            match arg {
                GccArg::Dual(first, path) if *first == "-o" => {
                    return Some(Path::new(*path));
                }
                _ => {}
            }
        }
        None
    }

    fn get_sources(&self) -> Result<SmallVec<[SourceArgWithLanguage<'a>; 16]>> {
        let mut sources = smallvec![];
        let mut current_language = None;
        for arg in &self.args {
            match arg {
                GccArg::Source(path) => {
                    sources.push(SourceArgWithLanguage {
                        path: Path::new(*path),
                        language: current_language,
                    });
                }
                GccArg::Dual(first, lang) if *first == "-x" => {
                    current_language = CodeLanguage::from_gcc_x_arg(&lang.to_string_lossy())?;
                }
                _ => {}
            }
        }
        Ok(sources)
    }

    fn get_absolute_sources(&self, cwd: &Path) -> Result<SmallVec<[SourceFile; 16]>> {
        let sources = self.get_sources()?;
        Ok(sources
            .iter()
            .map(|s| SourceFile {
                path: make_absolute(cwd, s.path),
                language_override: s.language,
            })
            .collect())
    }

    fn get_absolute_single_output(&self, cwd: &Path) -> Result<PathBuf> {
        let Some(output) = self.get_single_output() else {
            return Err(anyhow!("There has to be one output"));
        };
        Ok(make_absolute(cwd, output))
    }

    fn has_single_arg(&self, query: &OsStr) -> bool {
        self.args.iter().any(|arg| match arg {
            GccArg::Single(arg) => *arg == query,
            _ => false,
        })
    }

    fn has_single_arg_str(&self, query: &str) -> bool {
        self.has_single_arg(OsStr::new(query))
    }
}
