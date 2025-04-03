#![allow(dead_code)]

use std::io::Write;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Result;
use anyhow::anyhow;
use bstr::{BStr, BString, ByteSlice};

pub enum DirectivesUpdate {
    Unchanged,
    Changed,
    Removed,
}

pub fn get_corresponding_directives_path(
    directives_dir: &Path,
    original: &Path,
) -> Result<PathBuf> {
    if !original.is_absolute() {
        return Err(anyhow!("Path must be absolute"));
    }
    // TODO: Generalize making the path relative to root.
    let relative = original.strip_prefix("/")?;
    let derived_path = directives_dir.join(relative);
    Ok(derived_path)
}

pub fn get_original_path(directives_dir: &Path, derived: &Path) -> PathBuf {
    if let Ok(relative) = derived.strip_prefix(directives_dir) {
        // TODO: Generalize path root.
        PathBuf::from("/").join(relative)
    } else {
        derived.to_owned()
    }
}

pub async fn update_directives_file(
    directives_dir: &Path,
    original: &Path,
) -> Result<DirectivesUpdate> {
    let derived_path = get_corresponding_directives_path(directives_dir, original)?;

    let derived_exists = derived_path.exists();
    let original_exists = original.exists();
    if !original_exists {
        if derived_exists {
            tokio::fs::remove_file(derived_path).await?;
            return Ok(DirectivesUpdate::Removed);
        }
        return Ok(DirectivesUpdate::Unchanged);
    }
    let original_code = tokio::fs::read(original).await?;
    let updated_derived_code = extract_preprocessor_directives(original_code.as_bstr())?;

    if derived_exists {
        let old_derived_code = tokio::fs::read(&derived_path).await?;
        if old_derived_code == updated_derived_code {
            return Ok(DirectivesUpdate::Unchanged);
        }
    }
    tokio::fs::create_dir_all(derived_path.parent().expect("should be valid")).await?;
    tokio::fs::write(&derived_path, updated_derived_code).await?;
    Ok(DirectivesUpdate::Changed)
}

pub fn extract_preprocessor_directives(code: &BStr) -> Result<BString> {
    let mut result = BString::new(vec![]);
    let mut remaining = code;

    // Need to find any of the following:
    // - # at beginning of line (potentially with whitespace before it)
    // - //
    // - /*
    // - "
    // - R"delimiter(
    static RE_FIND_START: once_cell::sync::Lazy<regex::bytes::Regex> = once_cell::sync::Lazy::new(
        || {
            regex::bytes::Regex::new(
            r#"(?m)(?P<preproc>^[ \t]*#)|(?P<line_comment>//)|(?P<block_comment>/\*)|(?P<string>")|(?P<char>')|(?P<raw>R"[^(\r\n]*\()"#,
        )
        .expect("should be valid")
        },
    );

    while let Some(capture) = RE_FIND_START.captures(remaining) {
        if let Some(m) = capture.name("preproc") {
            let end = m.start() + find_directive_length(remaining[m.start()..].as_bstr())?;
            let part = &remaining[m.start()..end];
            write!(result, "{}", part)?;
            // println!("PREP: {}", part.to_str_lossy());
            remaining = remaining[end..].as_bstr();
        } else if let Some(m) = capture.name("line_comment") {
            let length = find_line_comment_length(remaining[m.start()..].as_bstr());
            let end = m.start() + length;
            remaining = remaining[end..].as_bstr();
        } else if let Some(m) = capture.name("block_comment") {
            let length = find_block_comment_length(remaining[m.start()..].as_bstr())?;
            let end = m.start() + length;
            remaining = remaining[end..].as_bstr();
        } else if let Some(m) = capture.name("string") {
            let length = find_string_length(remaining[m.start()..].as_bstr())?;
            let end = m.start() + length;
            // println!("STRING: {}", remaining[m.start()..end].to_str_lossy());
            remaining = remaining[end..].as_bstr();
        } else if let Some(m) = capture.name("char") {
            let start = m.start();
            if start > 0 && remaining[start - 1].is_ascii_hexdigit() {
                // Digit seperator.
                remaining = remaining[start + 1..].as_bstr();
            } else {
                let length = find_char_length(remaining[start..].as_bstr())?;
                let end = start + length;
                // println!("CHAR: {}", remaining[start..end].to_str_lossy());
                remaining = remaining[end..].as_bstr();
            }
        } else if let Some(m) = capture.name("raw") {
            let length = find_raw_string_length(remaining[m.start()..].as_bstr())?;
            let end = m.start() + length;
            remaining = remaining[end..].as_bstr();
        }
    }
    Ok(result)
}

