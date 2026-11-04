use std::collections::HashSet;

use super::diff_display::{DiffHunk, MoreContext};
use super::step_page::{DiffLine, LineKind, LineLocation, PieceDiff, Side};
use crate::file_path::FilePath;
use crate::line_number::LineNumber;

/// The pieces of one file that a page shows, in one box, in line order. The rows of all pieces
/// are joined, so a row two pieces both show appears once. A label marks where each piece starts:
/// at the first row that belongs to it.
pub struct FileBlock {
    pub path: FilePath,
    /// In the order the page names them.
    pub pieces: Vec<PieceDiff>,
    pub hunks: Vec<DiffHunk>,
    starts: Vec<PieceStart>,
}

/// Where the rows of a piece begin in the block. A piece starts again after the rows of a piece
/// inside it, like a class after one of its methods.
#[derive(Debug, Clone)]
struct PieceStart {
    at: LineLocation,
    piece: usize,
    is_resumed: bool,
}

impl FileBlock {
    /// One block per file, in the order the files first appear among `pieces`.
    pub fn group(pieces: Vec<PieceDiff>) -> Vec<Self> {
        let mut files: Vec<(FilePath, Vec<PieceDiff>)> = Vec::new();
        for piece in pieces {
            match files.iter_mut().find(|(path, _)| path == piece.id.path()) {
                Some((_, of_file)) => of_file.push(piece),
                None => files.push((piece.id.path().clone(), vec![piece])),
            }
        }
        files
            .into_iter()
            .map(|(path, pieces)| Self::of(path, pieces))
            .collect()
    }

    fn of(path: FilePath, pieces: Vec<PieceDiff>) -> Self {
        if let [piece] = pieces.as_slice() {
            return Self {
                path,
                hunks: piece.hunks.clone(),
                pieces,
                starts: Vec::new(),
            };
        }
        let (hunks, owners) = joined(&pieces);
        let starts = starts(&hunks, &owners);
        Self {
            path,
            pieces,
            hunks,
            starts,
        }
    }

    pub fn has_one_piece(&self) -> bool {
        self.pieces.len() == 1
    }

    pub fn lines(&self) -> impl Iterator<Item = &DiffLine> {
        self.hunks.iter().flat_map(|hunk| hunk.rows.iter())
    }

    /// The piece whose rows begin at this row, and whether it began once before.
    pub fn piece_starting_at(&self, location: &LineLocation) -> Option<(&PieceDiff, bool)> {
        self.starts
            .iter()
            .find(|start| start.at == *location)
            .and_then(|start| Some((self.pieces.get(start.piece)?, start.is_resumed)))
    }

    /// Pieces as the box shows them: those without rows first, then by where they first start.
    pub fn pieces_in_order(&self) -> Vec<&PieceDiff> {
        let first_start = |index: usize| {
            self.starts
                .iter()
                .position(|start| start.piece == index && !start.is_resumed)
        };
        let mut order: Vec<(usize, &PieceDiff)> = self.pieces.iter().enumerate().collect();
        order.sort_by_key(|&(index, _)| first_start(index));
        order.into_iter().map(|(_, piece)| piece).collect()
    }

    /// Pieces that own no row with code on it: their label goes above the rows.
    pub fn without_rows(&self) -> impl Iterator<Item = &PieceDiff> {
        self.pieces
            .iter()
            .enumerate()
            .filter(|(index, _)| !self.starts.iter().any(|start| start.piece == *index))
            .map(|(_, piece)| piece)
    }
}

/// Blank lines and lines of only closing brackets never move a label: they read as the end of
/// whatever is above them.
fn starts(hunks: &[DiffHunk], owners: &[Option<usize>]) -> Vec<PieceStart> {
    let mut starts = Vec::new();
    let mut started = HashSet::new();
    let mut current = None;
    for (row, owner) in hunks.iter().flat_map(|hunk| &hunk.rows).zip(owners) {
        let Some(owner) = *owner else { continue };
        if current == Some(owner) || !has_code(&row.text) {
            continue;
        }
        starts.push(PieceStart {
            at: row.location.clone(),
            piece: owner,
            is_resumed: !started.insert(owner),
        });
        current = Some(owner);
    }
    starts
}

fn has_code(text: &str) -> bool {
    text.chars()
        .any(|c| !c.is_whitespace() && !matches!(c, '}' | ')' | ']' | ';' | ','))
}

