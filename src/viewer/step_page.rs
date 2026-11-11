use std::ops::{Range, RangeInclusive};

use serde::{Deserialize, Serialize};

use super::code_link::CodeLink;
use super::comments::CommentRefusal;
use super::diff_display::{DiffDisplay, DiffHunk, PieceView, is_in_github_diff};
use super::highlight::Span;
use crate::code_source::Line;
use crate::coverage::{self, PiecePart};
use crate::file_path::FilePath;
use crate::line_number::LineNumber;
use crate::pieces::{ChangedFile, Piece, PieceId, PieceKind};
use crate::pull_request::Sha;
use crate::repin::RepinStatus;
use crate::tour::{Step, Tour};

use super::threads::ThreadView;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Side {
    Base,
    Head,
}

/// Where a shown line lives. Comments and code links start from this.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LineLocation {
    /// GitHub addresses both sides of a renamed file by its new path.
    pub path: FilePath,
    pub side: Side,
    pub line: LineNumber,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    Removed,
    Added,
    /// Unchanged: its location is at head, and this is where it is at base. `None` when the
    /// hunks above it do not add up to a line, as in a diff that does not match the file.
    Context {
        base_line: Option<LineNumber>,
        /// Shown on the reviewer's ask, outside GitHub's diff, so GitHub takes no comment on it.
        is_expanded: bool,
    },
}

/// Which diff a row is drawn in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowView {
    /// The PR's diff against its base.
    PrDiff,
    /// The "change since your read" view, whose removed rows are lines of the head the step was
    /// read at, not of the PR's base. Its head rows are lines of the PR's head.
    ChangeSinceRead { head_line_is_in_pr_diff: bool },
}

#[derive(Debug, Clone)]
pub struct DiffLine {
    pub location: LineLocation,
    pub kind: LineKind,
    pub text: String,
    /// Empty until the file's highlights are ready.
    pub spans: Vec<Span>,
    /// Opened through the `CodeOpener`.
    pub code_link: CodeLink,
    pub view: RowView,
    /// Byte ranges of the words that changed, on a removed or added line that has a partner on the
    /// other side; empty when the whole line changed.
    pub changed: Vec<Range<usize>>,
}

impl DiffLine {
    /// A thread hangs under this row when it is on the row's own line, or, for an unchanged row,
    /// on the line it has at base.
    pub(super) fn holds(&self, location: &LineLocation) -> bool {
        self.takes_comment()
            && self.location.path == location.path
            && match (self.kind, location.side) {
                (LineKind::Context { base_line, .. }, Side::Base) => {
                    base_line == Some(location.line)
                }
                _ => self.location.side == location.side && self.location.line == location.line,
            }
    }

    pub fn is_change(&self) -> bool {
        matches!(self.kind, LineKind::Removed | LineKind::Added)
    }

    /// Only lines inside a GitHub diff hunk can take a comment.
    pub fn is_inside_hunk(&self) -> bool {
        !matches!(
            self.kind,
            LineKind::Context {
                is_expanded: true,
                ..
            }
        )
    }
}

impl DiffLine {
    /// The one rule for a comment button and for a selection: why GitHub takes no comment on this
    /// row, or `None` when it does. A head line keeps its number when it is commented on from the
    /// change since the read, because that view's head is the PR's head.
    pub fn comment_refusal(&self) -> Option<CommentRefusal> {
        match (self.view, self.location.side) {
            (RowView::PrDiff, _) => {
                (!self.is_inside_hunk()).then_some(CommentRefusal::NotInsideHunk)
            }
            (RowView::ChangeSinceRead { .. }, Side::Base) => Some(CommentRefusal::OlderHeadLine),
            (
                RowView::ChangeSinceRead {
                    head_line_is_in_pr_diff,
                },
                Side::Head,
            ) => (!head_line_is_in_pr_diff).then_some(CommentRefusal::NotInsideHunk),
        }
    }

    pub fn takes_comment(&self) -> bool {
        self.comment_refusal().is_none()
    }

    /// A row shows the comment button when it could be a line of a GitHub diff, so that pressing
    /// it can say why it takes no comment.
    pub fn offers_comment_button(&self) -> bool {
        match self.view {
            RowView::PrDiff => self.is_inside_hunk(),
            RowView::ChangeSinceRead { .. } => true,
        }
    }
}

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
}

pub struct StepPage<'a> {
    pub step: &'a Step,
    pub pieces: Vec<PieceDiff>,
    pub is_ticked: bool,
    /// Skim, ticked and gone steps show their text but keep their pieces closed.
    pub is_folded: bool,
    pub threads: Vec<ThreadView>,
    /// Pieces this step showed before the last re-pin that no longer exist.
    pub gone_pieces: usize,
    /// Nothing left to show: no pieces, no diagram, only gone pieces.
    pub is_gone: bool,
    /// A piece changed in the last re-pin, so the step's text was written for older code.
    pub text_may_be_old: bool,
    /// Set when the step was read at an older head: its pieces then show only what changed since.
    pub since_read: Option<SinceRead>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SinceRead {
    pub read_at: Sha,
    /// The reviewer asked for the whole PR diff of the step instead of the change since reading.
    pub showing_full_diff: bool,
}

impl<'a> StepPage<'a> {
    pub(super) fn of(
        step: &'a Step,
        tour: &Tour,
        files: &[ChangedFile],
        display: &DiffDisplay,
        is_folded: bool,
    ) -> Self {
        let pieces = step
            .pieces
            .iter()
            .flat_map(|pointer| coverage::shown_parts(pointer, tour, files))
            .filter_map(|part| PieceDiff::of(&part, files, display))
            .collect();
        Self {
            step,
            pieces,
            is_ticked: false,
            is_folded,
            threads: Vec::new(),
            gone_pieces: 0,
            is_gone: false,
            text_may_be_old: false,
            since_read: None,
        }
    }
}

/// The changes no step shows. Computed from the tour and the diff, never stored.
pub struct SafetyNetPage {
    pub pieces: Vec<PieceDiff>,
    pub threads: Vec<ThreadView>,
}

impl SafetyNetPage {
    pub(super) fn of(tour: &Tour, files: &[ChangedFile], display: &DiffDisplay) -> Self {
        Self {
            pieces: coverage::safety_net(tour, files)
                .into_iter()
                .filter_map(|part| PieceDiff::of(&part, files, display))
                .collect(),
            threads: Vec::new(),
        }
    }
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
        })
    }

    pub fn lines(&self) -> impl Iterator<Item = &DiffLine> {
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
        };
        let Some(file) = since_files
            .iter()
            .find(|file| file.path == *piece.id.path())
        else {
            return diff;
        };
        let added_range = piece.added_lines.first().zip(piece.added_lines.last());
        let changed_since: Vec<&Piece> = file
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
            .collect();
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

/// The pieces a step shows, in pointer order.
pub(super) fn pieces_of<'a>(step: &Step, tour: &Tour, files: &'a [ChangedFile]) -> Vec<&'a Piece> {
    step.pieces
        .iter()
        .flat_map(|pointer| coverage::shown_parts(pointer, tour, files))
        .map(|part| part.piece)
        .collect()
}
