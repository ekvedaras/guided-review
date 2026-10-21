use std::fmt;

use sha2::{Digest, Sha256};

use crate::code_source::{CodeSource, FileDiff, FileStatus, Line};
use crate::php::{Symbol, Symbols};
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
            let symbol_pieces = if diff.path().ends_with(".php") && !diff.binary {
                cut_php(source, base, head, &diff)?
            } else {
                Vec::new()
            };
            let pieces = if symbol_pieces.is_empty() {
                vec![whole_file_piece(&diff)]
            } else {
                symbol_pieces
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
        status: diff.status().into(),
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
    let base_symbols = symbols_at(source, base, diff.old_path.as_deref())?;
    let head_symbols = symbols_at(source, head, diff.new_path.as_deref())?;

    let mut changes: Vec<SymbolChange> = Vec::new();
    for line in &diff.removed {
        let symbol = base_symbols.innermost_at(line.number);
        SymbolChange::entry_for(&mut changes, symbol)
            .removed
            .push(line);
    }
    for line in &diff.added {
        let symbol = head_symbols.innermost_at(line.number);
        SymbolChange::entry_for(&mut changes, symbol)
            .added
            .push(line);
    }

    let mut pieces: Vec<(usize, Piece)> = changes
        .into_iter()
        .map(|change| {
            let in_base = base_symbols.named(&change.symbol_path);
            let in_head = head_symbols.named(&change.symbol_path);
            let (kind, status) = match (in_base, in_head) {
                (None, Some(symbol)) => (symbol.kind, PieceStatus::Added),
                (Some(symbol), None) => (symbol.kind, PieceStatus::Deleted),
                (Some(_), Some(symbol)) => (symbol.kind, PieceStatus::Modified),
                (None, None) => (PieceKind::Top, diff.status().into()),
            };
            let position = in_head
                .or(in_base)
                .map_or_else(|| change.first_line(), |symbol| symbol.first_line);
            let piece = Piece {
                id: PieceId {
                    path: diff.path().to_string(),
                    symbol: Some(change.symbol_path),
                },
                kind,
                status,
                added: change.added.len(),
                removed: change.removed.len(),
                signature: in_head.or(in_base).map(|symbol| symbol.signature.clone()),
                diff_hash: DiffHash::of(&change.removed, &change.added),
            };
            (position, piece)
        })
        .collect();
    pieces.sort_by_key(|(position, _)| *position);
    Ok(pieces.into_iter().map(|(_, piece)| piece).collect())
}

fn symbols_at(source: &dyn CodeSource, sha: &Sha, path: Option<&str>) -> anyhow::Result<Symbols> {
    Ok(match path {
        Some(path) => Symbols::find(&source.file_at(sha, path)?),
        None => Symbols::find(""),
    })
}

struct SymbolChange<'a> {
    symbol_path: String,
    removed: Vec<&'a Line>,
    added: Vec<&'a Line>,
}

impl<'a> SymbolChange<'a> {
    fn entry_for<'c>(changes: &'c mut Vec<Self>, symbol: Option<&Symbol>) -> &'c mut Self {
        let symbol_path = symbol.map_or(TOP, |symbol| symbol.path.as_str());
        let index = match changes
            .iter()
            .position(|change| change.symbol_path == symbol_path)
        {
            Some(index) => index,
            None => {
                changes.push(Self {
                    symbol_path: symbol_path.to_string(),
                    removed: Vec::new(),
                    added: Vec::new(),
                });
                changes.len() - 1
            }
        };
        &mut changes[index]
    }

    fn first_line(&self) -> usize {
        self.added
            .iter()
            .chain(&self.removed)
            .map(|line| line.number)
            .min()
            .unwrap_or_default()
    }
}

impl From<FileStatus> for PieceStatus {
    fn from(file: FileStatus) -> Self {
        match file {
            FileStatus::Added => Self::Added,
            FileStatus::Deleted => Self::Deleted,
            FileStatus::Modified | FileStatus::Renamed => Self::Modified,
        }
    }
}
