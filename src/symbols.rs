use std::collections::HashMap;
use std::ops::{Range, RangeInclusive};

use tree_sitter::{Node, Parser, Query, QueryCursor, StreamingIterator};

use crate::language::Language;
use crate::pieces::PieceKind;

pub struct Symbol {
    /// `Order::total`
    pub path: String,
    pub kind: PieceKind,
    pub first_line: usize,
    pub last_line: usize,
    pub signature: String,
}

impl Symbol {
    fn contains(&self, line: usize) -> bool {
        (self.first_line..=self.last_line).contains(&line)
    }
}

/// Where a line of a source file belongs: a symbol, the imports, or the rest of the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Slot {
    Symbol(String),
    Imports,
    Top,
}

impl Slot {
    pub fn id(&self) -> String {
        match self {
            Self::Symbol(path) => path.clone(),
            Self::Imports => "imports".to_string(),
            Self::Top => "(top)".to_string(),
        }
    }
}

struct Token {
    text: String,
    line: usize,
}

pub struct Symbols {
    symbols: Vec<Symbol>,
    imports: Vec<RangeInclusive<usize>>,
    tokens: Vec<Token>,
    lines: Vec<String>,
}

impl Symbols {
    #[expect(
        clippy::expect_used,
        reason = "the grammars and symbol queries are compiled in, so they match tree-sitter"
    )]
    pub fn find(source: &str, language: Language) -> Self {
        let grammar = language.grammar();
        let mut parser = Parser::new();
        parser
            .set_language(&grammar)
            .expect("grammar matches tree-sitter");
        let lines = source.lines().map(str::to_string).collect();
        let Some(tree) = parser.parse(source, None) else {
            return Self {
                symbols: Vec::new(),
                imports: Vec::new(),
                tokens: Vec::new(),
                lines,
            };
        };
        let query = Query::new(&grammar, language.symbol_query()).expect("valid symbol query");

        let mut found: Vec<(Node, &str, PieceKind)> = Vec::new();
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&query, tree.root_node(), source.as_bytes());
        while let Some(found_match) = matches.next() {
            let mut name = "";
            let mut definition = None;
            for capture in found_match.captures() {
                match query.capture_names()[capture.index as usize] {
                    helper if helper.starts_with('_') => {}
                    "name" => name = &source[capture.node.byte_range()],
                    capture_name => {
                        if let Some(kind) = kind_of(capture_name) {
                            definition = Some((capture.node, kind));
                        }
                    }
                }
            }
            if let Some((node, kind)) = definition {
                found.push((node, name, kind));
            }
        }
        found.sort_by_key(|(node, ..)| (node.start_byte(), std::cmp::Reverse(node.end_byte())));

        let mut symbols = Vec::new();
        let mut enclosing: Vec<(Node, String)> = Vec::new();
        for (node, name, kind) in found {
            let name = name.replace(['#', '@', '/'], "_");
            while enclosing.last().is_some_and(|(outer, _)| {
                outer.end_byte() < node.end_byte() || outer.byte_range() == node.byte_range()
            }) {
                enclosing.pop();
            }
            let path = match enclosing.last() {
                Some((_, outer_path)) => format!("{outer_path}::{name}"),
                None => name,
            };
            let kind = if language.is_test(kind, &path) {
                PieceKind::Test
            } else {
                kind
            };
            symbols.push(Symbol {
                path: path.clone(),
                kind,
                first_line: first_line_with_docblock(node),
                last_line: node.end_position().row + 1,
                signature: signature_of(node, source),
            });
            enclosing.push((node, path));
        }
        Self {
            symbols,
            imports: import_lines(tree.root_node(), language),
            tokens: tokens_of(tree.root_node(), source),
            lines,
        }
    }

    pub fn slot_at(&self, line: usize) -> Slot {
        if let Some(symbol) = self.innermost_at(line) {
            Slot::Symbol(symbol.path.clone())
        } else if self.imports.iter().any(|lines| lines.contains(&line)) {
            Slot::Imports
        } else {
            Slot::Top
        }
    }

    /// Tokens in the lines the slot owns; layout and blank lines are not tokens.
    pub fn tokens_in(&self, slot: &Slot) -> Vec<&str> {
        self.tokens
            .iter()
            .filter(|token| self.slot_at(token.line) == *slot)
            .map(|token| token.text.as_str())
            .collect()
    }

    pub fn text_in(&self, slot: &Slot) -> String {
        self.lines
            .iter()
            .enumerate()
            .filter(|(index, _)| self.slot_at(index + 1) == *slot)
            .map(|(_, line)| line.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Maps each base symbol path to the head path it continues as. Same paths pair first,
    /// then the symbols left over pair top-down with the same kind under the same parent.
    pub fn pair_with_head(&self, head: &Self) -> HashMap<String, String> {
        let mut pairs: HashMap<String, String> = HashMap::new();
        let mut claimed: Vec<&str> = Vec::new();
        let max_depth = self.symbols.iter().map(|s| depth_of(&s.path)).max();
        for depth in 0..=max_depth.unwrap_or_default() {
            let level: Vec<&Symbol> = self
                .symbols
                .iter()
                .filter(|symbol| depth_of(&symbol.path) == depth)
                .collect();
            let mut unpaired = Vec::new();
            for symbol in level {
                let Some(parent) = head_parent_of(&symbol.path, &pairs) else {
                    continue;
                };
                let head_path = join_path(parent.as_deref(), name_of(&symbol.path));
                if let Some(same_path) = head.named(&head_path) {
                    claimed.push(&same_path.path);
                    pairs.insert(symbol.path.clone(), head_path);
                } else {
                    unpaired.push((symbol, parent));
                }
            }
            for (symbol, parent) in unpaired {
                let candidate = head.symbols.iter().find(|candidate| {
                    candidate.kind == symbol.kind
                        && depth_of(&candidate.path) == depth
                        && parent_of(&candidate.path) == parent.as_deref()
                        && !claimed.contains(&candidate.path.as_str())
                        && self.named(&candidate.path).is_none()
                });
                if let Some(candidate) = candidate {
                    claimed.push(&candidate.path);
                    pairs.insert(symbol.path.clone(), candidate.path.clone());
                }
            }
        }
        pairs
    }

    pub fn innermost_at(&self, line: usize) -> Option<&Symbol> {
        self.symbols
            .iter()
            .filter(|symbol| symbol.contains(line))
            .max_by_key(|symbol| symbol.first_line)
    }

    pub fn named(&self, path: &str) -> Option<&Symbol> {
        self.symbols.iter().find(|symbol| symbol.path == path)
    }
}

fn depth_of(path: &str) -> usize {
    path.matches("::").count()
}

fn parent_of(path: &str) -> Option<&str> {
    path.rsplit_once("::").map(|(parent, _)| parent)
}

fn name_of(path: &str) -> &str {
    path.rsplit_once("::").map_or(path, |(_, name)| name)
}

fn join_path(parent: Option<&str>, name: &str) -> String {
    parent.map_or_else(|| name.to_string(), |parent| format!("{parent}::{name}"))
}

/// The head path of the parent: `Some(None)` for a top-level symbol, `None` when the parent has no head path.
fn head_parent_of(path: &str, pairs: &HashMap<String, String>) -> Option<Option<String>> {
    match parent_of(path) {
        None => Some(None),
        Some(parent) => pairs.get(parent).map(|head| Some(head.clone())),
    }
}

/// A docblock sits right above its symbol and belongs to it.
fn first_line_with_docblock(node: Node) -> usize {
    let mut first_row = node.start_position().row;
    let anchor = node
        .parent()
        .filter(|parent| parent.kind() == "export_statement")
        .unwrap_or(node);
    first_row = first_row.min(anchor.start_position().row);
    let mut above = anchor.prev_sibling();
    while let Some(comment) = above.filter(|above| above.kind() == "comment") {
        if comment.end_position().row + 1 < first_row {
            break;
        }
        first_row = comment.start_position().row;
        above = comment.prev_sibling();
    }
    first_row + 1
}

fn import_lines(root: Node, language: Language) -> Vec<RangeInclusive<usize>> {
    let mut lines = Vec::new();
    let mut unvisited = vec![root];
    while let Some(node) = unvisited.pop() {
        if language.is_import(node) {
            lines.push(node.start_position().row + 1..=node.end_position().row + 1);
        } else {
            let mut children = node.walk();
            unvisited.extend(node.children(&mut children));
        }
    }
    lines
}

fn tokens_of(root: Node, source: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut unvisited = vec![root];
    while let Some(node) = unvisited.pop() {
        if node.child_count() == 0 {
            if !source[node.byte_range()].trim().is_empty() {
                tokens.push(Token {
                    text: source[node.byte_range()].to_string(),
                    line: node.start_position().row + 1,
                });
            }
        } else {
            let mut children = node.walk();
            let mut node_children: Vec<Node> = node.children(&mut children).collect();
            node_children.reverse();
            unvisited.extend(node_children);
        }
    }
    tokens
}

/// None for a capture that names no kind, such as one a query only uses to find its parts.
fn kind_of(capture: &str) -> Option<PieceKind> {
    Some(match capture {
        "class" => PieceKind::Class,
        "interface" => PieceKind::Interface,
        "trait" => PieceKind::Trait,
        "enum" => PieceKind::Enum,
        "type_alias" => PieceKind::TypeAlias,
        "const" => PieceKind::Const,
        "component" => PieceKind::Component,
        "test" => PieceKind::Test,
        "method" => PieceKind::Method,
        "function" => PieceKind::Function,
        _ => return None,
    })
}

/// The declaration without attributes and body: `public function total(): int`
fn signature_of(node: Node, source: &str) -> String {
    let mut children = node.walk();
    let start = node
        .children(&mut children)
        .find(|child| !matches!(child.kind(), "attribute_list" | "decorator" | "comment"))
        .map_or(node.start_byte(), |child| child.start_byte());
    let end = body_of(node).map_or(node.end_byte(), |body| body.start_byte());
    let mut declaration = String::new();
    let mut from = start;
    for comment in comments_inside(node) {
        if comment.start >= from && comment.end <= end {
            declaration.push_str(&source[from..comment.start]);
            from = comment.end;
        }
    }
    declaration.push_str(&source[from..end.max(from)]);
    let declaration = declaration.trim_end().trim_end_matches(';');
    declaration.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Arrow functions and test callbacks keep their body further down:
/// `const Cart = () => {}`, `it("works", () => {})`.
fn body_of(node: Node) -> Option<Node> {
    node.child_by_field_name("body").or_else(|| {
        if node.kind() == "expression_statement" {
            let arguments = node.named_child(0)?.child_by_field_name("arguments")?;
            let callback = arguments
                .named_child(u32::try_from(arguments.named_child_count().checked_sub(1)?).ok()?)?;
            return callback.child_by_field_name("body");
        }
        let mut children = node.walk();
        let declarator = node
            .children(&mut children)
            .find(|child| child.kind() == "variable_declarator")?;
        declarator
            .child_by_field_name("value")?
            .child_by_field_name("body")
    })
}

fn comments_inside(node: Node) -> Vec<Range<usize>> {
    let mut comments = Vec::new();
    let mut unvisited = vec![node];
    while let Some(node) = unvisited.pop() {
        if node.kind() == "comment" {
            comments.push(node.byte_range());
            continue;
        }
        let mut children = node.walk();
        unvisited.extend(node.children(&mut children));
    }
    comments.sort_by_key(|comment| comment.start);
    comments
}
