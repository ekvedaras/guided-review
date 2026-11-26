//! Places in a file as LSP names them: a file's URI, a position's character in the unit the server
//! counts, and a language's id.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use crate::language::Language;
use crate::line_number::LineNumber;

use super::Column;

/// Counts a position's column in the unit the server speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Encoding {
    Utf8,
    Utf16,
}

impl Encoding {
    /// The server's `positionEncoding`; UTF-16 is what LSP assumes when a server names none.
    pub(super) fn named(name: Option<&str>) -> Self {
        match name {
            Some("utf-8") => Self::Utf8,
            _ => Self::Utf16,
        }
    }

    /// The character of `column` (a byte offset) on `line` of `text`.
    pub(super) fn character(self, text: &str, line: LineNumber, column: Column) -> Option<u32> {
        let line_text = text.lines().nth(usize::from(line).checked_sub(1)?)?;
        let before = line_text.get(..usize::try_from(column.get()).ok()?)?;
        match self {
            Self::Utf8 => u32::try_from(before.len()).ok(),
            Self::Utf16 => u32::try_from(before.encode_utf16().count()).ok(),
        }
    }
}

pub(super) fn language_id(language: Language) -> &'static str {
    match language {
        Language::Rust => "rust",
        Language::Php => "php",
        Language::TypeScript => "typescript",
        Language::Tsx => "typescriptreact",
        Language::Python => "python",
        Language::Go => "go",
    }
}

pub(super) fn uri_of(path: &Path) -> String {
    let mut uri = String::from("file://");
    for byte in path.to_string_lossy().bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b'-' | b'_' | b'.' | b'~' => {
                uri.push(char::from(byte));
            }
            other => {
                // Writing to a String cannot fail.
                write!(uri, "%{other:02X}").ok();
            }
        }
    }
    uri
}

pub(super) fn path_of(uri: &str) -> Option<PathBuf> {
    let encoded = uri.strip_prefix("file://")?;
    let mut bytes = Vec::new();
    let mut rest = encoded.bytes();
    while let Some(byte) = rest.next() {
        if byte == b'%' {
            let high = rest.next()?;
            let low = rest.next()?;
            let pair = [high, low];
            let hex = std::str::from_utf8(&pair).ok()?;
            bytes.push(u8::from_str_radix(hex, 16).ok()?);
        } else {
            bytes.push(byte);
        }
    }
    String::from_utf8(bytes).ok().map(PathBuf::from)
}
