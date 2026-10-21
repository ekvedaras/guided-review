use std::fmt;

use sha2::{Digest, Sha256};

use crate::code_source::{CodeSource, FileDiff, FileStatus, Line};
use crate::php::Symbols;
use crate::pull_request::Sha;

/// `app/Foo.php#Foo::handle`, or just `app/Foo.php` for a whole-file piece.
/// Symbol paths never hold `#`, `@` or `/`, so a sub-part can later be appended
/// after one of them without changing the ids old tour files point at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PieceId {
    pub path: String,
    pub symbol: Option<String>,
}

impl fmt::Display for PieceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.symbol {
            Some(symbol) => write!(f, "{}#{symbol}", self.path),
            None => f.write_str(&self.path),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PieceKind {
    Class,
    Interface,
    Trait,
    Enum,
    Method,
    Function,
    Top,
    File,
}

impl fmt::Display for PieceKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Class => "class",
            Self::Interface => "interface",
            Self::Trait => "trait",
            Self::Enum => "enum",
            Self::Method => "method",
            Self::Function => "function",
            Self::Top => "top",
            Self::File => "file",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PieceStatus {
    Added,
    Modified,
    Deleted,
}

impl fmt::Display for PieceStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Added => "added",
            Self::Modified => "modified",
            Self::Deleted => "deleted",
        })
    }
}

/// Hash of the piece's old-side and new-side text. Re-pin compares it to tell same from changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffHash(String);

impl DiffHash {
    fn of(removed: &[&Line], added: &[&Line]) -> Self {
        let mut hasher = Sha256::new();
        for line in removed {
            hasher.update(format!("-{}\n", line.text));
        }
        for line in added {
            hasher.update(format!("+{}\n", line.text));
        }
        Self(
            hasher
                .finalize()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect(),
        )
    }
}

#[derive(Debug)]
pub struct Piece {
    pub id: PieceId,
    pub kind: PieceKind,
    pub status: PieceStatus,
    pub added: usize,
    pub removed: usize,
    pub signature: Option<String>,
    pub diff_hash: DiffHash,
}

#[derive(Debug)]
pub struct ChangedFile {
    pub path: String,
    pub status: FileStatus,
    pub added: usize,
    pub removed: usize,
    pub pieces: Vec<Piece>,
}

pub fn cut(source: &dyn CodeSource, base: &Sha, head: &Sha) -> anyhow::Result<Vec<ChangedFile>> {
    source
        .diff(base, head)?
        .into_iter()
        .map(|diff| {
            let pieces = if diff.path().ends_with(".php") && !diff.binary {
                cut_php(source, base, head, &diff)?
            } else {
                vec![whole_file_piece(&diff)]
            };
            Ok(ChangedFile {
                path: diff.path().to_string(),
                status: diff.status(),
                added: diff.added.len(),
                removed: diff.removed.len(),
                pieces,
            })
        })
        .collect()
}

fn whole_file_piece(diff: &FileDiff) -> Piece {
    let removed: Vec<&Line> = diff.removed.iter().collect();
    let added: Vec<&Line> = diff.added.iter().collect();
    Piece {
        id: PieceId {
            path: diff.path().to_string(),
            symbol: None,
        },
        kind: PieceKind::File,
        status: piece_status_of(diff.status()),
        added: added.len(),
        removed: removed.len(),
        signature: None,
        diff_hash: DiffHash::of(&removed, &added),
    }
}

const TOP: &str = "(top)";

fn cut_php(
    source: &dyn CodeSource,
    base: &Sha,
    head: &Sha,
    diff: &FileDiff,
) -> anyhow::Result<Vec<Piece>> {
    let base_symbols = match &diff.old_path {
        Some(path) => Symbols::find(&source.file_at(base, path)?),
        None => Symbols::find(""),
    };
    let head_symbols = match &diff.new_path {
        Some(path) => Symbols::find(&source.file_at(head, path)?),
        None => Symbols::find(""),
    };

    let mut changes: Vec<SymbolChange> = Vec::new();
    for line in &diff.removed {
        let symbol = base_symbols.innermost_at(line.number);
        let anchor = symbol.map_or(line.number, |symbol| symbol.first_line);
        let path = symbol.map_or(TOP, |symbol| symbol.path.as_str());
        SymbolChange::for_path(&mut changes, path, anchor)
            .removed
            .push(line);
    }
    for line in &diff.added {
        let symbol = head_symbols.innermost_at(line.number);
        let anchor = symbol.map_or(line.number, |symbol| symbol.first_line);
        let path = symbol.map_or(TOP, |symbol| symbol.path.as_str());
        let change = SymbolChange::for_path(&mut changes, path, anchor);
        change.added.push(line);
        change.head_anchor.get_or_insert(anchor);
    }
    changes.sort_by_key(|change| change.head_anchor.unwrap_or(change.base_anchor));

    Ok(changes
        .into_iter()
        .map(|change| {
            let in_base = base_symbols.named(&change.path);
            let in_head = head_symbols.named(&change.path);
            let (kind, status) = match (in_base, in_head) {
                (_, Some(symbol)) if in_base.is_none() => (symbol.kind, PieceStatus::Added),
                (Some(symbol), None) => (symbol.kind, PieceStatus::Deleted),
                (_, Some(symbol)) => (symbol.kind, PieceStatus::Modified),
                (None, None) => (PieceKind::Top, piece_status_of(diff.status())),
            };
            Piece {
                id: PieceId {
                    path: diff.path().to_string(),
                    symbol: Some(change.path.clone()),
                },
                kind,
                status,
                added: change.added.len(),
                removed: change.removed.len(),
                signature: in_head.or(in_base).map(|symbol| symbol.signature.clone()),
                diff_hash: DiffHash::of(&change.removed, &change.added),
            }
        })
        .collect())
}

struct SymbolChange<'a> {
    path: String,
    base_anchor: usize,
    head_anchor: Option<usize>,
    removed: Vec<&'a Line>,
    added: Vec<&'a Line>,
}

impl<'a> SymbolChange<'a> {
    fn for_path<'c>(changes: &'c mut Vec<Self>, path: &str, anchor: usize) -> &'c mut Self {
        let index = match changes.iter().position(|change| change.path == path) {
            Some(index) => index,
            None => {
                changes.push(Self {
                    path: path.to_string(),
                    base_anchor: anchor,
                    head_anchor: None,
                    removed: Vec::new(),
                    added: Vec::new(),
                });
                changes.len() - 1
            }
        };
        &mut changes[index]
    }
}

fn piece_status_of(file: FileStatus) -> PieceStatus {
    match file {
        FileStatus::Added => PieceStatus::Added,
        FileStatus::Deleted => PieceStatus::Deleted,
        FileStatus::Modified | FileStatus::Renamed => PieceStatus::Modified,
    }
}
