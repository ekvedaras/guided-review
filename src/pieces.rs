use std::collections::HashMap;
use std::fmt;

use sha2::{Digest, Sha256};

use crate::code_source::{CodeSource, FileAt, FileDiff, FileStatus, Hunk, Line};
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

impl Piece {
    fn new(
        id: PieceId,
        kind: PieceKind,
        status: PieceStatus,
        removed: &[&Line],
        added: &[&Line],
    ) -> Self {
        Self {
            id,
            kind,
            status,
            added: added.len(),
            removed: removed.len(),
            signature: None,
            marks: Vec::new(),
            diff_hash: DiffHash::of(removed, added),
            removed_lines: removed.iter().copied().cloned().collect(),
            added_lines: added.iter().copied().cloned().collect(),
            image: None,
            moved_body: None,
        }
    }

    fn with_signature(self, signature: Option<String>) -> Self {
        Self { signature, ..self }
    }

    fn with_marks(self, marks: Vec<PieceMark>) -> Self {
        Self { marks, ..self }
    }

    fn with_moved_body(self, moved_body: Option<String>) -> Self {
        Self { moved_body, ..self }
    }
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
    source.read_ahead(&files_cut_reads(base, head, &diffs));
    let generated = generated_files(source, head);
    let mut files = diffs
        .iter()
        .map(|diff| {
            let context = CutContext {
                source,
                base,
                head,
                diff,
            };
            let pieces = match context.cut_by_file_rules(&generated) {
                Some(pieces) => pieces,
                None => context.cut_by_symbols()?,
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

/// Generated files are read too, since `.gitattributes` is not known yet.
fn files_cut_reads(base: &Sha, head: &Sha, diffs: &[FileDiff]) -> Vec<FileAt> {
    let cut_by_symbols = diffs.iter().filter(|diff| {
        !diff.binary
            && diff.status() != FileStatus::Deleted
            && !(is_cut_per_hunk(diff.path()) && !diff.hunks.is_empty())
            && Language::for_path(diff.path()).is_some()
    });
    let both_sides = cut_by_symbols.flat_map(|diff| {
        let at_base = diff.old_path.as_deref().map(|path| (base, path));
        let at_head = diff.new_path.as_deref().map(|path| (head, path));
        at_base.into_iter().chain(at_head)
    });
    std::iter::once((head, GIT_ATTRIBUTES))
        .chain(both_sides)
        .map(|(sha, path)| FileAt {
            sha: sha.clone(),
            path: path.to_string(),
        })
        .collect()
}

const GIT_ATTRIBUTES: &str = ".gitattributes";

fn generated_files(source: &dyn CodeSource, head: &Sha) -> GeneratedFiles {
    let attributes_at_head = source.file_at(head, GIT_ATTRIBUTES).unwrap_or_default();
    GeneratedFiles::new(&attributes_at_head)
}

/// What cutting one changed file into pieces needs to know.
struct CutContext<'a> {
    source: &'a dyn CodeSource,
    base: &'a Sha,
    head: &'a Sha,
    diff: &'a FileDiff,
}

impl<'a> CutContext<'a> {
    fn cut_by_symbols(&self) -> anyhow::Result<Vec<Piece>> {
        let symbol_pieces = match Language::for_path(self.diff.path()) {
            Some(language) => self.cut_symbols(language)?,
            None => Vec::new(),
        };
        Ok(if symbol_pieces.is_empty() {
            vec![self.whole_file_piece()]
        } else {
            symbol_pieces
        })
    }

    /// Every file-level decision, made before any symbol is looked for.
    /// `None` means the file is a normal source file and symbols decide its pieces.
    fn cut_by_file_rules(&self, generated: &GeneratedFiles) -> Option<Vec<Piece>> {
        let diff = self.diff;
        if generated.contains(diff.path()) {
            let piece = self.whole_file_piece();
            return Some(vec![piece.with_marks(vec![PieceMark::Generated])]);
        }
        if image_facts::is_image_path(diff.path())
            && (diff.binary || image_facts::is_svg_path(diff.path()))
        {
            return Some(vec![self.image_piece()]);
        }
        if diff.binary || diff.status() == FileStatus::Deleted {
            return Some(vec![self.whole_file_piece()]);
        }
        if is_cut_per_hunk(diff.path()) && !diff.hunks.is_empty() {
            return Some(self.hunk_pieces());
        }
        None
    }

    fn hunk_pieces(&self) -> Vec<Piece> {
        self.diff
            .hunks
            .iter()
            .enumerate()
            .map(|(index, hunk)| {
                let removed: Vec<&Line> = hunk.removed.iter().collect();
                let added: Vec<&Line> = hunk.added.iter().collect();
                Piece::new(
                    PieceId {
                        path: self.diff.path().to_string(),
                        symbol: Some(format!("hunk-{}", index + 1)),
                    },
                    PieceKind::Hunk,
                    self.diff.status().into(),
                    &removed,
                    &added,
                )
                .with_signature(Some(hunk.header.clone()))
            })
            .collect()
    }

    fn whole_file_piece(&self) -> Piece {
        let removed: Vec<&Line> = self.diff.removed.iter().collect();
        let added: Vec<&Line> = self.diff.added.iter().collect();
        Piece::new(
            PieceId {
                path: self.diff.path().to_string(),
                symbol: None,
            },
            PieceKind::File,
            self.diff.status().into(),
            &removed,
            &added,
        )
    }

    fn image_piece(&self) -> Piece {
        let size_at = |sha: &Sha, path: Option<&str>| {
            let path = path?;
            if self.source.blob_info_at(sha, path).ok()?.size > image_facts::MAX_PREVIEW_BYTES {
                return None;
            }
            let bytes = self.source.blob_at(sha, path).ok()?;
            image_facts::measure(path, &bytes)
        };
        let mut piece = self.whole_file_piece();
        piece.kind = PieceKind::Image;
        piece.image = Some(ImageChange {
            base: size_at(self.base, self.diff.old_path.as_deref()),
            head: size_at(self.head, self.diff.new_path.as_deref()),
        });
        piece
    }

    fn symbols_at(
        &self,
        sha: &Sha,
        path: Option<&str>,
        language: Language,
    ) -> anyhow::Result<Symbols> {
        Ok(match path {
            Some(path) => Symbols::find(&self.source.file_at(sha, path)?, language),
            None => Symbols::find("", language),
        })
    }

    fn cut_symbols(&self, language: Language) -> anyhow::Result<Vec<Piece>> {
        let base_symbols = self.symbols_at(self.base, self.diff.old_path.as_deref(), language)?;
        let head_symbols = self.symbols_at(self.head, self.diff.new_path.as_deref(), language)?;
        let pairing = SymbolPairing::new(&base_symbols, &head_symbols);
        let changes = join_blank_line_changes(self.changes_by_slot(&pairing));
        let mut pieces: Vec<(usize, Piece)> = changes
            .into_iter()
            .map(|change| symbol_piece(&pairing, change, self.diff.status().into()))
            .collect();
        pieces.sort_by_key(|(position, _)| *position);
        Ok(pieces.into_iter().map(|(_, piece)| piece).collect())
    }

    /// Changed lines grouped by head slot: a removed line of a renamed symbol counts for its head name.
    fn changes_by_slot(&self, pairing: &SymbolPairing) -> Vec<SymbolChange<'a>> {
        let diff = self.diff;
        let mut changes: Vec<SymbolChange> = Vec::new();
        for line in &diff.removed {
            let slot = pairing.head_slot_of_removed_line(line.number);
            SymbolChange::entry_for(&mut changes, diff.path(), slot)
                .removed
                .push(line);
        }
        for line in &diff.added {
            let slot = pairing.head.slot_at(line.number);
            SymbolChange::entry_for(&mut changes, diff.path(), slot)
                .added
                .push(line);
        }
        changes
    }
}

/// The symbols of a file at base and at head, and which base symbol continues as which head symbol.
struct SymbolPairing<'a> {
    base: &'a Symbols,
    head: &'a Symbols,
    head_path_of_base: HashMap<String, String>,
    base_path_of_head: HashMap<String, String>,
}

impl<'a> SymbolPairing<'a> {
    fn new(base: &'a Symbols, head: &'a Symbols) -> Self {
        let head_path_of_base = base.pair_with_head(head);
        let base_path_of_head = head_path_of_base
            .iter()
            .map(|(base, head)| (head.clone(), base.clone()))
            .collect();
        Self {
            base,
            head,
            head_path_of_base,
            base_path_of_head,
        }
    }

