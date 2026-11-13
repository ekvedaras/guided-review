//! Piece labels: the thin `Class::method` lines inside a file box that mark where each piece starts.
//! A file box shows or hides them as a whole. It is a display option, so hiding them changes only the
//! look: pieces, coverage and read ticks stay as they are.

use std::collections::HashMap;

use super::step_page::{LineKind, PieceDiff};
use crate::file_path::FilePath;

/// A piece that changes this many lines or fewer is tiny: one line added, removed or rewritten, or a
/// pair. In an interface each piece is a signature, so the label comes after every line it labels.
pub const TINY_PIECE_LINES: usize = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PieceLabels {
    Shown,
    Hidden,
}

impl PieceLabels {
    /// What a file starts with, from how many lines each of its pieces changes: hidden when more than
    /// half of them are tiny. Exactly half is not most, so a file split evenly keeps its labels.
    pub fn starting_for_sizes(sizes: &[usize]) -> Self {
        let tiny = sizes
            .iter()
            .filter(|&&size| size <= TINY_PIECE_LINES)
            .count();
        if tiny * 2 > sizes.len() {
            Self::Hidden
        } else {
            Self::Shown
        }
    }

    pub(super) fn starting_for(pieces: &[PieceDiff]) -> Self {
        let sizes: Vec<usize> = pieces.iter().map(changed_lines).collect();
        Self::starting_for_sizes(&sizes)
    }

    pub fn flipped(self) -> Self {
        match self {
            Self::Shown => Self::Hidden,
            Self::Hidden => Self::Shown,
        }
    }
}

fn changed_lines(piece: &PieceDiff) -> usize {
    piece
        .hunks
        .iter()
        .flat_map(|hunk| &hunk.rows)
        .filter(|row| matches!(row.kind, LineKind::Added | LineKind::Removed))
        .count()
}

/// The piece-label toggle in the header of a file box that has labels, set to what the box shows.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LabelSwitch {
    pub path: FilePath,
    pub labels: PieceLabels,
}

/// What the reviewer chose for each file; a file with no choice follows the starting rule.
#[derive(Default)]
pub(super) struct FileLabels {
    chosen: HashMap<FilePath, PieceLabels>,
}

impl FileLabels {
    pub(super) fn of(&self, path: &FilePath, pieces: &[PieceDiff]) -> PieceLabels {
        self.chosen
            .get(path)
            .copied()
            .unwrap_or_else(|| PieceLabels::starting_for(pieces))
    }

    pub(super) fn show_as(&mut self, path: &FilePath, labels: PieceLabels) {
        self.chosen.insert(path.clone(), labels);
    }
}
