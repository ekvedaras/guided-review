use std::path::Path;
use std::sync::OnceLock;

use tree_sitter::{Node, Parser, Query};

use crate::piece_kind::PieceKind;

/// The one place that decides which source files are parsed, with which grammar and queries,
/// and which of them are cut into symbol pieces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    Php,
    TypeScript,
    Tsx,
    Rust,
    Python,
    Go,
}

const EXTENSIONS: [(&str, Language); 9] = [
    ("php", Language::Php),
    ("ts", Language::TypeScript),
    ("mts", Language::TypeScript),
    ("cts", Language::TypeScript),
    ("tsx", Language::Tsx),
    ("rs", Language::Rust),
    ("py", Language::Python),
    ("pyi", Language::Python),
    ("go", Language::Go),
];

impl Language {
    pub fn for_path(path: &str) -> Option<Self> {
        let extension = Path::new(path).extension()?.to_str()?;
        EXTENSIONS
            .iter()
            .find(|(known, _)| *known == extension)
            .map(|(_, language)| *language)
    }

    /// The language of a file that is cut into symbol pieces; `None` keeps it one piece.
    pub fn for_pieces(path: &str) -> Option<Self> {
        Self::for_path(path).filter(|language| language.cuts_into_pieces())
    }

    /// Rust, Python and Go are parsed to find definitions and usages, but their files stay one
    /// piece each until a change of their own teaches the outline their symbols.
    pub fn cuts_into_pieces(self) -> bool {
        self.symbol_query().is_some()
    }

    /// Git pathspecs of every file this module parses: `*.php`, `*.ts`, ...
    pub fn file_globs() -> impl Iterator<Item = String> {
        EXTENSIONS
            .iter()
            .map(|(extension, _)| format!("*.{extension}"))
    }

    pub fn grammar(self) -> tree_sitter::Language {
        match self {
            Self::Php => tree_sitter_php::LANGUAGE_PHP.into(),
            Self::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            Self::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
            Self::Rust => tree_sitter_rust::LANGUAGE.into(),
            Self::Python => tree_sitter_python::LANGUAGE.into(),
            Self::Go => tree_sitter_go::LANGUAGE.into(),
        }
    }

    /// `None` for a language that is not cut into pieces.
    pub fn symbol_query(self) -> Option<&'static str> {
        match self {
            Self::Php => Some(include_str!("../queries/php/symbols.scm")),
            Self::TypeScript | Self::Tsx => Some(include_str!("../queries/typescript/symbols.scm")),
            Self::Rust | Self::Python | Self::Go => None,
        }
    }

    #[expect(
        clippy::expect_used,
        reason = "the grammars are compiled in, so they match tree-sitter"
    )]
    pub fn parser(self) -> Parser {
        let mut parser = Parser::new();
        parser
            .set_language(&self.grammar())
            .expect("grammar matches tree-sitter");
        parser
    }

    /// Definitions and references, in tree-sitter's tags convention, compiled once per language.
    #[expect(
        clippy::expect_used,
        reason = "the tags queries are compiled in, so they match their grammars"
    )]
    pub fn tags_query(self) -> &'static Query {
        static PHP: OnceLock<Query> = OnceLock::new();
        static TYPESCRIPT: OnceLock<Query> = OnceLock::new();
        static TSX: OnceLock<Query> = OnceLock::new();
        static RUST: OnceLock<Query> = OnceLock::new();
        static PYTHON: OnceLock<Query> = OnceLock::new();
        static GO: OnceLock<Query> = OnceLock::new();
        let (compiled, text) = match self {
            Self::Php => (&PHP, include_str!("../queries/php/tags.scm")),
            Self::TypeScript => (&TYPESCRIPT, include_str!("../queries/typescript/tags.scm")),
            Self::Tsx => (&TSX, include_str!("../queries/typescript/tags.scm")),
            Self::Rust => (&RUST, include_str!("../queries/rust/tags.scm")),
            Self::Python => (&PYTHON, include_str!("../queries/python/tags.scm")),
            Self::Go => (&GO, include_str!("../queries/go/tags.scm")),
        };
        compiled.get_or_init(|| Query::new(&self.grammar(), text).expect("valid tags query"))
    }

    /// Whether the grammar spells `word` as one of its own tokens: `fn`, `function`, `const`.
    pub fn is_keyword(self, word: &str) -> bool {
        self.grammar().id_for_node_kind(word, false) != 0
    }

    /// Statements that only bring names into the file: they make up the `imports` piece.
    pub fn is_import(self, node: Node) -> bool {
        match self {
            Self::Php => match node.kind() {
                "namespace_use_declaration" => true,
                "namespace_definition" => node.child_by_field_name("body").is_none(),
                _ => false,
            },
            Self::TypeScript | Self::Tsx => node.kind() == "import_statement",
            Self::Rust => node.kind() == "use_declaration",
            Self::Python => matches!(
                node.kind(),
                "import_statement" | "import_from_statement" | "future_import_statement"
            ),
            Self::Go => node.kind() == "import_declaration",
        }
    }

    /// Whether a symbol of this kind, at this `Class::member` path, is a test.
    pub fn is_test(self, kind: PieceKind, symbol_path: &str) -> bool {
        match self {
            Self::Php => {
                kind == PieceKind::Method
                    && symbol_path
                        .split_once("::")
                        .is_some_and(|(class, _)| class.ends_with("Test"))
            }
            Self::TypeScript | Self::Tsx | Self::Rust | Self::Python | Self::Go => false,
        }
    }
}
