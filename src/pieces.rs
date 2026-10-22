use std::collections::HashMap;
use std::fmt;

use sha2::{Digest, Sha256};

use crate::code_source::{CodeSource, FileDiff, FileStatus, Hunk, Line};
use crate::file_rules::{GeneratedFiles, is_cut_per_hunk};
use crate::image_facts::{self, ImageChange};
use crate::language::Language;
use crate::pull_request::Sha;
use crate::symbols::{Slot, Symbol, Symbols};

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
    TypeAlias,
    Const,
    Component,
    Test,
    Method,
    Function,
    Imports,
    Top,
    File,
    Hunk,
    Image,
}

impl fmt::Display for PieceKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Class => "class",
            Self::Interface => "interface",
            Self::Trait => "trait",
            Self::Enum => "enum",
            Self::TypeAlias => "type",
            Self::Const => "const",
            Self::Component => "component",
            Self::Test => "test",
            Self::Method => "method",
            Self::Function => "function",
            Self::Imports => "imports",
            Self::Top => "top",
            Self::File => "file",
            Self::Hunk => "hunk",
            Self::Image => "image",
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
    Format,
    Moved,
}

impl fmt::Display for PieceMark {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Generated => "generated",
            Self::Format => "format",
            Self::Moved => "moved",
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
    /// Set on image pieces; the sizes are unknown when the bytes are not a readable image.
    pub image: Option<ImageChange>,
    moved_body: Option<String>,
}

#[derive(Debug)]
pub struct ChangedFile {
    pub path: String,
    /// Where the file lived at base when it was renamed.
    pub old_path: Option<String>,
    pub binary: bool,
    pub status: FileStatus,
    pub added: usize,
    pub removed: usize,
    pub pieces: Vec<Piece>,
    /// Every change of the file, whichever piece holds it.
    pub hunks: Vec<Hunk>,
}

pub fn cut(source: &dyn CodeSource, base: &Sha, head: &Sha) -> anyhow::Result<Vec<ChangedFile>> {
    let diffs = source.diff(base, head)?;
    let generated = generated_files(source, head);
    let mut files = diffs
        .into_iter()
        .map(|diff| {
            let pieces = match cut_by_file_rules(source, base, head, &diff, &generated) {
                Some(pieces) => pieces,
                None => cut_by_symbols(source, base, head, &diff)?,
            };
            Ok(ChangedFile {
                path: diff.path().to_string(),
                old_path: diff.old_path.clone().filter(|old| old != diff.path()),
                binary: diff.binary,
                status: diff.status(),
                added: diff.added.len(),
                removed: diff.removed.len(),
                pieces,
                hunks: diff.hunks.clone(),
            })
        })
        .collect::<anyhow::Result<Vec<ChangedFile>>>()?;
    mark_moved_symbols(&mut files);
    Ok(files)
}

fn cut_by_symbols(
    source: &dyn CodeSource,
    base: &Sha,
    head: &Sha,
    diff: &FileDiff,
) -> anyhow::Result<Vec<Piece>> {
    let symbol_pieces = match Language::for_path(diff.path()) {
        Some(language) => cut_symbols(source, base, head, diff, language)?,
        None => Vec::new(),
    };
    Ok(if symbol_pieces.is_empty() {
        vec![whole_file_piece(diff)]
    } else {
        symbol_pieces
    })
}

/// Every file-level decision, made before any symbol is looked for.
/// `None` means the file is a normal source file and symbols decide its pieces.
fn cut_by_file_rules(
    source: &dyn CodeSource,
    base: &Sha,
    head: &Sha,
    diff: &FileDiff,
    generated: &GeneratedFiles,
) -> Option<Vec<Piece>> {
    if generated.contains(diff.path()) {
        let mut piece = whole_file_piece(diff);
        piece.marks.push(PieceMark::Generated);
        return Some(vec![piece]);
    }
    if image_facts::is_image_path(diff.path())
        && (diff.binary || image_facts::is_svg_path(diff.path()))
    {
        return Some(vec![image_piece(source, base, head, diff)]);
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
                removed_lines: hunk.removed.clone(),
                added_lines: hunk.added.clone(),
                image: None,
                moved_body: None,
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
        image: None,
        moved_body: None,
    }
}

