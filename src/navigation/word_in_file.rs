use std::sync::Arc;

use tree_sitter::{Node, QueryCursor, StreamingIterator};

use crate::file_path::FilePath;
use crate::language::Language;
use crate::line_number::LineNumber;
use crate::symbols::{Signature, push_signature};

use super::Word;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Place {
    pub path: FilePath,
    pub line: LineNumber,
}

/// What a tags query names a definition: `@definition.<kind>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefinitionKind {
    Class,
    Struct,
    Enum,
    Interface,
    Trait,
    Type,
    Function,
    Method,
    Constant,
    Module,
    Macro,
}

impl DefinitionKind {
    fn from_capture(kind: &str) -> Option<Self> {
        Some(match kind {
            "class" => Self::Class,
            "struct" => Self::Struct,
            "enum" => Self::Enum,
            "interface" => Self::Interface,
            "trait" => Self::Trait,
            "type" => Self::Type,
            "function" => Self::Function,
            "method" => Self::Method,
            "constant" => Self::Constant,
            "module" => Self::Module,
            "macro" => Self::Macro,
            _ => return None,
        })
    }

    /// When two patterns name one node, the narrower kind wins: a method over a function,
    /// a function over a constant.
    fn narrowness(self) -> u8 {
        match self {
            Self::Method => 2,
            Self::Function => 1,
            _ => 0,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Class => "class",
            Self::Struct => "struct",
            Self::Enum => "enum",
            Self::Interface => "interface",
            Self::Trait => "trait",
            Self::Type => "type",
            Self::Function => "function",
            Self::Method => "method",
            Self::Constant => "constant",
            Self::Module => "module",
            Self::Macro => "macro",
        }
    }
}

/// Where a name is defined at a commit.
#[derive(Debug, Clone)]
pub struct Definition {
    pub name: Word,
    pub kind: DefinitionKind,
    /// The line of the name, not of the doc comment or attributes above it.
    pub place: Place,
    /// The declaration without its body: `pub fn total(&self) -> u32`
    pub signature: Arc<str>,
    /// The source byte each byte of `signature` came from, `u32::MAX` for the space that joins
    /// lines; empty when the signature was not cut from a file.
    pub signature_origin: Arc<[u32]>,
    pub doc: Option<Arc<str>>,
}

/// A line at a commit that refers to a name by that name.
#[derive(Debug, Clone)]
pub struct Usage {
    pub place: Place,
    /// The line, trimmed.
    pub text: Arc<str>,
}

/// The definitions and usages of one word in one file.
pub struct WordInFile {
    definitions: Vec<Definition>,
    usages: Vec<Usage>,
}

struct Tagged<'tree> {
    node: Node<'tree>,
    name: Node<'tree>,
    kind: DefinitionKind,
}

impl WordInFile {
    pub fn read(path: &FilePath, source: &str, language: Language, word: &Word) -> Self {
        let Some(tree) = language.parser().parse(source, None) else {
            return Self {
                definitions: Vec::new(),
                usages: Vec::new(),
            };
        };
        let query = language.tags_query();

        let mut tagged: Vec<Tagged> = Vec::new();
        let mut references: Vec<Node> = Vec::new();
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(query, tree.root_node(), source.as_bytes());
        while let Some(found) = matches.next() {
            let mut name = None;
            let mut definition = None;
            let mut reference = false;
            for capture in found.captures() {
                let Some(&capture_name) = query.capture_names().get(capture.index as usize) else {
                    continue;
                };
                if capture_name == "name" {
                    name = Some(capture.node);
                } else if let Some(kind) = capture_name.strip_prefix("definition.") {
                    definition =
                        DefinitionKind::from_capture(kind).map(|kind| (capture.node, kind));
                } else if capture_name.starts_with("reference.") {
                    reference = true;
                }
            }
            let Some(name) = name.filter(|name| text_of(*name, source) == word.as_str()) else {
                continue;
            };
            if let Some((node, kind)) = definition {
                add_narrowest(&mut tagged, Tagged { node, name, kind });
            } else if reference {
                references.push(name);
            }
        }

        let lines: Vec<&str> = source.lines().collect();
        let mut definitions: Vec<Definition> = tagged
            .iter()
            .filter_map(|tagged| {
                let signature = signature_with_export(tagged.node, source);
                Some(Definition {
                    name: word.clone(),
                    kind: tagged.kind,
                    place: place_of(path, tagged.name)?,
                    signature: signature.text.into(),
                    signature_origin: signature.origin.into(),
                    doc: doc_comment_above(tagged.node, source).map(Into::into),
                })
            })
            .collect();
        definitions.sort_by_key(|definition| definition.place.line);

        let mut usages: Vec<Usage> = references
            .iter()
            .filter(|reference| {
                !tagged
                    .iter()
                    .any(|tagged| tagged.name.byte_range() == reference.byte_range())
            })
            .filter_map(|reference| {
                let place = place_of(path, *reference)?;
                let text = lines.get(reference.start_position().row)?.trim();
                Some(Usage {
                    place,
                    text: text.into(),
                })
            })
            .collect();
        usages.sort_by_key(|usage| usage.place.line);
        usages.dedup_by_key(|usage| usage.place.line);

        Self {
            definitions,
            usages,
        }
    }

