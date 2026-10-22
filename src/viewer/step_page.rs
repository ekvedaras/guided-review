use crate::coverage;
use crate::pieces::{ChangedFile, Piece, PieceId};
use crate::tour::{PieceRef, Step, Tour};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Base,
    Head,
}

/// Where a shown line lives. Comments, GitHub links and "open in IDE" all start from this.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineLocation {
    /// GitHub addresses both sides of a renamed file by its new path.
    pub path: String,
    pub side: Side,
    pub line: usize,
}

#[derive(Debug, Clone)]
pub struct DiffLine {
    pub location: LineLocation,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct PieceDiff {
    pub id: PieceId,
    pub lines: Vec<DiffLine>,
}

pub struct StepPage<'a> {
    pub step: &'a Step,
    pub pieces: Vec<PieceDiff>,
}

impl<'a> StepPage<'a> {
    pub(super) fn of(step: &'a Step, tour: &Tour, files: &[ChangedFile]) -> Self {
        let pieces = step
            .pieces
            .iter()
            .flat_map(|pointer| match pointer {
                PieceRef::Rest { rest } => coverage::left_for_rest(tour, files, rest),
                _ => pointer.named_pieces(files).unwrap_or_default(),
            })
            .map(PieceDiff::of)
            .collect();
        Self { step, pieces }
    }
}

impl PieceDiff {
    fn of(piece: &Piece) -> Self {
        let on_side = |side: Side| {
            move |line: &crate::code_source::Line| DiffLine {
                location: LineLocation {
                    path: piece.id.path.clone(),
                    side,
                    line: line.number,
                },
                text: line.text.clone(),
            }
        };
        let lines = piece
            .removed_lines
            .iter()
            .map(on_side(Side::Base))
            .chain(piece.added_lines.iter().map(on_side(Side::Head)))
            .collect();
        Self {
            id: piece.id.clone(),
            lines,
        }
    }
}
