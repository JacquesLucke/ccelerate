use std::io::Write;

use anyhow::Result;
use anyhow::anyhow;
use bstr::{BStr, BString, ByteSlice};

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
            let end = m.start() + find_directive_length(remaining[m.start()..].as_bstr());
            let part = &remaining[m.start()..end];
            write!(result, "{}", part)?;
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

fn find_directive_length(code: &BStr) -> usize {
    let mut length = find_line_comment_length(code);
    let line = &code[..length];
    if let Some(pos) = line.find(b"//") {
        length = length.min(pos);
    }
    if let Some(pos) = line.find(b"/*") {
        length = length.min(pos);
    }
    length
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

#[cfg(test)]
mod tests {
    use super::*;
    use bstr::ByteSlice;

    #[test]
    fn test_extract_preprocessor_directives() {
        let dir = "";
        let files =
            glob::glob(format!("{}/home/jacques/Documents/**/*.h", dir).as_str()).expect("");
        for (i, file) in files.enumerate() {
            let Ok(file) = file else {
                continue;
            };
            let code = std::fs::read(&file).expect("should be valid");
            println!("FILE {}: {}", i, file.display());
            let directives = extract_preprocessor_directives(code.as_bytes().as_bstr())
                .expect("should be valid");
            println!("{}", directives);
        }
    }
}