fn find_line_comment_length(code: &BStr) -> usize {
    for (i, b) in code.iter().enumerate() {
        if *b == b'\n' {
            let ending = &code[..i];
            if ending.ends_with(b"\\") {
                continue;
            }
            if ending.ends_with(b"\\\r") {
                continue;
            }
            return i + 1;
        }
    }
    code.len()
}

fn find_directive_length(code: &BStr) -> Result<usize> {
    static RE_FIND_NEXT: once_cell::sync::Lazy<regex::bytes::Regex> = once_cell::sync::Lazy::new(
        || {
            regex::bytes::Regex::new(r#"(?m)(?P<newline>\n)|(?P<line_comment>//)|(?P<block_comment>/\*)|(?P<string>")|(?P<char>')|(?P<raw>R"[^(\r\n]*\()"#)
                .expect("should be valid")
        },
    );

    let mut current = 0;
    while let Some(capture) = RE_FIND_NEXT.captures(&code[current..]) {
        if let Some(m) = capture.name("newline") {
            let i = current + m.start();
            let before = &code[..i];
            if before.ends_with(b"\\") {
                current = i + 1;
                continue;
            }
            return Ok(i + 1);
        } else if let Some(m) = capture.name("line_comment") {
            let i = current + m.start();
            let length = find_line_comment_length(&code[i..]);
            return Ok(i + length);
        } else if let Some(m) = capture.name("block_comment") {
            let i = current + m.start();
            let length = find_block_comment_length(&code[i..])?;
            current = i + length;
        } else if let Some(m) = capture.name("string") {
            let i = current + m.start();
            let length = find_string_length(&code[i..])?;
            current = i + length;
        } else if let Some(m) = capture.name("char") {
            let i = current + m.start();
            let length = find_char_length(&code[i..])?;
            current = i + length;
        } else if let Some(m) = capture.name("raw") {
            let i = current + m.start();
            let length = find_raw_string_length(&code[i..])?;
            current = i + length;
        }
    }
    Ok(code.len())
}

fn find_block_comment_length(code: &BStr) -> Result<usize> {
    match code.find(b"*/") {
        Some(end) => Ok(end + 2),
        None => Err(anyhow!("Failed to find end of block comment")),
    }
}

fn count_trailing_backslashes(code: &BStr) -> usize {
    code.iter().rev().take_while(|b| **b == b'\\').count()
}

fn find_string_length(code: &BStr) -> Result<usize> {
    for (i, b) in code.iter().enumerate().skip(1) {
        if *b == b'"' {
            let ending = &code[..i];
            let slash_count = count_trailing_backslashes(ending);
            if slash_count % 2 == 1 {
                continue;
            }
            return Ok(i + 1);
        }
    }
    Err(anyhow!(
        "Failed to find end of string: {}",
        code[..100.min(code.len())].to_str_lossy()
    ))
}

fn find_char_length(code: &BStr) -> Result<usize> {
    for (i, b) in code.iter().enumerate().skip(1) {
        if *b == b'\'' {
            let ending = &code[..i];
            let slash_count = count_trailing_backslashes(ending);
            if slash_count % 2 == 1 {
                continue;
            }
            return Ok(i + 1);
        }
    }
    Err(anyhow!("Failed to find end of char"))
}

fn find_raw_string_length(code: &BStr) -> Result<usize> {
    let Some(delimiter_end) = code.find_byte(b'(') else {
        return Err(anyhow!("Failed to determine delimiter"));
    };
    let delimiter = &code[2..delimiter_end];
    let ending = format!("){}\"", delimiter);
    match code.find(&ending) {
        Some(end) => Ok(end + ending.len()),
        None => Err(anyhow!("Failed to find end of raw string")),
    }
}
