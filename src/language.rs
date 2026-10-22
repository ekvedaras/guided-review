use std::path::Path;

use tree_sitter::Node;

/// The one place that decides which files are cut into symbol pieces and with which grammar and query.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    Php,
    TypeScript,
    Tsx,
}

impl Language {
    pub fn for_path(path: &str) -> Option<Self> {
        match Path::new(path).extension()?.to_str()? {
            "php" => Some(Self::Php),
            "ts" | "mts" | "cts" => Some(Self::TypeScript),
            "tsx" => Some(Self::Tsx),
            _ => None,
        }
    }

    pub fn grammar(self) -> tree_sitter::Language {
        match self {
            Self::Php => tree_sitter_php::LANGUAGE_PHP.into(),
            Self::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            Self::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
        }
    }

    pub fn symbol_query(self) -> &'static str {
        match self {
            Self::Php => include_str!("../queries/php/symbols.scm"),
            Self::TypeScript | Self::Tsx => include_str!("../queries/typescript/symbols.scm"),
        }
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
        }
    }
}
