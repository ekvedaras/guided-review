use super::diff_display::{DiffDisplay, DiffHunk};
use super::github_link::GitHubLink;
use super::highlight::Span;
use crate::coverage::{self, PiecePart};
use crate::pieces::{ChangedFile, PieceId};
use crate::tour::{Step, Tour};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    Removed,
    Added,
    /// Unchanged: its location is at head, and this is where it is at base.
    Context {
        base_line: usize,
        /// Shown on the reviewer's ask, outside GitHub's diff, so GitHub takes no comment on it.
        is_expanded: bool,
    },
}

#[derive(Debug, Clone)]
pub struct DiffLine {
    pub location: LineLocation,
    pub kind: LineKind,
    pub text: String,
    /// Empty until the file's highlights are ready.
    pub spans: Vec<Span>,
    pub github_link: GitHubLink,
}

impl DiffLine {
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

#[derive(Debug, Clone)]
pub struct PieceDiff {
    pub id: PieceId,
    pub hunks: Vec<DiffHunk>,
    /// Its changes are all hidden because whitespace is ignored.
    pub changes_only_whitespace: bool,
    /// Set for a binary file: it has no lines, only this one row.
    pub binary_file: Option<GitHubLink>,
}

pub struct StepPage<'a> {
    pub step: &'a Step,
    pub pieces: Vec<PieceDiff>,
    pub is_ticked: bool,
    /// Skim and ticked steps show their text but keep their pieces closed.
    pub is_folded: bool,
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
            .flat_map(|pointer| pointer.shown_parts(tour, files))
            .map(|part| PieceDiff::of(part, files, display))
            .collect();
        Self {
            step,
            pieces,
            is_ticked: false,
            is_folded,
        }
    }
}

/// The changes no step shows. Computed from the tour and the diff, never stored.
pub struct SafetyNetPage {
    pub pieces: Vec<PieceDiff>,
}

impl SafetyNetPage {
    pub(super) fn of(tour: &Tour, files: &[ChangedFile], display: &DiffDisplay) -> Self {
        Self {
            pieces: coverage::safety_net(tour, files)
                .into_iter()
                .map(|part| PieceDiff::of(part, files, display))
                .collect(),
        }
    }
}

impl PieceDiff {
    fn of(part: PiecePart, files: &[ChangedFile], display: &DiffDisplay) -> Self {
        let piece = part.piece;
        let file = files
            .iter()
            .find(|file| file.path == piece.id.path)
            .expect("a piece belongs to a changed file");
        let hunks = display.hunks(
            &piece.id,
            file,
            &part.removed_lines,
            &part.added_lines,
            part.range
                .map(|[first, last]| first as usize..=last as usize),
        );
        let has_changes = !part.removed_lines.is_empty() || !part.added_lines.is_empty();
        Self {
            id: piece.id.clone(),
            changes_only_whitespace: has_changes && hunks.is_empty(),
            hunks,
            binary_file: file.binary.then(|| display.links.to_file(file)),
        }
    }

    pub fn lines(&self) -> impl Iterator<Item = &DiffLine> {
        self.hunks.iter().flat_map(|hunk| hunk.rows.iter())
    }
}
