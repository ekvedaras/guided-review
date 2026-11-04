use std::collections::HashMap;
use std::ops::Range;
use std::sync::LazyLock;

use tree_sitter_highlight::{HighlightConfiguration, HighlightEvent, Highlighter};

use crate::code_source::{CodeSource, FileAt, FileStatus};
use crate::file_path::FilePath;
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
    const ALL: [Self; 24] = [
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

/// A coloured stretch of one line: byte offsets into that line's text. Kept for every line of every
/// changed file, so the offsets are `u32`: 12 bytes a span instead of 24.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Span {
    start: u32,
    end: u32,
    pub capture: Capture,
}

impl Span {
    /// None for a line longer than 4 GiB, which stays plain.
    fn new(range: Range<usize>, capture: Capture) -> Option<Self> {
        Some(Self {
            start: u32::try_from(range.start).ok()?,
            end: u32::try_from(range.end).ok()?,
            capture,
        })
    }

    pub fn range(&self) -> Range<usize> {
        let at = |offset: u32| usize::try_from(offset).unwrap_or(usize::MAX);
        at(self.start)..at(self.end)
    }
}

/// The spans of every line of one file in one allocation, instead of one `Vec` per line.
#[derive(Debug, Default)]
struct FileSpans {
    spans: Box<[Span]>,
    /// Where each line's spans end in `spans`.
    line_ends: Box<[u32]>,
}

impl FileSpans {
    fn of(lines: Vec<Vec<Span>>) -> Self {
        let mut line_ends = Vec::with_capacity(lines.len());
        let mut spans = Vec::with_capacity(lines.iter().map(Vec::len).sum());
        for line in lines {
            spans.extend(line);
            line_ends.push(u32::try_from(spans.len()).unwrap_or(u32::MAX));
        }
        Self {
            spans: spans.into(),
            line_ends: line_ends.into(),
        }
    }

    /// `index` counts from 0.
    fn line(&self, index: usize) -> &[Span] {
        let end_of = |line: usize| {
            self.line_ends
                .get(line)
                .and_then(|&end| usize::try_from(end).ok())
        };
        let start = match index.checked_sub(1) {
            Some(previous) => end_of(previous),
            None => Some(0),
        };
        match (start, end_of(index)) {
            (Some(start), Some(end)) => self.spans.get(start..end).unwrap_or(&[]),
            _ => &[],
        }
    }
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

