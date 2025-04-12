#![deny(clippy::unwrap_used)]

use std::{
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::Result;
use bstr::{BStr, BString, ByteSlice};

use crate::{config::Config, path_utils::make_absolute};

#[derive(Debug, Default)]
pub struct LocalCode {
    // Preprocessed code of the source file without any of the headers.
    pub local_code: BString,
    // Global headers that are included in this file. Generally, all these headers
    // should have include guards and their include order should not matter.
    // This also includes standard library headers.
    pub global_includes: Vec<PathBuf>,
    // Sometimes, implementation files define values that affect headers that are typically global.
    // E.g. `#define DNA_DEPRECATED_ALLOW` in Blender.
    pub include_defines: Vec<BString>,
}

impl LocalCode {
    pub async fn from_preprocessed_code(
        code: &BStr,
        source_file_path: &Path,
        config: &Config,
    ) -> Result<LocalCode> {
        let Some(source_dir) = source_file_path.parent() else {
            return Err(anyhow::anyhow!(
                "Failed to get directory of source file path"
            ));
        };

        let mut result = LocalCode::default();

        writeln!(result.local_code, "#pragma GCC diagnostic push")?;

        let mut header_stack: Vec<&Path> = Vec::new();
        let mut local_depth = 0;

        let mut revertable_previous_line_start = None;
        let write_line_markers = true;

        for line in code.split(|&b| b == b'\n') {
            let is_local = header_stack.len() == local_depth;
            let line = line.as_bstr();
            if line.starts_with(b"#define ") {
                if is_local {
                    if let Ok(macro_def) = MacroDefinition::parse(line) {
                        if config.is_include_define(macro_def.name) {
                            result.include_defines.push(line.to_owned());
                        }
                    }
                }
            } else if let Some(_undef) = line.strip_prefix(b"#undef ") {
                continue;
            } else if line.starts_with(b"# ") {
                let Ok(line_marker) = GccLinemarker::parse(line) else {
                    continue;
                };
                let header_path = Path::new(line_marker.header_name);
                if line_marker.is_start_of_new_file {
                    if is_local {
                        if config.is_local_header(header_path) {
                            local_depth += 1;
                        } else {
                            result.global_includes.push(header_path.to_owned());
                        }
                    }
                    header_stack.push(header_path);
                } else if line_marker.is_return_to_file {
                    header_stack.pop();
                    local_depth = local_depth.min(header_stack.len());
                }
                if write_line_markers && header_stack.len() == local_depth {
                    if let Some(len) = revertable_previous_line_start {
                        // Remove the previously written line marker because it does not have a purpose
                        // oi the next line contains a line marker as well.
                        result.local_code.truncate(len);
                    }
                    let file_path = header_stack.last().unwrap_or(&source_file_path);
                    revertable_previous_line_start = Some(result.local_code.len());
                    writeln!(
                        result.local_code,
                        "# {} \"{}\"",
                        line_marker.line_number,
                        file_path.display()
                    )?;
                }
            } else if is_local {
                writeln!(result.local_code, "{}", line)?;
                if !line.trim_ascii().is_empty() {
                    revertable_previous_line_start = None;
                }
            }
        }
        writeln!(result.local_code, "#pragma GCC diagnostic pop")?;

        result
            .global_includes
            .iter_mut()
            .for_each(|p| *p = make_absolute(source_dir, p));

        Ok(result)
    }
}

#[derive(Debug, Clone)]
struct MacroDefinition<'a> {
    name: &'a BStr,
    _value: &'a BStr,
}

impl<'a> MacroDefinition<'a> {
    fn parse(line: &'a BStr) -> Result<Self> {
        static RE: once_cell::sync::Lazy<regex::bytes::Regex> = once_cell::sync::Lazy::new(|| {
            regex::bytes::Regex::new(r#"(?m)^#define\s+(\w+)(.*)$"#).expect("should be valid")
        });
        let Some(captures) = RE.captures(line) else {
            return Err(anyhow::anyhow!("Failed to parse line: {:?}", line));
        };
        let name = captures
            .get(1)
            .expect("group should exist")
            .as_bytes()
            .as_bstr();
        let value = captures
            .get(2)
            .expect("group should exist")
            .as_bytes()
            .as_bstr();
        Ok(MacroDefinition {
            name,
            _value: value,
        })
    }
}

#[derive(Debug, Clone, Default)]
struct GccLinemarker<'a> {
    line_number: usize,
    header_name: &'a str,
    is_start_of_new_file: bool,
    is_return_to_file: bool,
    _next_is_system_header: bool,
    _next_is_extern_c: bool,
}

impl<'a> GccLinemarker<'a> {
    fn parse(line: &'a BStr) -> Result<Self> {
        let line = std::str::from_utf8(line)?;
        let err = || anyhow::anyhow!("Failed to parse line: {:?}", line);
        static RE: once_cell::sync::Lazy<regex::Regex> = once_cell::sync::Lazy::new(|| {
            regex::Regex::new(r#"# (\d+) "(.*)"\s*(\d?)\s*(\d?)\s*(\d?)\s*(\d?)"#)
                .expect("should be valid")
        });
        let Some(captures) = RE.captures(line) else {
            return Err(err());
        };
        let Some(line_number) = captures
            .get(1)
            .expect("group should exist")
            .as_str()
            .parse::<usize>()
            .ok()
        else {
            return Err(err());
        };
        let name = captures.get(2).expect("group should exist").as_str();
        let mut numbers = vec![];
        for i in 3..=6 {
            let number_str = captures.get(i).expect("group should exist").as_str();
            if number_str.is_empty() {
                continue;
            }
            let Some(number) = number_str.parse::<i32>().ok() else {
                return Err(err());
            };
            numbers.push(number);
        }

        Ok(GccLinemarker {
            line_number,
            header_name: name,
            is_start_of_new_file: numbers.contains(&1),
            is_return_to_file: numbers.contains(&2),
            _next_is_system_header: numbers.contains(&3),
            _next_is_extern_c: numbers.contains(&4),
        })
    }
}