    fn head_slot_of_removed_line(&self, base_line: usize) -> Slot {
        match self.base.slot_at(base_line) {
            Slot::Symbol(path) => {
                Slot::Symbol(self.head_path_of_base.get(&path).cloned().unwrap_or(path))
            }
            other => other,
        }
    }

    fn slots_of(&self, head_slot: &Slot) -> SlotPair {
        let base = match head_slot {
            Slot::Symbol(path) => {
                Slot::Symbol(self.base_path_of_head.get(path).unwrap_or(path).clone())
            }
            other => other.clone(),
        };
        SlotPair {
            base,
            head: head_slot.clone(),
        }
    }
}

/// The same part of a file as it is named at base and at head.
struct SlotPair {
    base: Slot,
    head: Slot,
}

fn symbol_piece(
    pairing: &SymbolPairing,
    change: SymbolChange,
    file_status: PieceStatus,
) -> (usize, Piece) {
    let slots = pairing.slots_of(&change.slot);
    let in_base = symbol_in(pairing.base, &slots.base);
    let in_head = symbol_in(pairing.head, &slots.head);
    let (kind, status) = kind_and_status(&change.slot, in_base, in_head, file_status);
    let position = in_head
        .or(in_base)
        .map_or_else(|| change.first_line(), |symbol| symbol.first_line);
    let base_tokens = pairing.base.tokens_in(&slots.base);
    let head_tokens = pairing.head.tokens_in(&slots.head);
    let mark = mark_for(&change, &slots, pairing, base_tokens == head_tokens);
    let moved_body = match (&change.slot, status) {
        (Slot::Symbol(_), PieceStatus::Added) => Some(head_tokens.join(TOKEN_SEPARATOR)),
        (Slot::Symbol(_), PieceStatus::Deleted) => Some(base_tokens.join(TOKEN_SEPARATOR)),
        _ => None,
    };
    let piece = Piece::new(
        PieceId {
            path: change.path.to_string(),
            symbol: Some(change.slot.id()),
        },
        kind,
        status,
        &change.removed,
        &change.added,
    )
    .with_marks(mark.into_iter().collect())
    .with_signature(in_head.or(in_base).map(|symbol| symbol.signature.clone()))
    .with_moved_body(moved_body);
    (position, piece)
}

fn kind_and_status(
    slot: &Slot,
    in_base: Option<&Symbol>,
    in_head: Option<&Symbol>,
    file_status: PieceStatus,
) -> (PieceKind, PieceStatus) {
    match (slot, in_base, in_head) {
        (Slot::Imports, ..) => (PieceKind::Imports, file_status),
        (Slot::Top, ..) => (PieceKind::Top, file_status),
        (_, None, Some(symbol)) => (symbol.kind, PieceStatus::Added),
        (_, Some(symbol), None) => (symbol.kind, PieceStatus::Deleted),
        (_, Some(_), Some(symbol)) => (symbol.kind, PieceStatus::Modified),
        (_, None, None) => unreachable!("a symbol slot is found on base or head"),
    }
}

/// Same tokens on both sides means only formatting changed, or the symbol only moved inside the file.
fn mark_for(
    change: &SymbolChange,
    slots: &SlotPair,
    pairing: &SymbolPairing,
    same_tokens: bool,
) -> Option<PieceMark> {
    if !same_tokens {
        return None;
    }
    let moved = matches!(change.slot, Slot::Symbol(_))
        && change.moves_code()
        && pairing.base.text_in(&slots.base) == pairing.head.text_in(&slots.head);
    Some(if moved {
        PieceMark::Moved
    } else {
        PieceMark::Format
    })
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

struct SymbolChange<'a> {
    path: &'a str,
    slot: Slot,
    removed: Vec<&'a Line>,
    added: Vec<&'a Line>,
}

impl<'a> SymbolChange<'a> {
    fn entry_for<'c>(changes: &'c mut Vec<Self>, path: &'a str, slot: Slot) -> &'c mut Self {
        let index = match changes.iter().position(|change| change.slot == slot) {
            Some(index) => index,
            None => {
                changes.push(Self {
                    path,
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
