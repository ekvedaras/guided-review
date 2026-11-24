use std::path::Path;

use tree_sitter::Node;

use crate::pieces::PieceKind;

/// The one place that decides which source files are parsed, with which grammar and queries,
/// and which of them are cut into symbol pieces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    Php,
    TypeScript,
    Tsx,
    Rust,
}

const EXTENSIONS: [(&str, Language); 6] = [
    ("php", Language::Php),
    ("ts", Language::TypeScript),
    ("mts", Language::TypeScript),
    ("cts", Language::TypeScript),
    ("tsx", Language::Tsx),
    ("rs", Language::Rust),
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

    /// Rust is parsed to find definitions and usages, but its files stay one piece each until
    /// a change of its own teaches the outline Rust's symbols.
    pub fn cuts_into_pieces(self) -> bool {
        match self {
            Self::Php | Self::TypeScript | Self::Tsx => true,
            Self::Rust => false,
        }
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
        }
    }

    /// `None` for a language that is not cut into pieces.
    pub fn symbol_query(self) -> Option<&'static str> {
        match self {
            Self::Php => Some(include_str!("../queries/php/symbols.scm")),
            Self::TypeScript | Self::Tsx => Some(include_str!("../queries/typescript/symbols.scm")),
            Self::Rust => None,
        }
    }

    /// Definitions and references, in tree-sitter's tags convention.
    pub fn tags_query(self) -> &'static str {
        match self {
            Self::Php => include_str!("../queries/php/tags.scm"),
            Self::TypeScript | Self::Tsx => include_str!("../queries/typescript/tags.scm"),
            Self::Rust => include_str!("../queries/rust/tags.scm"),
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
            Self::Rust => node.kind() == "use_declaration",
        }
    }

    /// Names a file imports: `use App\{Cart, Item as Line};` gives `Cart` and `Item`.
    pub fn imported_names(self, text: &str) -> Vec<String> {
        match self {
            Self::Php => php_imports(text),
            Self::TypeScript | Self::Tsx => typescript_imports(text),
            Self::Rust => rust_imports(text),
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
            Self::TypeScript | Self::Tsx | Self::Rust => false,
        }
    }
}

/// `use App\Models\Order;`, `use App\{Cart, Item as Line};`
fn php_imports(text: &str) -> Vec<String> {
    let mut names = Vec::new();
    for statement in text
        .lines()
        .filter_map(|line| line.trim().strip_prefix("use "))
    {
        let statement = statement.trim_end_matches(';');
        let statement = statement
            .strip_prefix("function ")
            .or_else(|| statement.strip_prefix("const "))
            .unwrap_or(statement);
        let items = match statement.split_once('{') {
            Some((_, group)) => group.trim_end_matches('}').split(',').collect(),
            None => vec![statement],
        };
        for item in items {
            let imported = item.split(" as ").next().unwrap_or(item).trim();
            names.push(imported.rsplit('\\').next().unwrap_or(imported).to_owned());
        }
    }
    names
}

/// `import { Cart as C } from "../src/cart"` gives `Cart` and `cart`.
fn typescript_imports(text: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut statement = String::new();
    for line in text.lines() {
        if statement.is_empty() && !line.trim_start().starts_with("import ") {
            continue;
        }
        statement.push_str(line);
        statement.push(' ');
        let Some((clause, module)) = statement.rsplit_once(" from ") else {
            if statement.trim_start().starts_with("import \"")
                || statement.trim_start().starts_with("import '")
            {
                statement.clear();
            }
            continue;
        };
        let module = module
            .trim()
            .trim_end_matches(';')
            .trim_matches(['"', '\'']);
        names.push(module_stem(module).to_owned());
        let clause = clause.trim_start().trim_start_matches("import").trim();
        let clause = clause.trim_prefix("type ");
        for part in clause.split([',', '{', '}']) {
            let imported = part.split(" as ").next().unwrap_or(part).trim();
            if !imported.is_empty() && !imported.starts_with('*') {
                names.push(imported.to_owned());
            }
        }
        statement.clear();
    }
    names
}

/// `use crate::cart::{Cart, item::Item as Line};` gives `cart`, `Cart`, `item` and `Item`.
fn rust_imports(text: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut statement = String::new();
    for line in text.lines() {
        let line = line.trim();
        if statement.is_empty() {
            let Some(rest) = line.trim_start_matches("pub ").strip_prefix("use ") else {
                continue;
            };
            statement.push_str(rest);
        } else {
            statement.push(' ');
            statement.push_str(line);
        }
        let Some((paths, _)) = statement.split_once(';') else {
            continue;
        };
        let mut words = paths
            .split(|c: char| c == ':' || c == '{' || c == '}' || c == ',' || c.is_whitespace())
            .filter(|word| !word.is_empty());
        while let Some(word) = words.next() {
            match word {
                "as" => {
                    words.next();
                }
                "crate" | "self" | "super" | "*" => {}
                name => names.push(name.to_owned()),
            }
        }
        statement.clear();
    }
    names
}

fn module_stem(module: &str) -> &str {
    let name = module.rsplit('/').next().unwrap_or(module);
    name.split('.').next().unwrap_or(name)
}
