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
        let (head, doc) = match markdown.split_once("\n---") {
            Some((head, doc)) => (head, Some(doc)),
            None => (markdown.as_str(), None),
        };
        Self {
            signature: last_code_block(head),
            doc: doc.and_then(summary),
        }
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
        } else if let Some(block) = &mut open {
            block.push(line.trim());
        }
    }
    blocks.pop().filter(|block| !block.is_empty())
}

/// The first paragraph, cut where raw tags (`@param`) or a section heading start.
fn summary(doc: &str) -> Option<String> {
    let mut words: Vec<&str> = Vec::new();
    for line in doc.lines().map(str::trim) {
        if line.starts_with('@') || line.starts_with('#') || line.starts_with("```") {
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
    let mut tokens = signature
        .split(|c: char| c.is_whitespace() || c == '(')
        .filter(|token| !token.is_empty())
        .skip_while(|token| {
            matches!(
                *token,
                "pub" | "async" | "unsafe" | "extern" | "default" | "export" | "declare"
            ) || token.starts_with("crate)")
                || token.starts_with("super)")
                || token.starts_with('"')
        });
    match tokens.next() {
        Some("fn" | "function") => {
            if signature.contains("self") {
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
        Some("class" | "abstract") => DefinitionKind::Class,
        Some("interface") => DefinitionKind::Interface,
        Some("method" | "constructor") => DefinitionKind::Method,
        _ => DefinitionKind::Variable,
    }
}
