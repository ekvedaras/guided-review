use std::fmt;

use sha2::{Digest, Sha256};

use crate::code_source::{CodeSource, FileDiff, FileStatus, Line};
use crate::file_rules::{GeneratedFiles, is_cut_per_hunk};
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

impl PieceId {
    /// `#Foo::handle` under its file header in the outline; a whole-file piece keeps its path.
    pub fn within_file(&self) -> String {
        match &self.symbol {
            Some(symbol) => format!("#{symbol}"),
            None => self.path.clone(),
        }
    }
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
    Hunk,
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
            Self::Hunk => "hunk",
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PieceMark {
    Generated,
}

impl fmt::Display for PieceMark {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Generated => "generated",
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
    pub marks: Vec<PieceMark>,
    pub diff_hash: DiffHash,
    pub removed_lines: Vec<Line>,
    pub added_lines: Vec<Line>,
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
    let diffs = source.diff(base, head)?;
    let generated = generated_files(source, head);
    diffs
        .into_iter()
        .map(|diff| {
            let pieces = match cut_by_file_rules(&diff, &generated) {
                Some(pieces) => pieces,
                None => cut_by_symbols(source, base, head, &diff)?,
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

fn cut_by_symbols(
    source: &dyn CodeSource,
    base: &Sha,
    head: &Sha,
    diff: &FileDiff,
) -> anyhow::Result<Vec<Piece>> {
    let symbol_pieces = if diff.path().ends_with(".php") {
        cut_php(source, base, head, diff)?
    } else {
        Vec::new()
    };
    Ok(if symbol_pieces.is_empty() {
        vec![whole_file_piece(diff)]
    } else {
        symbol_pieces
    })
}

/// Every file-level decision, made before any symbol is looked for.
/// `None` means the file is a normal source file and symbols decide its pieces.
fn cut_by_file_rules(diff: &FileDiff, generated: &GeneratedFiles) -> Option<Vec<Piece>> {
    if generated.contains(diff.path()) {
        let mut piece = whole_file_piece(diff);
        piece.marks.push(PieceMark::Generated);
        return Some(vec![piece]);
    }
    if diff.binary || diff.status() == FileStatus::Deleted {
        return Some(vec![whole_file_piece(diff)]);
    }
    if is_cut_per_hunk(diff.path()) && !diff.hunks.is_empty() {
        return Some(hunk_pieces(diff));
    }
    None
}

fn generated_files(source: &dyn CodeSource, head: &Sha) -> GeneratedFiles {
    let attributes_at_head = source.file_at(head, ".gitattributes").unwrap_or_default();
    GeneratedFiles::new(&attributes_at_head)
}

fn hunk_pieces(diff: &FileDiff) -> Vec<Piece> {
    diff.hunks
        .iter()
        .enumerate()
        .map(|(index, hunk)| {
            let removed: Vec<&Line> = hunk.removed.iter().collect();
            let added: Vec<&Line> = hunk.added.iter().collect();
            Piece {
                id: PieceId {
                    path: diff.path().to_string(),
                    symbol: Some(format!("hunk-{}", index + 1)),
                },
                kind: PieceKind::Hunk,
                status: diff.status().into(),
                added: added.len(),
                removed: removed.len(),
                signature: Some(hunk.header.clone()),
                marks: Vec::new(),
                diff_hash: DiffHash::of(&removed, &added),
            }
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
        marks: Vec::new(),
        diff_hash: DiffHash::of(&removed, &added),
        removed_lines: diff.removed.clone(),
        added_lines: diff.added.clone(),
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
                marks: Vec::new(),
                diff_hash: DiffHash::of(&change.removed, &change.added),
                removed_lines: change.removed.iter().copied().cloned().collect(),
                added_lines: change.added.iter().copied().cloned().collect(),
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
