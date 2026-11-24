//! What a server's hover says: the signature and the summary of the doc comment, read from the
//! markdown it sends.

use serde_json::Value;

use crate::navigation::DefinitionKind;

pub(super) struct Hovered {
    pub signature: Option<String>,
    pub doc: Option<String>,
}

impl Hovered {
    pub(super) fn of(hover: &Value) -> Self {
        let markdown = markdown_of(&hover["contents"]);
        // rust-analyzer puts the doc under a rule; Intelephense puts its summary between a bold title
        // and the code, and the tags after the code.
        let (head, doc) = match markdown.split_once("\n---") {
            Some((head, doc)) => (head, summary(doc)),
            None => (markdown.as_str(), summary(&prose_of(&markdown))),
        };
        // tsserver describes an imported name as the import, not as what it declares, so the
        // definition's own line says more.
        let signature = last_code_block(head).filter(|signature| !signature.starts_with("(alias)"));
        Self { signature, doc }
    }
}

fn markdown_of(contents: &Value) -> String {
    match contents {
        Value::String(text) => text.clone(),
        Value::Array(parts) => parts
            .iter()
            .map(markdown_of)
            .collect::<Vec<_>>()
            .join("\n\n"),
        Value::Object(_) => contents["value"]
            .as_str()
            .map_or_else(String::new, |value| match contents["language"].as_str() {
                Some(language) => format!("```{language}\n{value}\n```"),
                None => value.to_owned(),
            }),
        _ => String::new(),
    }
}

fn last_code_block(markdown: &str) -> Option<String> {
    let mut blocks = Vec::new();
    let mut open: Option<Vec<&str>> = None;
    for line in markdown.lines() {
        if line.trim_start().starts_with("```") {
            match open.take() {
                Some(block) => blocks.push(block.join(" ")),
                None => open = Some(Vec::new()),
            }
        } else if let Some(block) = &mut open
            && line.trim() != "<?php"
        {
            block.push(line.trim());
        }
    }
    blocks.pop().filter(|block| !block.is_empty())
}

/// Intelephense's first line: `__App\\Billing::schedule__`.
fn is_title(line: &str) -> bool {
    line.len() > 4 && line.starts_with("__") && line.ends_with("__")
}

/// The lines outside code blocks, without a bold title line.
fn prose_of(markdown: &str) -> String {
    let mut in_code = false;
    let mut prose = Vec::new();
    for line in markdown.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            in_code = !in_code;
        } else if !in_code && !is_title(trimmed) {
            prose.push(line);
        }
    }
    prose.join("\n")
}

/// The first paragraph, cut where raw tags (`@param`) or a section heading start.
fn summary(doc: &str) -> Option<String> {
    let mut words: Vec<&str> = Vec::new();
    for line in doc.lines().map(str::trim) {
        if line.starts_with('@')
            || line.starts_with("_@")
            || line.starts_with('#')
            || line.starts_with("```")
        {
            break;
        }
        if line.is_empty() {
            if words.is_empty() {
                continue;
            }
            break;
        }
        words.push(line);
    }
    (!words.is_empty()).then(|| words.join(" "))
}

/// What a declaration is, read from the start of its signature.
pub(super) fn kind_of(signature: &str) -> DefinitionKind {
    // tsserver names the kind first in brackets: `(method) Circle.area(): number`.
    if let Some(label) = signature
        .strip_prefix('(')
        .and_then(|rest| rest.split_once(')'))
        .map(|(label, _)| label)
    {
        return match label {
            "method" => DefinitionKind::Method,
            "function" | "local function" => DefinitionKind::Function,
            "parameter" => DefinitionKind::Parameter,
            "enum member" => DefinitionKind::Constant,
            _ => DefinitionKind::Variable,
        };
    }
    let mut in_a_class = false;
    let mut tokens = signature
        .split(|c: char| c.is_whitespace() || c == '(')
        .filter(|token| !token.is_empty())
        .skip_while(|token| {
            if matches!(*token, "public" | "protected" | "private") {
                in_a_class = true;
            }
            matches!(
                *token,
                "pub"
                    | "async"
                    | "unsafe"
                    | "extern"
                    | "default"
                    | "export"
                    | "declare"
                    | "public"
                    | "protected"
                    | "private"
                    | "static"
                    | "final"
                    | "abstract"
                    | "readonly"
            ) || token.starts_with("crate)")
                || token.starts_with("super)")
                || token.starts_with('"')
        });
    match tokens.next() {
        Some("fn" | "function") => {
            if in_a_class || signature.contains("self") {
                DefinitionKind::Method
            } else {
                DefinitionKind::Function
            }
        }
        Some("struct") => DefinitionKind::Struct,
        Some("enum") => DefinitionKind::Enum,
        Some("trait") => DefinitionKind::Trait,
        Some("type") => DefinitionKind::Type,
        Some("mod" | "namespace") => DefinitionKind::Module,
        Some("macro_rules!" | "macro") => DefinitionKind::Macro,
        Some("const" | "static") => DefinitionKind::Constant,
        Some("class") => DefinitionKind::Class,
        Some("interface") => DefinitionKind::Interface,
        Some("method" | "constructor") => DefinitionKind::Method,
        _ => DefinitionKind::Variable,
    }
}
