//! A local, a variable or parameter of a function, is the same thing only inside that function,
//! so it is looked up there and nowhere else.

use std::sync::Arc;

use tree_sitter::{Node, Point};

use crate::file_path::FilePath;
use crate::language::Language;
use crate::line_number::LineNumber;
use crate::symbols::Signature;

use super::{Column, Definition, DefinitionKind, Place, Usage, Word};

/// A local's definition, where it is first given a value or taken as a parameter, and its usages
/// in its function.
pub(super) struct Local {
    pub(super) definition: Option<Definition>,
    pub(super) usages: Vec<Usage>,
}

/// How a function-like node bounds the names inside it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Scope {
    /// Sees no name of the code around it: a PHP function, method or closure.
    Closed,
    /// Sees no local of the code around it, but a name it does not bind is no local either: a Rust
    /// `fn`, where it is a constant, a function or a unit struct.
    Wall,
    /// Sees the locals around it: a closure, an arrow function, a TypeScript function.
    Open,
}

impl Local {
    /// The local named `word` that starts at `column` of `line`, or `None` when the name
    /// there is no local: a property, method, function, type or module-level name.
    pub(super) fn at(
        path: &FilePath,
        source: &str,
        language: Language,
        line: LineNumber,
        column: Column,
        word: &Word,
    ) -> Option<Self> {
        let tree = language.parser().parse(source, None)?;
        let point = Point::new(
            usize::from(line).checked_sub(1)?,
            usize::try_from(column.get()).ok()?,
        );
        let hovered = tree.root_node().descendant_for_point_range(point, point)?;
        let rules = Rules {
            language,
            source,
            word,
        };
        if !rules.is_local_name(hovered) {
            return None;
        }
        let mut above = hovered.parent();
        while let Some(node) = above {
            above = node.parent();
            let Some(scope) = rules.scope(node) else {
                continue;
            };
            if is_field(node, "name", hovered) {
                continue;
            }
            let names = rules.names_in(node);
            let first_binding = names
                .iter()
                .find_map(|name| rules.binding(*name).map(|kind| (*name, kind)));
            match (first_binding, scope) {
                (Some(binding), _) => return Some(rules.local(path, &names, Some(binding))),
                (None, Scope::Closed) => return Some(rules.local(path, &names, None)),
                (None, Scope::Wall) => return None,
                (None, Scope::Open) => {}
            }
        }
        None
    }
}

struct Rules<'a> {
    language: Language,
    source: &'a str,
    word: &'a Word,
}

impl Rules<'_> {
    fn text(&self, node: Node) -> &str {
        self.source.get(node.byte_range()).unwrap_or_default()
    }

    /// A name the local could be read or bound as here, by kind and text alone.
    fn is_local_name(&self, node: Node) -> bool {
        if self.text(node) != self.word.as_str() {
            return false;
        }
        match self.language {
            Language::Php => {
                node.kind() == "name"
                    && self.word.as_str() != "this"
                    && node.parent().is_some_and(|variable| {
                        variable.kind() == "variable_name"
                            && variable.parent().is_none_or(|holder| {
                                !matches!(
                                    holder.kind(),
                                    "property_element" | "scoped_property_access_expression"
                                )
                            })
                    })
            }
            Language::Rust => {
                matches!(node.kind(), "identifier" | "shorthand_field_identifier")
                    && !self.word.as_str().starts_with(char::is_uppercase)
            }
            Language::TypeScript | Language::Tsx => matches!(
                node.kind(),
                "identifier"
                    | "shorthand_property_identifier"
                    | "shorthand_property_identifier_pattern"
            ),
        }
    }

    fn scope(&self, node: Node) -> Option<Scope> {
        Some(match (self.language, node.kind()) {
            (
                Language::Php,
                "function_definition" | "method_declaration" | "anonymous_function",
            ) => Scope::Closed,
            (Language::Rust, "function_item") => Scope::Wall,
            (Language::Php, "arrow_function")
            | (Language::Rust, "closure_expression")
            | (
                Language::TypeScript | Language::Tsx,
                "function_declaration"
                | "generator_function_declaration"
                | "function_expression"
                | "generator_function"
                | "arrow_function"
                | "method_definition",
            ) => Scope::Open,
            _ => return None,
        })
    }

    /// Every name in `scope` that can be the local, in source order. A function inside it that
    /// binds the name, or sees nothing around it, has a local of its own and is left out.
    fn names_in<'tree>(&self, scope: Node<'tree>) -> Vec<Node<'tree>> {
        let mut names = Vec::new();
        let mut cursor = scope.walk();
        let own_name = scope.child_by_field_name("name").map(|name| name.id());
        let mut unvisited: Vec<Node> = scope
            .children(&mut cursor)
            .filter(|child| Some(child.id()) != own_name)
            .collect();
        while let Some(node) = unvisited.pop() {
            if self.is_local_name(node) {
                names.push(node);
                continue;
            }
            if let Some(inner) = self.scope(node) {
                let inner_names = self.names_in(node);
                let has_own_local = inner != Scope::Open
                    || inner_names.iter().any(|name| self.binding(*name).is_some());
                if !has_own_local {
                    names.extend(inner_names);
                }
                continue;
            }
            unvisited.extend(node.children(&mut cursor));
        }
        names.sort_by_key(Node::start_byte);
        names
    }

    /// What the name is when it binds the local here: a parameter, or a variable given a value.
    fn binding(&self, name: Node) -> Option<DefinitionKind> {
        match self.language {
            Language::Php => php_binding(name),
            Language::Rust => rust_binding(name),
            Language::TypeScript | Language::Tsx => typescript_binding(name),
        }
    }

    fn local(
        &self,
        path: &FilePath,
        names: &[Node],
        binding: Option<(Node, DefinitionKind)>,
    ) -> Local {
        let lines: Vec<&str> = self.source.lines().collect();
        let definition = binding.and_then(|(name, kind)| {
            let place = place_of(path, name)?;
            let line_start = self
                .source
                .get(..name.start_byte())?
                .rfind('\n')
                .map_or(0, |newline| newline + 1);
            let line_end = self
                .source
                .get(line_start..)?
                .find('\n')
                .map_or(self.source.len(), |end| line_start + end);
            let mut signature = Signature::empty();
            signature.push(self.source, line_start..line_end);
            Some(Definition {
                name: self.word.clone(),
                kind,
                place,
                signature: signature.text.into(),
                signature_origin: signature.origin.into(),
                doc: None,
            })
        });
        let defined_at = binding.map(|(name, _)| name.id());
        let mut usages: Vec<Usage> = names
            .iter()
            .filter(|name| Some(name.id()) != defined_at)
            .filter_map(|name| {
                let text = lines.get(name.start_position().row)?.trim();
                Some(Usage {
                    place: place_of(path, *name)?,
                    text: Arc::from(text),
                })
            })
            .collect();
        usages.dedup_by_key(|usage| usage.place.line);
        Local { definition, usages }
    }
}

