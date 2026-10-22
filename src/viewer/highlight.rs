use std::collections::HashMap;
use std::ops::Range;

use tree_sitter_highlight::{HighlightConfiguration, HighlightEvent, Highlighter};

use crate::code_source::{CodeSource, FileAt, FileStatus};
use crate::pieces::ChangedFile;
use crate::pull_request::Sha;

use super::step_page::Side;

/// What a piece of code is. Spans keep this and not a colour, so a theme switch needs no new highlighting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Capture {
    Attribute,
    Comment,
    Constant,
    ConstantBuiltin,
    Constructor,
    Function,
    FunctionBuiltin,
    FunctionMethod,
    Keyword,
    Number,
    Operator,
    Property,
    Punctuation,
    PunctuationBracket,
    PunctuationDelimiter,
    PunctuationSpecial,
    String,
    StringSpecial,
    Tag,
    Type,
    TypeBuiltin,
    Variable,
    VariableBuiltin,
    VariableParameter,
}

impl Capture {
    const ALL: [Capture; 24] = [
        Self::Attribute,
        Self::Comment,
        Self::Constant,
        Self::ConstantBuiltin,
        Self::Constructor,
        Self::Function,
        Self::FunctionBuiltin,
        Self::FunctionMethod,
        Self::Keyword,
        Self::Number,
        Self::Operator,
        Self::Property,
        Self::Punctuation,
        Self::PunctuationBracket,
        Self::PunctuationDelimiter,
        Self::PunctuationSpecial,
        Self::String,
        Self::StringSpecial,
        Self::Tag,
        Self::Type,
        Self::TypeBuiltin,
        Self::Variable,
        Self::VariableBuiltin,
        Self::VariableParameter,
    ];

    /// The name the grammars' highlight queries use.
    fn query_name(self) -> &'static str {
        match self {
            Self::Attribute => "attribute",
            Self::Comment => "comment",
            Self::Constant => "constant",
            Self::ConstantBuiltin => "constant.builtin",
            Self::Constructor => "constructor",
            Self::Function => "function",
            Self::FunctionBuiltin => "function.builtin",
            Self::FunctionMethod => "function.method",
            Self::Keyword => "keyword",
            Self::Number => "number",
            Self::Operator => "operator",
            Self::Property => "property",
            Self::Punctuation => "punctuation",
            Self::PunctuationBracket => "punctuation.bracket",
            Self::PunctuationDelimiter => "punctuation.delimiter",
            Self::PunctuationSpecial => "punctuation.special",
            Self::String => "string",
            Self::StringSpecial => "string.special",
            Self::Tag => "tag",
            Self::Type => "type",
            Self::TypeBuiltin => "type.builtin",
            Self::Variable => "variable",
            Self::VariableBuiltin => "variable.builtin",
            Self::VariableParameter => "variable.parameter",
        }
    }
}

/// A coloured stretch of one line: byte offsets into that line's text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub range: Range<usize>,
    pub capture: Capture,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum HighlightLanguage {
    Php,
    TypeScript,
    Tsx,
    JavaScript,
    Json,
    Css,
    Html,
    Rust,
    Bash,
    Yaml,
}

impl HighlightLanguage {
    fn for_path(path: &str) -> Option<Self> {
        let extension = path.rsplit_once('.')?.1;
        match extension {
            "php" => Some(Self::Php),
            "ts" | "mts" | "cts" => Some(Self::TypeScript),
            "tsx" => Some(Self::Tsx),
            "js" | "jsx" | "mjs" | "cjs" => Some(Self::JavaScript),
            "json" => Some(Self::Json),
            "css" => Some(Self::Css),
            "html" | "htm" => Some(Self::Html),
            "rs" => Some(Self::Rust),
            "sh" | "bash" => Some(Self::Bash),
            "yml" | "yaml" => Some(Self::Yaml),
            _ => None,
        }
    }

    /// The names code fences use: a file extension or the language's own name.
    fn for_fence(language: &str) -> Option<Self> {
        let extension = match language.to_ascii_lowercase().as_str() {
            "javascript" => "js",
            "typescript" => "ts",
            "shell" | "zsh" | "console" => "sh",
            "yaml" => "yml",
            "rust" => "rs",
            other => return Self::for_path(&format!("snippet.{other}")),
        };
        Self::for_path(&format!("snippet.{extension}"))
    }