    /// Compiling a language's queries takes milliseconds, so each is compiled once, on first use, and shared.
    fn configuration(self) -> Option<&'static HighlightConfiguration> {
        static PHP: LazyLock<Option<HighlightConfiguration>> =
            LazyLock::new(|| HighlightLanguage::Php.compile());
        static TYPESCRIPT: LazyLock<Option<HighlightConfiguration>> =
            LazyLock::new(|| HighlightLanguage::TypeScript.compile());
        static TSX: LazyLock<Option<HighlightConfiguration>> =
            LazyLock::new(|| HighlightLanguage::Tsx.compile());
        static JAVASCRIPT: LazyLock<Option<HighlightConfiguration>> =
            LazyLock::new(|| HighlightLanguage::JavaScript.compile());
        static JSON: LazyLock<Option<HighlightConfiguration>> =
            LazyLock::new(|| HighlightLanguage::Json.compile());
        static CSS: LazyLock<Option<HighlightConfiguration>> =
            LazyLock::new(|| HighlightLanguage::Css.compile());
        static HTML: LazyLock<Option<HighlightConfiguration>> =
            LazyLock::new(|| HighlightLanguage::Html.compile());
        static RUST: LazyLock<Option<HighlightConfiguration>> =
            LazyLock::new(|| HighlightLanguage::Rust.compile());
        static BASH: LazyLock<Option<HighlightConfiguration>> =
            LazyLock::new(|| HighlightLanguage::Bash.compile());
        static YAML: LazyLock<Option<HighlightConfiguration>> =
            LazyLock::new(|| HighlightLanguage::Yaml.compile());
        let configuration = match self {
            Self::Php => &PHP,
            Self::TypeScript => &TYPESCRIPT,
            Self::Tsx => &TSX,
            Self::JavaScript => &JAVASCRIPT,
            Self::Json => &JSON,
            Self::Css => &CSS,
            Self::Html => &HTML,
            Self::Rust => &RUST,
            Self::Bash => &BASH,
            Self::Yaml => &YAML,
        };
        configuration.as_ref()
    }

    /// None when the built-in query does not compile; the file then stays plain, and stderr says which language.
    fn compile(self) -> Option<HighlightConfiguration> {
        // The later pattern wins, so a grammar's own additions come after the JavaScript base they extend.
        let (name, language, highlights, injections, locals) = match self {
            Self::Php => (
                "php",
                tree_sitter_php::LANGUAGE_PHP.into(),
                tree_sitter_php::HIGHLIGHTS_QUERY.to_owned(),
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
                tree_sitter_json::HIGHLIGHTS_QUERY.to_owned(),
                "",
                "",
            ),
            Self::Css => (
                "css",
                tree_sitter_css::LANGUAGE.into(),
                tree_sitter_css::HIGHLIGHTS_QUERY.to_owned(),
                "",
                "",
            ),
            Self::Html => (
                "html",
                tree_sitter_html::LANGUAGE.into(),
                tree_sitter_html::HIGHLIGHTS_QUERY.to_owned(),
                "",
                "",
            ),
            Self::Rust => (
                "rust",
                tree_sitter_rust::LANGUAGE.into(),
                tree_sitter_rust::HIGHLIGHTS_QUERY.to_owned(),
                "",
                "",
            ),
            Self::Bash => (
                "bash",
                tree_sitter_bash::LANGUAGE.into(),
                tree_sitter_bash::HIGHLIGHT_QUERY.to_owned(),
                "",
                "",
            ),
            Self::Yaml => (
                "yaml",
                tree_sitter_yaml::LANGUAGE.into(),
                tree_sitter_yaml::HIGHLIGHTS_QUERY.to_owned(),
                "",
                "",
            ),
        };
        let mut configuration =
            HighlightConfiguration::new(language, name, &highlights, injections, locals)
                .inspect_err(|error| eprintln!("the {name} highlight query is broken: {error}"))
                .ok()?;
        let names: Vec<&str> = Capture::ALL
            .iter()
            .map(|capture| capture.query_name())
            .collect();
        configuration.configure(&names);
        Some(configuration)
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
                let Some(capture) = Capture::ALL.get(capture).copied() else {
                    continue;
                };
                let mut at = start;
                while at < end {
                    let line = line_starts.partition_point(|&line_start| line_start <= at) - 1;
                    let line_end = line_starts
                        .get(line + 1)
                        .map_or(source.len(), |&next| next - 1);
                    let stop = end.min(line_end);
                    let (Some(&line_start), Some(spans)) =
                        (line_starts.get(line), lines.get_mut(line))
                    else {
                        break;
                    };
                    if stop > at
                        && let Some(span) = Span::new(at - line_start..stop - line_start, capture)
                    {
                        spans.push(span);
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
    path: FilePath,
    /// The name the review shows it under.
    shown_path: FilePath,
}

/// What to highlight, owned so it can move to a background thread.
#[derive(Debug, Clone)]
pub struct HighlightRequest {
    base: Sha,
    head: Sha,
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
                    path: file.old_path.as_ref().unwrap_or(&file.path).clone(),
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
        targets.retain(|target| HighlightLanguage::for_path(target.path.as_str()).is_some());
        Self {
            base: base.clone(),
            head: head.clone(),
            targets,
        }
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
        let mut base = HashMap::new();
        let mut head = HashMap::new();
        for target in &self.targets {
            let Some(language) = HighlightLanguage::for_path(target.path.as_str()) else {
                continue;
            };
            let Ok(text) = source.file_at(&target.sha, &target.path) else {
                continue;
            };
            let Some(configuration) = language.configuration() else {
                continue;
            };
            let side = match target.side {
                Side::Base => &mut base,
                Side::Head => &mut head,
            };
            side.insert(
                target.shown_path.clone(),
                FileSpans::of(highlight_file(configuration, &text)),
            );
        }
        Highlights {
            base_sha: self.base.clone(),
            head_sha: self.head.clone(),
            base,
            head,
        }
    }
}

#[derive(Debug, Default)]
pub struct Highlights {
    base_sha: Sha,
    head_sha: Sha,
    /// Spans per line, by the path the review shows the file under; one map per side, so a lookup borrows the path.
    base: HashMap<FilePath, FileSpans>,
    head: HashMap<FilePath, FileSpans>,
}

impl Highlights {
    pub(super) fn is_of(&self, base: &Sha, head: &Sha) -> bool {
        self.base_sha == *base && self.head_sha == *head
    }

    pub(super) fn spans(&self, side: Side, path: &str, line: usize) -> &[Span] {
        let files = match side {
            Side::Base => &self.base,
            Side::Head => &self.head,
        };
        files
            .get(path)
            .map_or(&[], |lines| lines.line(line.wrapping_sub(1)))
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
        text.to_owned()
    };
    let Some(configuration) = language.configuration() else {
        return vec![Vec::new(); line_count];
    };
    let mut lines = highlight_file(configuration, &source);
    if needs_php_tag {
        lines.remove(0);
    }
    lines.resize(line_count, Vec::new());
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span(range: Range<usize>) -> Span {
        Span::new(range, Capture::Keyword).unwrap()
    }

    #[test]
    fn a_span_is_twelve_bytes() {
        assert_eq!(size_of::<Span>(), 12);
    }

    #[test]
    fn each_line_gets_its_own_spans_back_from_the_one_list() {
        let file = FileSpans::of(vec![vec![span(0..2)], vec![], vec![span(1..3), span(4..5)]]);

        assert_eq!(file.line(0), [span(0..2)]);
        assert_eq!(file.line(1), [] as [Span; 0]);
        assert_eq!(file.line(2), [span(1..3), span(4..5)]);
        assert_eq!(file.line(3), [] as [Span; 0]);
        assert_eq!(file.line(usize::MAX), [] as [Span; 0]);
    }
}
