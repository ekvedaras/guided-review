//! The lines of the declaration that starts on a line.

use crate::language::Language;
use crate::line_number::LineNumber;
use crate::line_range::LineRange;

use tree_sitter::Node;

/// The lines of the declaration that starts on `line` of `source`: the `fn`, method, class or
/// statement whose first token is the line's first token, with the doc comment and attributes right
/// above it, to the end of its body, or of its signature when it has none. `None` when no such
/// node exists (a blank line, or a line inside a larger node that started above it).
pub fn declaration_lines(language: Language, source: &str, line: LineNumber) -> Option<LineRange> {
    let tree = language.parser().parse(source, None)?;
    let row = usize::try_from(line.get()).ok()?.checked_sub(1)?;
    let line_start: usize = source.split_inclusive('\n').take(row).map(str::len).sum();
    let rest = source.get(line_start..)?;
    let first_token = line_start + (rest.len() - rest.trim_start_matches([' ', '\t']).len());
    let mut node = tree
        .root_node()
        .descendant_for_byte_range(first_token, first_token)?;
    let mut largest = None;
    let mut started_above = None;
    while let Some(parent) = node.parent() {
        if node.start_position().row < row {
            started_above = Some(node);
            break;
        }
        if node.start_position().row == row {
            largest = Some(node);
        }
        node = parent;
    }
    let declaration = started_above
        .filter(|node| only_leads_to(*node, first_token))
        .or(largest)?;
    let end = declaration.end_position();
    let last_row = if end.column == 0 {
        end.row.saturating_sub(1)
    } else {
        end.row
    }
    .max(row);
    let first_row = leading_siblings(declaration)
        .last()
        .unwrap_or(declaration)
        .start_position()
        .row
        .min(row);
    Some(LineRange::new(
        LineNumber::try_from(first_row + 1).ok()?,
        LineNumber::try_from(last_row + 1).ok()?,
    ))
}

/// Whether everything in `node` before `first_token` is attributes, decorators or comments, so
/// `node` is the declaration under them (PHP attributes, TypeScript and Python decorators).
fn only_leads_to(node: Node, first_token: usize) -> bool {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .take_while(|child| child.end_byte() <= first_token)
        .all(|child| is_lead(child))
}

/// The comments and attributes right above `declaration`, nearest first, up to a blank line.
fn leading_siblings(declaration: Node) -> impl Iterator<Item = Node> {
    std::iter::successors(Some(declaration), |below| {
        below.prev_sibling().filter(|above| {
            is_lead(*above) && above.end_position().row + 1 >= below.start_position().row
        })
    })
    .skip(1)
}

/// A comment, attribute or decorator, which belongs to the declaration below it. A Rust inner
/// attribute (`#![...]`) belongs to what encloses it.
fn is_lead(node: Node) -> bool {
    let kind = node.kind();
    (kind.contains("comment") || kind.contains("attribute") || kind == "decorator")
        && !kind.starts_with("inner")
}