    fn configuration(self) -> HighlightConfiguration {
        // The later pattern wins, so a grammar's own additions come after the JavaScript base they extend.
        let (name, language, highlights, injections, locals) = match self {
            Self::Php => (
                "php",
                tree_sitter_php::LANGUAGE_PHP.into(),
                tree_sitter_php::HIGHLIGHTS_QUERY.to_string(),
                "",
                "",
            ),
            Self::TypeScript => (
                "typescript",
                tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
                [
                    tree_sitter_javascript::HIGHLIGHT_QUERY,
                    tree_sitter_typescript::HIGHLIGHTS_QUERY,
                ]
                .join("\n"),
                "",
                tree_sitter_typescript::LOCALS_QUERY,
            ),
            Self::Tsx => (
                "tsx",
                tree_sitter_typescript::LANGUAGE_TSX.into(),
                [
                    tree_sitter_javascript::HIGHLIGHT_QUERY,
                    tree_sitter_javascript::JSX_HIGHLIGHT_QUERY,
                    tree_sitter_typescript::HIGHLIGHTS_QUERY,
                ]
                .join("\n"),
                "",
                tree_sitter_typescript::LOCALS_QUERY,
            ),
            Self::JavaScript => (
                "javascript",
                tree_sitter_javascript::LANGUAGE.into(),
                [
                    tree_sitter_javascript::HIGHLIGHT_QUERY,
                    tree_sitter_javascript::JSX_HIGHLIGHT_QUERY,
                ]
                .join("\n"),
                "",
                tree_sitter_javascript::LOCALS_QUERY,
            ),
            Self::Json => (
                "json",
                tree_sitter_json::LANGUAGE.into(),
                tree_sitter_json::HIGHLIGHTS_QUERY.to_string(),
                "",
                "",
            ),
            Self::Css => (
                "css",
                tree_sitter_css::LANGUAGE.into(),
                tree_sitter_css::HIGHLIGHTS_QUERY.to_string(),
                "",
                "",
            ),
            Self::Html => (
                "html",
                tree_sitter_html::LANGUAGE.into(),
                tree_sitter_html::HIGHLIGHTS_QUERY.to_string(),
                "",
                "",
            ),
            Self::Rust => (
                "rust",
                tree_sitter_rust::LANGUAGE.into(),
                tree_sitter_rust::HIGHLIGHTS_QUERY.to_string(),
                "",
                "",
            ),
            Self::Bash => (
                "bash",
                tree_sitter_bash::LANGUAGE.into(),
                tree_sitter_bash::HIGHLIGHT_QUERY.to_string(),
                "",
                "",
            ),
            Self::Yaml => (
                "yaml",
                tree_sitter_yaml::LANGUAGE.into(),
                tree_sitter_yaml::HIGHLIGHTS_QUERY.to_string(),
                "",
                "",
            ),
        };
        let mut configuration =
            HighlightConfiguration::new(language, name, &highlights, injections, locals)
                .unwrap_or_else(|error| panic!("the {name} highlight query is broken: {error}"));
        let names: Vec<&str> = Capture::ALL
            .iter()
            .map(|capture| capture.query_name())
            .collect();
        configuration.configure(&names);
        configuration
    }
}