/// Where a row sits in the file as the diff shows it: a head line, or a removed line right after
/// the head line it was removed below. Rows one place apart have no hidden line between them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Place {
    head_line: usize,
    row: RowAtPlace,
}

/// A head row sorts before the rows removed below its line, which sort by their base line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum RowAtPlace {
    Head,
    Removed { base_line: LineNumber },
}

struct PlacedRow<'a> {
    place: Place,
    row: &'a DiffLine,
    piece: usize,
}

struct Edges<'a> {
    first: Place,
    last: Place,
    more_above: &'a Option<MoreContext>,
    more_below: &'a Option<MoreContext>,
}

/// The joined hunks, and for each of their rows the piece it belongs to: a changed row to the
/// piece that changed it, an unchanged row to the innermost piece whose symbol spans it.
fn joined(pieces: &[PieceDiff]) -> (Vec<DiffHunk>, Vec<Option<usize>>) {
    let mut rows: Vec<PlacedRow> = Vec::new();
    let mut edges: Vec<Edges> = Vec::new();
    for (piece, hunk) in pieces
        .iter()
        .enumerate()
        .flat_map(|(index, piece)| piece.hunks.iter().map(move |hunk| (index, hunk)))
    {
        let placed = placed_rows(hunk, piece);
        if let (Some(first), Some(last)) = (placed.first(), placed.last()) {
            edges.push(Edges {
                first: first.place,
                last: last.place,
                more_above: &hunk.more_above,
                more_below: &hunk.more_below,
            });
        }
        rows.extend(placed);
    }
    rows.sort_by_key(|placed| placed.place);
    rows.dedup_by(|later, earlier| later.row.location == earlier.row.location);

    let owners = rows
        .iter()
        .map(|placed| match placed.row.kind {
            LineKind::Context { .. } => innermost_spanning(pieces, placed.row.location.line.into()),
            LineKind::Added | LineKind::Removed => Some(placed.piece),
        })
        .collect();

    let mut hunks: Vec<(Place, Place, Vec<DiffLine>)> = Vec::new();
    for placed in rows {
        match hunks.last_mut() {
            Some((_, last, hunk_rows)) if placed.place.head_line <= last.head_line + 1 => {
                *last = placed.place;
                hunk_rows.push(placed.row.clone());
            }
            _ => hunks.push((placed.place, placed.place, vec![placed.row.clone()])),
        }
    }
    let hunks = hunks
        .into_iter()
        .map(|(first, last, rows)| DiffHunk {
            rows,
            more_above: edges
                .iter()
                .find(|edge| edge.first == first)
                .and_then(|edge| edge.more_above.clone()),
            more_below: edges
                .iter()
                .find(|edge| edge.last == last)
                .and_then(|edge| edge.more_below.clone()),
        })
        .collect();
    (hunks, owners)
}

fn innermost_spanning(pieces: &[PieceDiff], head_line: usize) -> Option<usize> {
    pieces
        .iter()
        .enumerate()
        .filter_map(|(index, piece)| {
            let lines = piece.head_lines.as_ref()?;
            lines
                .contains(&head_line)
                .then(|| (lines.end() - lines.start(), index))
        })
        .min()
        .map(|(_, index)| index)
}

/// A removed row takes its place from the head rows around it in its own hunk.
fn placed_rows(hunk: &DiffHunk, piece: usize) -> Vec<PlacedRow<'_>> {
    let head_line = |row: &DiffLine| {
        (row.location.side == Side::Head).then_some(usize::from(row.location.line))
    };
    hunk.rows
        .iter()
        .enumerate()
        .map(|(index, row)| {
            let place = match row.kind {
                LineKind::Removed => Place {
                    head_line: hunk
                        .rows
                        .iter()
                        .take(index)
                        .rev()
                        .find_map(head_line)
                        .or_else(|| {
                            hunk.rows
                                .iter()
                                .skip(index)
                                .find_map(head_line)
                                .map(|line| line - 1)
                        })
                        .unwrap_or_else(|| row.location.line.into()),
                    row: RowAtPlace::Removed {
                        base_line: row.location.line,
                    },
                },
                LineKind::Added | LineKind::Context { .. } => Place {
                    head_line: row.location.line.into(),
                    row: RowAtPlace::Head,
                },
            };
            PlacedRow { place, row, piece }
        })
        .collect()
}