fn place_of(path: &FilePath, node: Node) -> Option<Place> {
    Some(Place {
        path: path.clone(),
        line: LineNumber::try_from(node.start_position().row + 1).ok()?,
    })
}

fn is_field<'tree>(parent: Node<'tree>, field: &str, child: Node<'tree>) -> bool {
    parent
        .child_by_field_name(field)
        .is_some_and(|named| named.id() == child.id())
}

/// `$order` in `function plan(Order $order)`, `$date = ...`, `foreach ($days as $day)`.
fn php_binding(name: Node) -> Option<DefinitionKind> {
    let mut node = name.parent()?;
    loop {
        let parent = node.parent()?;
        match parent.kind() {
            "by_ref" | "list_literal" | "pair" => node = parent,
            "simple_parameter"
            | "variadic_parameter"
            | "property_promotion_parameter"
            | "anonymous_function_use_clause" => return Some(DefinitionKind::Parameter),
            "assignment_expression" | "reference_assignment_expression" => {
                return is_field(parent, "left", node).then_some(DefinitionKind::Variable);
            }
            "foreach_statement" => {
                let is_subject = parent
                    .named_child(0)
                    .is_some_and(|first| first.id() == node.id());
                return (!is_subject).then_some(DefinitionKind::Variable);
            }
            "catch_clause" | "static_variable_declaration" | "global_declaration" => {
                return Some(DefinitionKind::Variable);
            }
            _ => return None,
        }
    }
}

/// `let total = ...`, `fn pay(cart: &Cart)`, `|item| ...`, `for line in lines`, `if let Some(x)`.
fn rust_binding(name: Node) -> Option<DefinitionKind> {
    let mut node = name;
    loop {
        let parent = node.parent()?;
        match parent.kind() {
            "tuple_struct_pattern" if is_field(parent, "type", node) => return None,
            "match_pattern" if is_field(parent, "condition", node) => return None,
            "match_pattern" | "let_condition" | "for_expression" | "let_declaration" => {
                let is_pattern =
                    parent.kind() == "match_pattern" || is_field(parent, "pattern", node);
                return is_pattern.then_some(DefinitionKind::Variable);
            }
            "parameter" => {
                return is_field(parent, "pattern", node).then_some(DefinitionKind::Parameter);
            }
            "closure_parameters" => return Some(DefinitionKind::Parameter),
            kind if kind.ends_with("_pattern") => node = parent,
            _ => return None,
        }
    }
}

/// `const total = ...`, `function pay(cart: Cart)`, `item => ...`, `for (const line of lines)`.
fn typescript_binding(name: Node) -> Option<DefinitionKind> {
    if name.kind() == "shorthand_property_identifier" {
        return None;
    }
    let mut node = name;
    loop {
        let parent = node.parent()?;
        match parent.kind() {
            "object_pattern" | "array_pattern" | "rest_pattern" => node = parent,
            "pair_pattern" if is_field(parent, "value", node) => node = parent,
            "object_assignment_pattern" | "assignment_pattern"
                if is_field(parent, "left", node) =>
            {
                node = parent;
            }
            "variable_declarator" => {
                return is_field(parent, "name", node).then_some(DefinitionKind::Variable);
            }
            "required_parameter" | "optional_parameter" => {
                let is_bound = is_field(parent, "pattern", node) || is_field(parent, "name", node);
                return is_bound.then_some(DefinitionKind::Parameter);
            }
            "arrow_function" => {
                return is_field(parent, "parameter", node).then_some(DefinitionKind::Parameter);
            }
            "for_in_statement" => {
                return is_field(parent, "left", node).then_some(DefinitionKind::Variable);
            }
            "catch_clause" => {
                return is_field(parent, "parameter", node).then_some(DefinitionKind::Variable);
            }
            _ => return None,
        }
    }
}
