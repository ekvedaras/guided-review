//! A piece's diff as a screen shows it, and the pieces a step shows.

use super::code_link::CodeLink;
use super::diff_display::{DiffDisplay, DiffHunk, PieceView, is_in_github_diff};
use super::diff_line::{DiffLine, RowView};
use super::line_location::Side;
use crate::code_source::Line;
use crate::coverage::{self, PiecePart};
use crate::pieces::{ChangedFile, Piece, PieceId, PieceKind};
use crate::repin::RepinStatus;
use crate::tour::{GuidedTour, Step};
use std::ops::RangeInclusive;

#[derive(Debug, Clone)]
pub struct PieceDiff {
    pub id: PieceId,
    pub hunks: Vec<DiffHunk>,
    /// Its changes are all hidden because whitespace is ignored.
    pub changes_only_whitespace: bool,
    /// Set for a binary file: it has no lines, only this one row.
    pub binary_file: Option<CodeLink>,
    /// The file is a picture; the app shows it from `Review::image_preview`.
    pub is_image: bool,
    /// What the last re-pin did to this piece; `None` when the tour was not re-pinned.
    pub repin_status: Option<RepinStatus>,
    /// The lines its symbol spans at head; unchanged rows there belong to it.
    pub head_lines: Option<RangeInclusive<usize>>,
    /// Lines the piece adds and removes in the PR, whether or not whitespace changes are shown.
    pub changed_lines: usize,
}

impl PieceDiff {
    /// Where an IDE opens this piece: the first added line, or the first removed one when nothing was added.
    /// The first line at head, which the working copy has; a piece of only removed lines opens at its first.
    pub fn code_link(&self) -> Option<&CodeLink> {
        self.lines()
            .find(|line| line.location.side == Side::Head)
            .or_else(|| self.lines().next())
            .map(|line| &line.code_link)
            .or(self.binary_file.as_ref())
    }

    /// None when the piece's file is not among the changed files, which a cut piece never is.
    pub(super) fn of(
        part: &PiecePart,
        files: &[ChangedFile],
        display: &DiffDisplay,
    ) -> Option<Self> {
        let piece = part.piece;
        let file = files.iter().find(|file| file.path == *piece.id.path())?;
        let hunks = display.hunks(
            &PieceView::Full(piece.id.clone()),
            file,
            &part.removed_lines,
            &part.added_lines,
            part.range
                .map(|[first, last]| first as usize..=last as usize),
        );
        let is_image = piece.kind == PieceKind::Image;
        let has_changes = !part.removed_lines.is_empty() || !part.added_lines.is_empty();
        Some(Self {
            id: piece.id.clone(),
            changes_only_whitespace: has_changes && hunks.is_empty(),
            hunks,
            binary_file: (file.binary && !is_image).then(|| display.links.to_file(file)),
            is_image,
            repin_status: None,
            head_lines: piece.head_lines.clone(),
            changed_lines: part.removed_lines.len() + part.added_lines.len(),
        })
    }

    pub fn lines(&self) -> impl Iterator<Item = &DiffLine> + Clone {
        self.hunks.iter().flat_map(|hunk| hunk.rows.iter())
    }
}

impl PieceDiff {
    /// The change to a piece between the head it was read at and the tour's head, cut from the
    /// diff between those two heads. Symbol pieces match by id; hunks and whole-file pieces by
    /// where their lines are, because hunk ordinals of the two diffs say nothing about each other.
    /// No rows means nothing changed since the read. `display` links lines to the read head and
    /// shares everything else with the full diff, so rows keep path, side, line, links and spans.
    pub(super) fn since_read(
        part: &PiecePart,
        since_files: &[ChangedFile],
        pr_file: Option<&ChangedFile>,
        display: &DiffDisplay,
    ) -> Self {
        let piece = part.piece;
        let mut diff = Self {
            id: piece.id.clone(),
            hunks: Vec::new(),
            changes_only_whitespace: false,
            binary_file: None,
            is_image: false,
            repin_status: None,
            head_lines: piece.head_lines.clone(),
            changed_lines: part.removed_lines.len() + part.added_lines.len(),
        };
        let Some(file) = since_files
            .iter()
            .find(|file| file.path == *piece.id.path())
        else {
            return diff;
        };
        let changed_since = pieces_changed_since(piece, file);
        let removed: Vec<&Line> = changed_since
            .iter()
            .flat_map(|since| &since.removed_lines)
            .collect();
        let added: Vec<&Line> = changed_since
            .iter()
            .flat_map(|since| &since.added_lines)
            .collect();
        // Its own key, so asking for more lines here does not widen the same piece in the full diff.
        diff.hunks = display.hunks(
            &PieceView::SinceRead(piece.id.clone()),
            file,
            &removed,
            &added,
            part.range
                .map(|[first, last]| first as usize..=last as usize),
        );
        // Removed lines are numbered at the read head, so the PR base's colours do not fit them.
        for row in diff.hunks.iter_mut().flat_map(|hunk| hunk.rows.iter_mut()) {
            row.view = RowView::ChangeSinceRead {
                head_line_is_in_pr_diff: pr_file.is_some_and(|file| {
                    is_in_github_diff(&file.hunks, usize::from(row.location.line))
                }),
            };
            if row.location.side == Side::Base {
                row.spans.clear();
            }
        }
        diff.changes_only_whitespace =
            (!removed.is_empty() || !added.is_empty()) && diff.hunks.is_empty();
        diff.binary_file =
            (file.binary && !changed_since.is_empty() && piece.kind != PieceKind::Image)
                .then(|| display.links.to_file(file));
        diff
    }
}

/// The pieces of `since_file`, a file's changes between a read head and the tour's head, that
/// change what `piece` shows.
pub(super) fn pieces_changed_since<'a>(
    piece: &Piece,
    since_file: &'a ChangedFile,
) -> Vec<&'a Piece> {
    let added_range = piece.added_lines.first().zip(piece.added_lines.last());
    since_file
        .pieces
        .iter()
        .filter(|since| match (piece.id.symbol_path(), piece.kind) {
            (None, _) => true,
            (Some(_), PieceKind::Hunk) => added_range.is_some_and(|(first, last)| {
                since
                    .added_lines
                    .iter()
                    .any(|line| (first.number..=last.number).contains(&line.number))
            }),
            (Some(_), _) => since.id == piece.id,
        })
        .collect()
}

/// The pieces a step shows, in pointer order.
pub(super) fn pieces_of<'a>(
    step: &Step,
    tour: &GuidedTour,
    files: &'a [ChangedFile],
) -> Vec<&'a Piece> {
    step.pieces
        .iter()
        .flat_map(|pointer| coverage::shown_parts(pointer, tour, files))
        .map(|part| part.piece)
        .collect()
}
