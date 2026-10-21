use std::ops::Range;

use tree_sitter::{Node, Parser, Query, QueryCursor, StreamingIterator};

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

pub struct Symbols(Vec<Symbol>);

impl Symbols {
    pub fn find(source: &str) -> Self {
        let language = tree_sitter_php::LANGUAGE_PHP.into();
        let mut parser = Parser::new();
        parser
            .set_language(&language)
            .expect("PHP grammar matches tree-sitter");
        let Some(tree) = parser.parse(source, None) else {
            return Self(Vec::new());
        };
        let query = Query::new(&language, include_str!("../queries/php/symbols.scm"))
            .expect("valid PHP query");

        let mut found: Vec<(Node, &str, PieceKind)> = Vec::new();
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&query, tree.root_node(), source.as_bytes());
        while let Some(found_match) = matches.next() {
            let mut name = "";
            let mut definition = None;
            for capture in found_match.captures() {
                match query.capture_names()[capture.index as usize] {
                    "name" => name = &source[capture.node.byte_range()],
                    kind => definition = Some((capture.node, kind_of(kind))),
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
            while enclosing
                .last()
                .is_some_and(|(outer, _)| outer.end_byte() < node.end_byte())
            {
                enclosing.pop();
            }
            let path = match enclosing.last() {
                Some((_, outer_path)) => format!("{outer_path}::{name}"),
                None => name.to_string(),
            };
            symbols.push(Symbol {
                path: path.clone(),
                kind,
                first_line: node.start_position().row + 1,
                last_line: node.end_position().row + 1,
                signature: signature_of(node, source),
            });
            enclosing.push((node, path));
        }
        Self(symbols)
    }

    pub fn innermost_at(&self, line: usize) -> Option<&Symbol> {
        self.0
            .iter()
            .filter(|symbol| symbol.contains(line))
            .max_by_key(|symbol| symbol.first_line)
    }

    pub fn named(&self, path: &str) -> Option<&Symbol> {
        self.0.iter().find(|symbol| symbol.path == path)
    }
}

fn kind_of(capture: &str) -> PieceKind {
    match capture {
        "class" => PieceKind::Class,
        "interface" => PieceKind::Interface,
        "trait" => PieceKind::Trait,
        "enum" => PieceKind::Enum,
        "method" => PieceKind::Method,
        "function" => PieceKind::Function,
        other => unreachable!("unknown capture @{other} in queries/php/symbols.scm"),
    }
}

/// The declaration without attributes and body: `public function total(): int`
fn signature_of(node: Node, source: &str) -> String {
    let mut children = node.walk();
    let start = node
        .children(&mut children)
        .find(|child| child.kind() != "attribute_list" && child.kind() != "comment")
        .map_or(node.start_byte(), |child| child.start_byte());
    let end = node
        .child_by_field_name("body")
        .map_or(node.end_byte(), |body| body.start_byte());
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
