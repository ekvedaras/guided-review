use std::collections::HashMap;
use std::fmt;

use crate::code_source::CodeSource;
use crate::language::Language;
use crate::pieces::{ChangedFile, Piece, PieceId, PieceKind, PieceStatus};
use crate::pull_request::Sha;

/// For every test piece of the PR, the changed pieces it probably tests, best first.
/// Candidates are listed, never picked: the tour writer decides.
pub struct SubjectCandidates(HashMap<String, Subjects>);

impl SubjectCandidates {
    pub fn find(source: &dyn CodeSource, head: &Sha, files: &[ChangedFile]) -> Self {
        let pool: Vec<(&ChangedFile, &Piece)> = files
            .iter()
            .flat_map(|file| file.pieces.iter().map(move |piece| (file, piece)))
            .filter(|(_, piece)| is_possible_subject(piece))
            .collect();

        let mut found = HashMap::new();
        for file in files {
            let imports = ImportedNames::of(source, head, &file.path);
            for test in file.pieces.iter().filter(|piece| is_live_test(piece)) {
                let candidates = subjects_of_test(&file.path, test, &imports, &pool);
                found.insert(test.id.to_string(), Subjects(candidates));
            }
        }
        Self(found)
    }

    pub fn of(&self, test: &PieceId) -> Option<&Subjects> {
        self.0.get(&test.to_string())
    }
}

const MAX_LISTED: usize = 3;

pub struct Subjects(Vec<Subject>);

struct Subject {
    id: PieceId,
    basis: Basis,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Basis {
    Name,
    Import,
}

impl fmt::Display for Subjects {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0.is_empty() {
            return f.write_str("none found");
        }
        for (index, subject) in self.0.iter().take(MAX_LISTED).enumerate() {
            if index > 0 {
                f.write_str(", ")?;
            }
            let basis = match subject.basis {
                Basis::Name => "name",
                Basis::Import => "import",
            };
            write!(f, "{} ({basis})", subject.id)?;
        }
        match self.0.len().saturating_sub(MAX_LISTED) {
            0 => Ok(()),
            more => write!(f, ", +{more} more"),
        }
    }
}

fn is_live_test(piece: &Piece) -> bool {
    piece.kind == PieceKind::Test && piece.status != PieceStatus::Deleted
}

fn is_possible_subject(piece: &Piece) -> bool {
    piece.status != PieceStatus::Deleted
        && !matches!(
            piece.kind,
            PieceKind::Test
                | PieceKind::Top
                | PieceKind::Imports
                | PieceKind::File
                | PieceKind::Hunk
        )
}

fn subjects_of_test(
    test_path: &str,
    test: &Piece,
    imports: &ImportedNames,
    pool: &[(&ChangedFile, &Piece)],
) -> Vec<Subject> {
    let symbol = test.id.symbol.as_deref().unwrap_or_default();
    let test_name = normalized(symbol.rsplit("::").next().unwrap_or(symbol));
    let stems = stems_of_test(test_path, symbol);
    let candidates = || {
        pool.iter()
            .filter(|(file, _)| file.path != test_path)
            .map(|(file, piece)| (*file, *piece))
    };

    let mut by_name: Vec<(u8, &Piece)> = candidates()
        .filter(|(file, piece)| {
            let names = Names::of(file, piece);
            stems.contains(&names.owner) || stems.contains(&names.file)
        })
        .map(|(_, piece)| {
            let own_name = normalized(own_name(piece));
            let rank = if own_name.len() >= 3 && test_name.contains(&own_name) {
                0
            } else if is_member(piece) {
                2
            } else {
                1
            };
            (rank, piece)
        })
        .collect();
    by_name.sort_by_key(|(rank, _)| *rank);

    let by_import = candidates()
        .filter(|(file, piece)| {
            let names = Names::of(file, piece);
            imports.contain(&names.owner) || imports.contain(&names.file)
        })
        .map(|(_, piece)| piece)
        .filter(|piece| !by_name.iter().any(|(_, named)| named.id == piece.id));

    by_name
        .iter()
        .map(|(_, piece)| Subject {
            id: piece.id.clone(),
            basis: Basis::Name,
        })
        .chain(by_import.map(|piece| Subject {
            id: piece.id.clone(),
            basis: Basis::Import,
        }))
        .collect()
}

/// What the test is named after: `OrderTest` and `order.test.ts` are about `order`;
/// a `describe("Cart")` around the test is about `cart` too.
fn stems_of_test(path: &str, symbol: &str) -> Vec<String> {
    let mut stems = vec![strip_test_suffix(normalized(file_stem(path)))];
    if let Some((outer, _)) = symbol.split_once("::") {
        stems.push(strip_test_suffix(normalized(outer)));
    }
    stems
}

fn strip_test_suffix(name: String) -> String {
    name.strip_suffix("test")
        .or_else(|| name.strip_suffix("spec"))
        .map_or(name.clone(), str::to_string)
}

struct Names {
    owner: String,
    file: String,
}

impl Names {
    fn of(file: &ChangedFile, piece: &Piece) -> Self {
        let symbol = piece.id.symbol.as_deref().unwrap_or_default();
        Self {
            owner: normalized(symbol.split("::").next().unwrap_or(symbol)),
            file: normalized(file_stem(&file.path)),
        }
    }
}

fn is_member(piece: &Piece) -> bool {
    piece
        .id
        .symbol
        .as_deref()
        .is_some_and(|symbol| symbol.contains("::"))
}

fn own_name(piece: &Piece) -> &str {
    let symbol = piece.id.symbol.as_deref().unwrap_or_default();
    symbol.rsplit("::").next().unwrap_or(symbol)
}

/// `app/Models/Order.php` and `src/cart.test.ts` become `Order` and `cart`.
fn file_stem(path: &str) -> &str {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.split('.').next().unwrap_or(name)
}

fn normalized(name: &str) -> String {
    name.chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// Names a test file imports, as `normalized` class, function and module names.
struct ImportedNames(Vec<String>);

impl ImportedNames {
    fn of(source: &dyn CodeSource, head: &Sha, path: &str) -> Self {
        let Ok(text) = source.file_at(head, path) else {
            return Self(Vec::new());
        };
        let names = match Language::for_path(path) {
            Some(Language::Php) => php_imports(&text),
            Some(Language::TypeScript | Language::Tsx) | None => typescript_imports(&text),
        };
        Self(names.iter().map(|name| normalized(name)).collect())
    }

    fn contain(&self, name: &str) -> bool {
        !name.is_empty() && self.0.iter().any(|imported| imported == name)
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
            names.push(imported.rsplit('\\').next().unwrap_or(imported).to_string());
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
        names.push(file_stem(module).to_string());
        let clause = clause.trim_start().trim_start_matches("import").trim();
        let clause = clause.strip_prefix("type ").unwrap_or(clause);
        for part in clause.split([',', '{', '}']) {
            let imported = part.split(" as ").next().unwrap_or(part).trim();
            if !imported.is_empty() && !imported.starts_with('*') {
                names.push(imported.to_string());
            }
        }
        statement.clear();
    }
    names
}