/// Spans per line of a whole file. Whole files, because a PHP fragment is read as HTML
/// and a multi-line string or comment is only coloured right with the lines around it.
fn highlight_file(configuration: &HighlightConfiguration, source: &str) -> Vec<Vec<Span>> {
    let line_starts: Vec<usize> = std::iter::once(0)
        .chain(source.match_indices('\n').map(|(at, _)| at + 1))
        .collect();
    let mut lines: Vec<Vec<Span>> = vec![Vec::new(); line_starts.len()];
    let mut highlighter = Highlighter::new();
    let Ok(events) = highlighter.highlight(configuration, source.as_bytes(), None, None, |_| None)
    else {
        return lines;
    };
    let mut open: Vec<usize> = Vec::new();
    for event in events.map_while(Result::ok) {
        match event {
            HighlightEvent::HighlightStart(highlight) => open.push(highlight.0),
            HighlightEvent::HighlightEnd => {
                open.pop();
            }
            HighlightEvent::Source { start, end } => {
                let Some(&capture) = open.last() else {
                    continue;
                };
                let mut at = start;
                while at < end {
                    let line = line_starts.partition_point(|&line_start| line_start <= at) - 1;
                    let line_end = line_starts
                        .get(line + 1)
                        .map_or(source.len(), |&next| next - 1);
                    let stop = end.min(line_end);
                    if stop > at {
                        lines[line].push(Span {
                            range: at - line_starts[line]..stop - line_starts[line],
                            capture: Capture::ALL[capture],
                        });
                    }
                    at = stop.max(line_end) + 1;
                }
            }
        }
    }
    lines
}

#[derive(Debug, Clone)]
struct Target {
    side: Side,
    sha: Sha,
    /// Where to read the file at `sha`.
    path: String,
    /// The name the review shows it under.
    shown_path: String,
}

/// What to highlight, owned so it can move to a background thread.
#[derive(Debug, Clone)]
pub struct HighlightRequest {
    targets: Vec<Target>,
}

impl HighlightRequest {
    pub(super) fn for_files(files: &[ChangedFile], base: &Sha, head: &Sha) -> Self {
        let mut targets = Vec::new();
        for file in files {
            if file.status != FileStatus::Added {
                targets.push(Target {
                    side: Side::Base,
                    sha: base.clone(),
                    path: file.old_path.clone().unwrap_or_else(|| file.path.clone()),
                    shown_path: file.path.clone(),
                });
            }
            if file.status != FileStatus::Deleted {
                targets.push(Target {
                    side: Side::Head,
                    sha: head.clone(),
                    path: file.path.clone(),
                    shown_path: file.path.clone(),
                });
            }
        }
        targets.retain(|target| HighlightLanguage::for_path(&target.path).is_some());
        Self { targets }
    }

    /// A file that cannot be read stays plain.
    pub fn compute(&self, source: &dyn CodeSource) -> Highlights {
        source.read_ahead(
            &self
                .targets
                .iter()
                .map(|target| FileAt {
                    sha: target.sha.clone(),
                    path: target.path.clone(),
                })
                .collect::<Vec<_>>(),
        );
        let mut configurations: HashMap<HighlightLanguage, HighlightConfiguration> = HashMap::new();
        let mut files = HashMap::new();
        for target in &self.targets {
            let Some(language) = HighlightLanguage::for_path(&target.path) else {
                continue;
            };
            let Ok(text) = source.file_at(&target.sha, &target.path) else {
                continue;
            };
            let configuration = configurations
                .entry(language)
                .or_insert_with(|| language.configuration());
            files.insert(
                (target.side, target.shown_path.clone()),
                highlight_file(configuration, &text),
            );
        }
        Highlights { files }
    }
}

#[derive(Debug, Default)]
pub struct Highlights {
    files: HashMap<(Side, String), Vec<Vec<Span>>>,
}

impl Highlights {
    pub(super) fn spans(&self, side: Side, path: &str, line: usize) -> &[Span] {
        self.files
            .get(&(side, path.to_string()))
            .and_then(|lines| lines.get(line.wrapping_sub(1)))
            .map_or(&[], Vec::as_slice)
    }
}

/// Spans per line of a code fence in a comment. A language we cannot colour gives plain lines.
pub fn highlight_snippet(language: Option<&str>, text: &str) -> Vec<Vec<Span>> {
    let line_count = text.lines().count();
    let Some(language) = language.and_then(HighlightLanguage::for_fence) else {
        return vec![Vec::new(); line_count];
    };
    // A PHP fragment without an opening tag is read as HTML, so give it one and drop its line again.
    let needs_php_tag = language == HighlightLanguage::Php && !text.trim_start().starts_with("<?");
    let source = if needs_php_tag {
        format!("<?php\n{text}")
    } else {
        text.to_string()
    };
    let mut lines = highlight_file(&language.configuration(), &source);
    if needs_php_tag {
        lines.remove(0);
    }
    lines.resize(line_count, Vec::new());
    lines
}
