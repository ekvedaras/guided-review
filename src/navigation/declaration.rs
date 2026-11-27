//! Where the declaration that starts on a line ends.

use crate::language::Language;
use crate::line_number::LineNumber;

/// The last line of the largest declaration that starts on `line` of `source`: the `fn`, method,
/// class or statement whose first token is the line's first token. `None` when no such node
/// exists (a blank line, or a line inside a larger node that started above it).
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
    };
    LineNumber::try_from(last_row.max(row) + 1).ok()
}