fn image_piece(source: &dyn CodeSource, base: &Sha, head: &Sha, diff: &FileDiff) -> Piece {
    let size_at = |sha: &Sha, path: Option<&str>| {
        let path = path?;
        if source.blob_info_at(sha, path).ok()?.size > image_facts::MAX_PREVIEW_BYTES {
            return None;
        }
        let bytes = source.blob_at(sha, path).ok()?;
        image_facts::measure(path, &bytes)
    };
    let mut piece = whole_file_piece(diff);
    piece.kind = PieceKind::Image;
    piece.image = Some(ImageChange {
        base: size_at(base, diff.old_path.as_deref()),
        head: size_at(head, diff.new_path.as_deref()),
    });
    piece
}

fn cut_symbols(
    source: &dyn CodeSource,
    base: &Sha,
    head: &Sha,
    diff: &FileDiff,
    language: Language,
) -> anyhow::Result<Vec<Piece>> {
    let base_symbols = symbols_at(source, base, diff.old_path.as_deref(), language)?;
    let head_symbols = symbols_at(source, head, diff.new_path.as_deref(), language)?;
    let head_path_of_base = base_symbols.pair_with_head(&head_symbols);
    let base_path_of_head: HashMap<&str, &str> = head_path_of_base
        .iter()
        .map(|(base, head)| (head.as_str(), base.as_str()))
        .collect();

    let mut changes: Vec<SymbolChange> = Vec::new();
    for line in &diff.removed {
        let slot = match base_symbols.slot_at(line.number) {
            Slot::Symbol(path) => {
                Slot::Symbol(head_path_of_base.get(&path).cloned().unwrap_or(path))
            }
            other => other,
        };
        SymbolChange::entry_for(&mut changes, slot)
            .removed
            .push(line);
    }
    for line in &diff.added {
        let slot = head_symbols.slot_at(line.number);
        SymbolChange::entry_for(&mut changes, slot).added.push(line);
    }

    let changes = join_blank_line_changes(changes);

    let mut pieces: Vec<(usize, Piece)> = changes
        .into_iter()
        .map(|change| {
            let (base_slot, head_slot) = match &change.slot {
                Slot::Symbol(path) => (
                    Slot::Symbol(
                        base_path_of_head
                            .get(path.as_str())
                            .map_or(path.clone(), |base| base.to_string()),
                    ),
                    change.slot.clone(),
                ),
                other => (other.clone(), other.clone()),
            };
            let in_base = symbol_in(&base_symbols, &base_slot);
            let in_head = symbol_in(&head_symbols, &head_slot);
            let (kind, status) = match (&change.slot, in_base, in_head) {
                (Slot::Imports, ..) => (PieceKind::Imports, diff.status().into()),
                (Slot::Top, ..) => (PieceKind::Top, diff.status().into()),
                (_, None, Some(symbol)) => (symbol.kind, PieceStatus::Added),
                (_, Some(symbol), None) => (symbol.kind, PieceStatus::Deleted),
                (_, Some(_), Some(symbol)) => (symbol.kind, PieceStatus::Modified),
                (_, None, None) => unreachable!("a symbol slot is found on base or head"),
            };
            let position = in_head
                .or(in_base)
                .map_or_else(|| change.first_line(), |symbol| symbol.first_line);
            let base_tokens = base_symbols.tokens_in(&base_slot);
            let head_tokens = head_symbols.tokens_in(&head_slot);
            let same_tokens = base_tokens == head_tokens;
            let mark = if !same_tokens {
                None
            } else if matches!(change.slot, Slot::Symbol(_))
                && change.moves_code()
                && base_symbols.text_in(&base_slot) == head_symbols.text_in(&head_slot)
            {
                Some(PieceMark::Moved)
            } else {
                Some(PieceMark::Format)
            };
            let moved_body = match (&change.slot, status) {
                (Slot::Symbol(_), PieceStatus::Added) => Some(head_tokens.join(TOKEN_SEPARATOR)),
                (Slot::Symbol(_), PieceStatus::Deleted) => Some(base_tokens.join(TOKEN_SEPARATOR)),
                _ => None,
            };
            let piece = Piece {
                id: PieceId {
                    path: diff.path().to_string(),
                    symbol: Some(change.slot.id()),
                },
                kind,
                status,
                marks: mark.into_iter().collect(),
                added: change.added.len(),
                removed: change.removed.len(),
                signature: in_head.or(in_base).map(|symbol| symbol.signature.clone()),
                diff_hash: DiffHash::of(&change.removed, &change.added),
                removed_lines: change.removed.iter().copied().cloned().collect(),
                added_lines: change.added.iter().copied().cloned().collect(),
                image: None,
                moved_body,
            };
            (position, piece)
        })
        .collect();
    pieces.sort_by_key(|(position, _)| *position);
    Ok(pieces.into_iter().map(|(_, piece)| piece).collect())
}