    pub fn definitions(&self) -> &[Definition] {
        &self.definitions
    }

    pub fn usages(&self) -> &[Usage] {
        &self.usages
    }

    pub(super) fn into_parts(self) -> (Vec<Definition>, Vec<Usage>) {
        (self.definitions, self.usages)
    }
}

fn add_narrowest<'tree>(tagged: &mut Vec<Tagged<'tree>>, found: Tagged<'tree>) {
    match tagged
        .iter_mut()
        .find(|known| known.node.byte_range() == found.node.byte_range())
    {
        Some(known) if known.kind.narrowness() < found.kind.narrowness() => *known = found,
        Some(_) => {}
        None => tagged.push(found),
    }
}

fn place_of(path: &FilePath, node: Node) -> Option<Place> {
    Some(Place {
        path: path.clone(),
        line: LineNumber::try_from(node.start_position().row + 1).ok()?,
    })
}

fn text_of<'a>(node: Node, source: &'a str) -> &'a str {
    source.get(node.byte_range()).unwrap_or_default()
}

/// The node a doc comment sits above: an exported TypeScript declaration's export statement.
fn outermost(node: Node) -> Node {
    node.parent()
        .filter(|parent| parent.kind() == "export_statement")
        .unwrap_or(node)
}

/// `export const total = (cart: Cart): number =>`
fn signature_with_export(node: Node, source: &str) -> Signature {
    let mut signature = Signature::empty();
    let outer = outermost(node);
    if outer.id() != node.id() {
        signature.push(source, outer.start_byte()..node.start_byte());
    }
    push_signature(&mut signature, node, source);
    signature.finished()
}

/// The `///` or `/** */` comments right above a definition, past its Rust attributes.
fn doc_comment_above(node: Node, source: &str) -> Option<String> {
    let anchor = outermost(node);
    let mut first_row = anchor.start_position().row;
    let mut paragraphs: Vec<String> = Vec::new();
    let mut above = anchor.prev_sibling();
    while let Some(sibling) = above {
        if sibling.end_position().row + 1 < first_row {
            break;
        }
        if sibling.kind() == "attribute_item" {
            first_row = sibling.start_position().row;
            above = sibling.prev_sibling();
            continue;
        }
        let Some(doc) = sibling
            .kind()
            .ends_with("comment")
            .then(|| doc_text(text_of(sibling, source)))
            .flatten()
        else {
            break;
        };
        paragraphs.push(doc);
        first_row = sibling.start_position().row;
        above = sibling.prev_sibling();
    }
    paragraphs.reverse();
    let doc = paragraphs.join("\n");
    let doc = doc.trim();
    (!doc.is_empty()).then(|| doc.to_owned())
}

/// The text of a doc comment without its markers; `None` for a comment that is not a doc comment.
fn doc_text(comment: &str) -> Option<String> {
    let comment = comment.trim();
    if let Some(line) = comment.strip_prefix("///") {
        if line.starts_with('/') {
            return None;
        }
        return Some(line.strip_prefix(' ').unwrap_or(line).trim_end().to_owned());
    }
    let block = comment.strip_prefix("/**")?.strip_suffix("*/")?;
    if block.starts_with('*') || block.starts_with('/') {
        return None;
    }
    let lines: Vec<&str> = block
        .lines()
        .map(|line| {
            let line = line.trim();
            let line = line.strip_prefix('*').unwrap_or(line);
            line.strip_prefix(' ').unwrap_or(line).trim_end()
        })
        .collect();
    Some(lines.join("\n").trim().to_owned())
}
