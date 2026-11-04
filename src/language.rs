use std::path::Path;

use tree_sitter::Node;

use crate::pieces::PieceKind;

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

    /// Names a file imports: `use App\{Cart, Item as Line};` gives `Cart` and `Item`.
    pub fn imported_names(self, text: &str) -> Vec<String> {
        match self {
            Self::Php => php_imports(text),
            Self::TypeScript | Self::Tsx => typescript_imports(text),
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
            Self::TypeScript | Self::Tsx => false,
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
        let clause = clause.strip_prefix("type ").unwrap_or(clause);
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

fn module_stem(module: &str) -> &str {
    let name = module.rsplit('/').next().unwrap_or(module);
    name.split('.').next().unwrap_or(name)
}
