//! Where the declaration that starts on a line ends.

use crate::language::Language;
use crate::line_number::LineNumber;

/// The last line of the largest declaration that starts on `line` of `source`: the `fn`, method,
/// class or statement whose first token is the line's first token. `None` when no such node
/// exists (a blank line, or a line inside a larger node that started above it).
///
/// An attribute or decorator above the declaration belongs to its node, which then starts above
/// `line`; the tree then gives only the line's own tokens, so a brace language goes on to the
/// brace that opens the body, wherever it is, and ends at its partner.
pub fn declaration_end(language: Language, source: &str, line: LineNumber) -> Option<LineNumber> {
    let tree = language.parser().parse(source, None)?;
    let row = usize::try_from(line.get()).ok()?.checked_sub(1)?;
    let line_start: usize = source.split_inclusive('\n').take(row).map(str::len).sum();
    let rest = source.get(line_start..)?;
    let first_token = line_start + (rest.len() - rest.trim_start_matches([' ', '\t']).len());
    let mut node = tree
        .root_node()
        .descendant_for_byte_range(first_token, first_token)?;
    let mut largest = None;
    while let Some(parent) = node.parent() {
        if node.start_position().row < row {
            break;
        }
        if node.start_position().row == row {
            largest = Some(node);
        }
        node = parent;
    }
    let end = largest?.end_position();
    let last_row = if end.column == 0 {
        end.row.saturating_sub(1)
    } else {
        end.row
    }
    .max(row);
    let last_row = if last_row == row && language != Language::Python {
        body_end_row(language, source, first_token).unwrap_or(row)
    } else {
        last_row
    };
    LineNumber::try_from(last_row + 1).ok()
}

/// The row of the brace that closes the first block opened after `from`, when a `{` comes before
/// any `;` outside parentheses and brackets (so a signature's `[u8; 3]` or a default `'{'` does not
/// stop the search).
fn body_end_row(language: Language, source: &str, from: usize) -> Option<usize> {
    let text = source.get(from..)?;
    let bytes = text.as_bytes();
    let mut row = source.get(..from)?.matches('\n').count();
    let mut nesting = 0_usize;
    let mut braces = 0_usize;
    let mut at = 0;
    while let Some(&byte) = bytes.get(at) {
        match byte {
            b'\n' => row += 1,
            b'/' if bytes.get(at + 1) == Some(&b'/') => at = skip_to_line_end(bytes, at),
            b'#' if language == Language::Php && bytes.get(at + 1) != Some(&b'[') => {
                at = skip_to_line_end(bytes, at);
            }
            b'/' if bytes.get(at + 1) == Some(&b'*') => {
                let (next, rows) = skip_block_comment(bytes, at);
                at = next;
                row += rows;
            }
            b'"' | b'\'' | b'`' => {
                if byte == b'\'' && language == Language::Rust && !is_char_literal(bytes, at) {
                    at += 1;
                    continue;
                }
                let (next, rows) = skip_quoted(bytes, at, byte);
                at = next;
                row += rows;
            }
            b'(' | b'[' => nesting += 1,
            b')' | b']' => nesting = nesting.saturating_sub(1),
            b';' if nesting == 0 && braces == 0 => return None,
            b'{' if nesting == 0 || braces > 0 => braces += 1,
            b'}' if braces > 0 => {
                braces -= 1;
                if braces == 0 {
                    return Some(row);
                }
            }
            _ => {}
        }
        at += 1;
    }
    None
}

fn skip_to_line_end(bytes: &[u8], from: usize) -> usize {
    bytes
        .iter()
        .enumerate()
        .skip(from)
        .find(|(_, byte)| **byte == b'\n')
        .map_or(bytes.len(), |(newline, _)| newline.saturating_sub(1))
}

/// The index of the `/` that ends the comment, and the newlines inside it.
fn skip_block_comment(bytes: &[u8], from: usize) -> (usize, usize) {
    let mut rows = 0;
    let mut at = from + 2;
    while let Some(byte) = bytes.get(at) {
        if *byte == b'\n' {
            rows += 1;
        } else if *byte == b'*' && bytes.get(at + 1) == Some(&b'/') {
            return (at + 1, rows);
        }
        at += 1;
    }
    (bytes.len(), rows)
}

/// The index of the closing quote, and the newlines inside the literal.
fn skip_quoted(bytes: &[u8], from: usize, quote: u8) -> (usize, usize) {
    let mut rows = 0;
    let mut at = from + 1;
    while let Some(byte) = bytes.get(at) {
        match *byte {
            b'\\' => at += 1,
            b'\n' => rows += 1,
            byte if byte == quote => return (at, rows),
            _ => {}
        }
        at += 1;
    }
    (bytes.len(), rows)
}

/// `'x'` or `'\n'`, as against a lifetime like `'a`.
fn is_char_literal(bytes: &[u8], at: usize) -> bool {
    match bytes.get(at + 1) {
        Some(b'\\') => true,
        Some(_) => bytes.get(at + 2) == Some(&b'\''),
        None => false,
    }
}
