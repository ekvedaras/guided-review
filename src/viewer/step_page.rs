use super::github_link::{GitHubLink, GitHubLinks};
use super::highlight::{Highlights, Span};
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

#[derive(Debug, Clone)]
pub struct DiffLine {
    pub location: LineLocation,
    pub text: String,
    /// Empty until the file's highlights are ready.
    pub spans: Vec<Span>,
    pub github_link: GitHubLink,
}

#[derive(Debug, Clone)]
pub struct PieceDiff {
    pub id: PieceId,
    pub lines: Vec<DiffLine>,
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
        links: &GitHubLinks,
        highlights: &Highlights,
        is_folded: bool,
    ) -> Self {
        let pieces = step
            .pieces
            .iter()
            .flat_map(|pointer| pointer.shown_parts(tour, files))
            .map(|part| PieceDiff::of(part, files, links, highlights))
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
    pub(super) fn of(
        tour: &Tour,
        files: &[ChangedFile],
        links: &GitHubLinks,
        highlights: &Highlights,
    ) -> Self {
        Self {
            pieces: coverage::safety_net(tour, files)
                .into_iter()
                .map(|part| PieceDiff::of(part, files, links, highlights))
                .collect(),
        }
    }
}

impl PieceDiff {
    fn of(
        part: PiecePart,
        files: &[ChangedFile],
        links: &GitHubLinks,
        highlights: &Highlights,
    ) -> Self {
        let piece = part.piece;
        let file = files
            .iter()
            .find(|file| file.path == piece.id.path)
            .expect("a piece belongs to a changed file");
        let on_side = |side: Side| {
            move |line: &crate::code_source::Line| {
                let location = LineLocation {
                    path: piece.id.path.clone(),
                    side,
                    line: line.number,
                };
                DiffLine {
                    github_link: links.to_line(file, &location),
                    spans: highlights.spans(side, &piece.id.path, line.number).to_vec(),
                    location,
                    text: line.text.clone(),
                }
            }
        };
        let lines = part
            .removed_lines
            .into_iter()
            .map(on_side(Side::Base))
            .chain(part.added_lines.into_iter().map(on_side(Side::Head)))
            .collect();
        Self {
            id: piece.id.clone(),
            lines,
            binary_file: file.binary.then(|| links.to_file(file)),
        }
    }
}