/// Blank-line-only changes ride along with the nearest piece that has real changes.
/// They stay a piece of their own only when nothing else in the file changed.
fn join_blank_line_changes(changes: Vec<SymbolChange>) -> Vec<SymbolChange> {
    let (blank_only, mut changes): (Vec<_>, Vec<_>) =
        changes.into_iter().partition(|change| !change.moves_code());
    if changes.is_empty() {
        return blank_only;
    }
    for blank in blank_only {
        for (is_added, line) in blank
            .removed
            .into_iter()
            .map(|line| (false, line))
            .chain(blank.added.into_iter().map(|line| (true, line)))
        {
            let nearest = changes
                .iter_mut()
                .min_by_key(|change| change.distance_to(line.number))
                .expect("changes is not empty");
            if is_added {
                nearest.added.push(line);
            } else {
                nearest.removed.push(line);
            }
        }
    }
    for change in &mut changes {
        change.added.sort_by_key(|line| line.number);
        change.removed.sort_by_key(|line| line.number);
    }
    changes
}

fn symbol_in<'s>(symbols: &'s Symbols, slot: &Slot) -> Option<&'s Symbol> {
    match slot {
        Slot::Symbol(path) => symbols.named(path),
        Slot::Imports | Slot::Top => None,
    }
}

const TOKEN_SEPARATOR: &str = "\u{1f}";

/// A symbol deleted in one place and added in another with the same tokens did not change.
fn mark_moved_symbols(files: &mut [ChangedFile]) {
    let mut deleted: Vec<&mut Piece> = Vec::new();
    let mut added: Vec<&mut Piece> = Vec::new();
    for piece in files.iter_mut().flat_map(|file| file.pieces.iter_mut()) {
        match (piece.status, piece.moved_body.is_some()) {
            (PieceStatus::Deleted, true) => deleted.push(piece),
            (PieceStatus::Added, true) => added.push(piece),
            _ => {}
        }
    }
    for deleted_piece in &mut deleted {
        let counterpart = added.iter_mut().find(|candidate| {
            candidate.marks.is_empty()
                && candidate.kind == deleted_piece.kind
                && candidate.moved_body == deleted_piece.moved_body
        });
        if let Some(counterpart) = counterpart {
            counterpart.marks.push(PieceMark::Moved);
            deleted_piece.marks.push(PieceMark::Moved);
        }
    }
}

fn symbols_at(
    source: &dyn CodeSource,
    sha: &Sha,
    path: Option<&str>,
    language: Language,
) -> anyhow::Result<Symbols> {
    Ok(match path {
        Some(path) => Symbols::find(&source.file_at(sha, path)?, language),
        None => Symbols::find("", language),
    })
}

struct SymbolChange<'a> {
    slot: Slot,
    removed: Vec<&'a Line>,
    added: Vec<&'a Line>,
}

impl<'a> SymbolChange<'a> {
    fn entry_for(changes: &mut Vec<Self>, slot: Slot) -> &mut Self {
        let index = match changes.iter().position(|change| change.slot == slot) {
            Some(index) => index,
            None => {
                changes.push(Self {
                    slot,
                    removed: Vec::new(),
                    added: Vec::new(),
                });
                changes.len() - 1
            }
        };
        &mut changes[index]
    }

    fn moves_code(&self) -> bool {
        self.added
            .iter()
            .chain(&self.removed)
            .any(|line| !line.text.trim().is_empty())
    }

    fn distance_to(&self, line: usize) -> usize {
        self.added
            .iter()
            .chain(&self.removed)
            .map(|other| other.number.abs_diff(line))
            .min()
            .unwrap_or(usize::MAX)
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
