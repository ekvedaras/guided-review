use std::collections::{HashMap, HashSet};

use crate::code_source::Line;
use crate::pieces::{ChangedFile, Piece, PieceId};
use crate::tour::{PieceRef, Tour};

/// The pieces this pointer names directly; `None` when it names something that did not change.
/// `rest` names nothing directly, it only mops up what is left.
pub fn named_pieces<'a>(pointer: &PieceRef, files: &'a [ChangedFile]) -> Option<Vec<&'a Piece>> {
    match pointer {
        PieceRef::Rest { .. } => Some(Vec::new()),
        PieceRef::WholeFile { file } => files
            .iter()
            .find(|changed| changed.path == *file)
            .map(|changed| changed.pieces.iter().collect()),
        PieceRef::Lines { file, lines } => {
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
        PieceRef::Symbol { file, symbol } => files
            .iter()
            .flat_map(|changed| &changed.pieces)
            .find(|piece| {
                piece.id.path == *file && piece.id.symbol.as_deref() == Some(symbol.as_str())
            })
            .map(|piece| vec![piece]),
    }
}

/// What a step shows for this pointer: whole pieces, or for a line range only the lines inside it.
pub fn shown_parts<'a>(
    pointer: &PieceRef,
    tour: &Tour,
    files: &'a [ChangedFile],
) -> Vec<PiecePart<'a>> {
    match pointer {
        PieceRef::Rest { rest } => left_for_rest(tour, files, rest),
        _ => named_parts(pointer, files),
    }
}

/// The parts this pointer names directly; a line range names only the lines inside it.
fn named_parts<'a>(pointer: &PieceRef, files: &'a [ChangedFile]) -> Vec<PiecePart<'a>> {
    let pieces = named_pieces(pointer, files).unwrap_or_default();
    match pointer {
        PieceRef::Lines { lines, .. } => pieces
            .into_iter()
            .filter_map(|piece| PiecePart::within(piece, *lines))
            .collect(),
        _ => pieces.into_iter().map(PiecePart::whole).collect(),
    }
}

/// A piece, or the part of it that one place shows.
pub struct PiecePart<'a> {
    pub piece: &'a Piece,
    pub removed_lines: Vec<&'a Line>,
    pub added_lines: Vec<&'a Line>,
    /// Set when a line range cut the piece: unchanged lines around it stay inside the range.
    pub range: Option<[u32; 2]>,
}

impl<'a> PiecePart<'a> {
    pub fn whole(piece: &'a Piece) -> Self {
        Self::keeping(piece, |_| true)
    }

    /// A line is inside a range by its own number: base numbers for removed lines, head numbers for added.
    fn within(piece: &'a Piece, [first, last]: [u32; 2]) -> Option<Self> {
        let range = first as usize..=last as usize;
        let part = Self::keeping(piece, |line| range.contains(&line.number));
        (!part.is_empty()).then_some(Self {
            range: Some([first, last]),
            ..part
        })
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
            range: None,
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
            range: None,
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
struct Seen<'a>(HashMap<&'a PieceId, SeenLines>);

#[derive(Default)]
struct SeenLines {
    removed: HashSet<usize>,
    added: HashSet<usize>,
}

impl<'a> Seen<'a> {
    fn of(parts: impl IntoIterator<Item = PiecePart<'a>>) -> Self {
        let mut seen = Self::default();
        for part in parts {
            let lines = seen.0.entry(&part.piece.id).or_default();
            lines
                .removed
                .extend(part.removed_lines.iter().map(|line| line.number));
            lines
                .added
                .extend(part.added_lines.iter().map(|line| line.number));
        }
        seen
    }

    fn named_by(pointers: &[&PieceRef], files: &'a [ChangedFile]) -> Self {
        Self::of(pointers.iter().flat_map(|pointer| match pointer {
            PieceRef::Rest { .. } => Vec::new(),
            named => named_parts(named, files),
        }))
    }

    /// The lines of the piece nobody shows; a piece without changed lines is unseen until a pointer names it.
    fn unseen<'p>(&self, piece: &'p Piece) -> Option<PiecePart<'p>> {
        match self.0.get(&piece.id) {
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
            .flat_map(|pointer| shown_parts(pointer, tour, files)),
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
                .any(|folder| is_under(file.path.as_str(), folder))
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
        .filter(|file| is_under(file.path.as_str(), folder))
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

/// What one `rest` pointer took: the changes under its folder that no other pointer names
/// and no earlier `rest` pointer took.
pub struct RestTaken<'a> {
    pub folder: &'a str,
    pub pieces: usize,
    pub files: Vec<&'a str>,
}

pub fn rest_taken<'a>(tour: &'a Tour, files: &'a [ChangedFile]) -> Vec<RestTaken<'a>> {
    let steps = tour.steps();
    let pointers: Vec<&PieceRef> = steps.iter().flat_map(|step| &step.pieces).collect();
    let seen = Seen::named_by(&pointers, files);
    let mut taken_by_earlier: HashSet<&PieceId> = HashSet::new();

    let mut all = Vec::new();
    for pointer in pointers {
        let PieceRef::Rest { rest } = pointer else {
            continue;
        };
        let folder = rest.trim_matches('/');
        let mut taken = RestTaken {
            folder,
            pieces: 0,
            files: Vec::new(),
        };
        for file in files
            .iter()
            .filter(|file| is_under(file.path.as_str(), folder))
        {
            let mut took_any = false;
            for piece in &file.pieces {
                if seen.unseen(piece).is_some() && taken_by_earlier.insert(&piece.id) {
                    taken.pieces += 1;
                    took_any = true;
                }
            }
            if took_any {
                taken.files.push(file.path.as_str());
            }
        }
        all.push(taken);
    }
    all
}
