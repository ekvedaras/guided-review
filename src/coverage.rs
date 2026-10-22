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
            Self::Rest { rest } => left_for_rest(tour, files, rest),
            _ => self.named_parts(files),
        }
    }

    /// The parts this pointer names directly; a line range names only the lines inside it.
    fn named_parts<'a>(&self, files: &'a [ChangedFile]) -> Vec<PiecePart<'a>> {
        let pieces = self.named_pieces(files).unwrap_or_default();
        match self {
            Self::Lines { lines, .. } => pieces
                .into_iter()
                .filter_map(|piece| PiecePart::within(piece, *lines))
                .collect(),
            _ => pieces.into_iter().map(PiecePart::whole).collect(),
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

    fn keeping_unseen(piece: &'a Piece, seen: &SeenLines) -> Self {
        Self {
            piece,
            removed_lines: piece
                .removed_lines
                .iter()
                .filter(|line| !seen.removed.contains(&line.number))
                .collect(),
            added_lines: piece
                .added_lines
                .iter()
                .filter(|line| !seen.added.contains(&line.number))
                .collect(),
        }
    }

    fn is_empty(&self) -> bool {
        self.removed_lines.is_empty() && self.added_lines.is_empty()
    }

    /// No line of the piece is missing from this part.
    pub fn is_whole(&self) -> bool {
        self.removed_lines.len() == self.piece.removed_lines.len()
            && self.added_lines.len() == self.piece.added_lines.len()
    }
}

/// The changed lines of each piece that some place shows.
#[derive(Default)]
struct Seen(HashMap<String, SeenLines>);

#[derive(Default)]
struct SeenLines {
    removed: HashSet<usize>,
    added: HashSet<usize>,
}

impl Seen {
    fn of<'a>(parts: impl IntoIterator<Item = PiecePart<'a>>) -> Self {
        let mut seen = Self::default();
        for part in parts {
            let lines = seen.0.entry(part.piece.id.to_string()).or_default();
            lines
                .removed
                .extend(part.removed_lines.iter().map(|line| line.number));
            lines
                .added
                .extend(part.added_lines.iter().map(|line| line.number));
        }
        seen
    }

    fn named_by(pointers: &[&PieceRef], files: &[ChangedFile]) -> Self {
        Self::of(pointers.iter().flat_map(|pointer| match pointer {
            PieceRef::Rest { .. } => Vec::new(),
            named => named.named_parts(files),
        }))
    }

    /// The lines of the piece nobody shows; a piece without changed lines is unseen until a pointer names it.
    fn unseen<'a>(&self, piece: &'a Piece) -> Option<PiecePart<'a>> {
        match self.0.get(&piece.id.to_string()) {
            None => Some(PiecePart::whole(piece)),
            Some(seen) => {
                Some(PiecePart::keeping_unseen(piece, seen)).filter(|unseen| !unseen.is_empty())
            }
        }
    }
}

/// Everything the tour leaves unseen: pieces no step shows, and the lines of a piece
/// that line ranges cut through but do not reach. Computed by the viewer, never stored.
pub fn safety_net<'a>(tour: &Tour, files: &'a [ChangedFile]) -> Vec<PiecePart<'a>> {
    let seen = Seen::of(
        tour.steps()
            .iter()
            .flat_map(|step| &step.pieces)
            .flat_map(|pointer| pointer.shown_parts(tour, files)),
    );
    files
        .iter()
        .flat_map(|file| &file.pieces)
        .filter_map(|piece| seen.unseen(piece))
        .collect()
}

/// Changes no step covers. A `rest` pointer covers everything under its folder
/// that is not already covered, so it only matters for what no other pointer names.
pub fn uncovered<'a>(tour: &Tour, files: &'a [ChangedFile]) -> Vec<PiecePart<'a>> {
    let steps = tour.steps();
    let pointers: Vec<&PieceRef> = steps.iter().flat_map(|step| &step.pieces).collect();
    let seen = Seen::named_by(&pointers, files);
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
        .filter_map(|piece| seen.unseen(piece))
        .collect()
}

/// What a `rest` pointer stands for: the changes under its folder that no other pointer names.
pub fn left_for_rest<'a>(
    tour: &Tour,
    files: &'a [ChangedFile],
    folder: &str,
) -> Vec<PiecePart<'a>> {
    let steps = tour.steps();
    let pointers: Vec<&PieceRef> = steps.iter().flat_map(|step| &step.pieces).collect();
    let seen = Seen::named_by(&pointers, files);
    let folder = folder.trim_matches('/');
    files
        .iter()
        .filter(|file| is_under(&file.path, folder))
        .flat_map(|file| &file.pieces)
        .filter_map(|piece| seen.unseen(piece))
        .collect()
}

fn is_under(path: &str, folder: &str) -> bool {
    folder.is_empty()
        || path
            .strip_prefix(folder)
            .is_some_and(|rest| rest.starts_with('/'))
}
