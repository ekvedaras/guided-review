use std::collections::{HashMap, HashSet};

use crate::code_source::Line;
use crate::pieces::{ChangedFile, Piece};
use crate::tour::{PieceRef, Tour};

impl PieceRef {
    /// The pieces this pointer names directly; `None` when it names something that did not change.
    /// `rest` names nothing directly, it only mops up what is left.
    pub fn named_pieces<'a>(&self, files: &'a [ChangedFile]) -> Option<Vec<&'a Piece>> {
        match self {
            Self::Rest { .. } => Some(Vec::new()),
            Self::WholeFile { file } => files
                .iter()
                .find(|changed| changed.path == *file)
                .map(|changed| changed.pieces.iter().collect()),
            Self::Lines { file, lines } => {
                files
                    .iter()
                    .find(|changed| changed.path == *file)
                    .map(|changed| {
                        changed
                            .pieces
                            .iter()
                            .filter(|piece| PiecePart::within(piece, *lines).is_some())
                            .collect()
                    })
            }
            Self::Symbol { file, symbol } => files
                .iter()
                .flat_map(|changed| &changed.pieces)
                .find(|piece| {
                    piece.id.path == *file && piece.id.symbol.as_deref() == Some(symbol.as_str())
                })
                .map(|piece| vec![piece]),
        }
    }
}

impl PieceRef {
    /// What a step shows for this pointer: whole pieces, or for a line range only the lines inside it.
    pub fn shown_parts<'a>(&self, tour: &Tour, files: &'a [ChangedFile]) -> Vec<PiecePart<'a>> {
        match self {
            Self::Rest { rest } => left_for_rest(tour, files, rest)
                .into_iter()
                .map(PiecePart::whole)
                .collect(),
            Self::Lines { lines, .. } => self
                .named_pieces(files)
                .unwrap_or_default()
                .into_iter()
                .filter_map(|piece| PiecePart::within(piece, *lines))
                .collect(),
            Self::Symbol { .. } | Self::WholeFile { .. } => self
                .named_pieces(files)
                .unwrap_or_default()
                .into_iter()
                .map(PiecePart::whole)
                .collect(),
        }
    }
}

/// A piece, or the part of it that one place shows.
pub struct PiecePart<'a> {
    pub piece: &'a Piece,
    pub removed_lines: Vec<&'a Line>,
    pub added_lines: Vec<&'a Line>,
}

impl<'a> PiecePart<'a> {
    pub fn whole(piece: &'a Piece) -> Self {
        Self::keeping(piece, |_| true)
    }

    /// A line is inside a range by its own number: base numbers for removed lines, head numbers for added.
    fn within(piece: &'a Piece, [first, last]: [u32; 2]) -> Option<Self> {
        let range = first as usize..=last as usize;
        Some(Self::keeping(piece, |line| range.contains(&line.number)))
            .filter(|part| !part.is_empty())
    }

    fn keeping(piece: &'a Piece, keep: impl Fn(&Line) -> bool) -> Self {
        Self {
            piece,
            removed_lines: piece
                .removed_lines
                .iter()
                .filter(|line| keep(line))
                .collect(),
            added_lines: piece.added_lines.iter().filter(|line| keep(line)).collect(),
        }
    }

    fn is_empty(&self) -> bool {
        self.removed_lines.is_empty() && self.added_lines.is_empty()
    }
}

/// Everything the tour leaves unseen: pieces no step shows, and the lines of a piece
/// that line ranges cut through but do not reach. Computed by the viewer, never stored.
pub fn safety_net<'a>(tour: &Tour, files: &'a [ChangedFile]) -> Vec<PiecePart<'a>> {
    #[derive(Default)]
    struct Seen {
        removed: HashSet<usize>,
        added: HashSet<usize>,
    }
    let mut seen: HashMap<String, Seen> = HashMap::new();
    for part in tour
        .steps()
        .iter()
        .flat_map(|step| &step.pieces)
        .flat_map(|pointer| pointer.shown_parts(tour, files))
    {
        let lines = seen.entry(part.piece.id.to_string()).or_default();
        lines
            .removed
            .extend(part.removed_lines.iter().map(|line| line.number));
        lines
            .added
            .extend(part.added_lines.iter().map(|line| line.number));
    }
    files
        .iter()
        .flat_map(|file| &file.pieces)
        .filter_map(|piece| match seen.get(&piece.id.to_string()) {
            None => Some(PiecePart::whole(piece)),
            Some(lines) => Some(PiecePart {
                piece,
                removed_lines: piece
                    .removed_lines
                    .iter()
                    .filter(|line| !lines.removed.contains(&line.number))
                    .collect(),
                added_lines: piece
                    .added_lines
                    .iter()
                    .filter(|line| !lines.added.contains(&line.number))
                    .collect(),
            })
            .filter(|unseen| !unseen.is_empty()),
        })
        .collect()
}

/// Changes no step covers. A `rest` pointer covers everything under its folder
/// that is not already covered, so it only matters for what no other pointer names.
pub fn uncovered<'a>(tour: &Tour, files: &'a [ChangedFile]) -> Vec<&'a Piece> {
    let steps = tour.steps();
    let pointers: Vec<&PieceRef> = steps.iter().flat_map(|step| &step.pieces).collect();
    let named = named_by_any_pointer(&pointers, files);
    let rest_folders: Vec<&str> = pointers
        .iter()
        .filter_map(|pointer| match pointer {
            PieceRef::Rest { rest } => Some(rest.trim_matches('/')),
            _ => None,
        })
        .collect();

    files
        .iter()
        .filter(|file| {
            !rest_folders
                .iter()
                .any(|folder| is_under(&file.path, folder))
        })
        .flat_map(|file| &file.pieces)
        .filter(|piece| !named.contains(&piece.id.to_string()))
        .collect()
}

/// What a `rest` pointer stands for: the changes under its folder that no other pointer names.
pub fn left_for_rest<'a>(tour: &Tour, files: &'a [ChangedFile], folder: &str) -> Vec<&'a Piece> {
    let steps = tour.steps();
    let pointers: Vec<&PieceRef> = steps.iter().flat_map(|step| &step.pieces).collect();
    let named = named_by_any_pointer(&pointers, files);
    let folder = folder.trim_matches('/');
    files
        .iter()
        .filter(|file| is_under(&file.path, folder))
        .flat_map(|file| &file.pieces)
        .filter(|piece| !named.contains(&piece.id.to_string()))
        .collect()
}

fn named_by_any_pointer(pointers: &[&PieceRef], files: &[ChangedFile]) -> HashSet<String> {
    pointers
        .iter()
        .filter_map(|pointer| pointer.named_pieces(files))
        .flatten()
        .map(|piece| piece.id.to_string())
        .collect()
}

fn is_under(path: &str, folder: &str) -> bool {
    folder.is_empty()
        || path
            .strip_prefix(folder)
            .is_some_and(|rest| rest.starts_with('/'))
}
