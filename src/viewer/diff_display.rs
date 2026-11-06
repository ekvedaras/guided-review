use std::collections::{BTreeSet, HashMap};
use std::ops::RangeInclusive;

use crate::code_source::{CodeSource, FileAt, FileStatus, Line};
use crate::file_path::FilePath;
use crate::line_number::LineNumber;
use crate::pieces::{ChangedFile, PieceId};
use crate::pull_request::Sha;

use super::code_link::CodeLinks;
use super::highlight::Highlights;
use super::step_page::{DiffLine, LineKind, LineLocation, RowView, Side};

/// GitHub's diff shows this many unchanged lines around a change; only those take comments.
const GITHUB_CONTEXT_LINES: usize = 3;
const MORE_CONTEXT_LINES: usize = 20;

/// Whether GitHub shows this head line in the diff of `hunks`: in a hunk or in the context around it.
pub(super) fn is_in_github_diff(hunks: &[crate::code_source::Hunk], head_line: usize) -> bool {
    hunks.iter().any(|hunk| {
        head_line + GITHUB_CONTEXT_LINES > hunk.head_line_above()
            && head_line < hunk.head_line_below() + GITHUB_CONTEXT_LINES
    })
}

/// What to read at head for context lines, owned so it can move to a background thread.
#[derive(Debug, Clone)]
pub struct FilesAtHeadRequest {
    head: Sha,
    paths: Vec<FilePath>,
}

impl FilesAtHeadRequest {
    pub(super) fn for_files(files: &[ChangedFile], head: &Sha) -> Self {
        Self {
            head: head.clone(),
            paths: files
                .iter()
                .filter(|file| file.status != FileStatus::Deleted)
                .map(|file| file.path.clone())
                .collect(),
        }
    }

    /// A file that cannot be read shows its changes without context.
    pub fn read(&self, source: &dyn CodeSource) -> FilesAtHead {
        source.read_ahead(
            &self
                .paths
                .iter()
                .map(|path| FileAt {
                    sha: self.head.clone(),
                    path: path.clone(),
                })
                .collect::<Vec<_>>(),
        );
        let files = self
            .paths
            .iter()
            .filter_map(|path| {
                let text = source.file_at(&self.head, path).ok()?;
                Some((path.clone(), FileText::of(text)))
            })
            .collect();
        FilesAtHead {
            head: self.head.clone(),
            files,
        }
    }
}

#[derive(Debug, Default)]
pub struct FilesAtHead {
    head: Sha,
    files: HashMap<FilePath, FileText>,
}

/// A file's text and where each of its lines is, so the lines of every changed file are not each a `String`.
#[derive(Debug, Default)]
struct FileText {
    text: String,
    /// Byte ranges of the lines, without their line breaks, as `str::lines` cuts them.
    lines: Box<[(u32, u32)]>,
}

impl FileText {
    fn of(text: String) -> Self {
        let mut lines = Vec::new();
        let mut at = 0;
        for piece in text.split_inclusive('\n') {
            let line = piece
                .strip_suffix('\n')
                .map_or(piece, |line| line.strip_suffix('\r').unwrap_or(line));
            let (Ok(start), Ok(end)) = (u32::try_from(at), u32::try_from(at + line.len())) else {
                break;
            };
            lines.push((start, end));
            at += piece.len();
        }
        Self {
            text,
            lines: lines.into(),
        }
    }

    fn line_count(&self) -> usize {
        self.lines.len()
    }

    /// `number` counts from 1.
    fn line(&self, number: usize) -> Option<&str> {
        let &(start, end) = self.lines.get(number.checked_sub(1)?)?;
        self.text
            .get(usize::try_from(start).ok()?..usize::try_from(end).ok()?)
    }
}

impl FilesAtHead {
    pub(super) fn is_at(&self, head: &Sha) -> bool {
        self.head == *head
    }

    /// Lines `first` to `last` (1-based, both included) of a file at head; none when the file was not read.
    pub(super) fn lines(&self, path: &str, first: usize, last: usize) -> Vec<String> {
        self.files.get(path).map_or_else(Vec::new, |file| {
            (first.max(1)..=last)
                .map_while(|number| file.line(number))
                .map(str::to_owned)
                .collect()
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Edge {
    Above,
    Below,
}

/// The full diff of a piece and the change since read are shown apart, so asking for more lines in one leaves the other alone.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PieceView {
    Full(PieceId),
    SinceRead(PieceId),
}

/// The reviewer's ask for more unchanged lines on one edge of one change of a piece.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MoreContext {
    piece: PieceView,
    change: usize,
    pub edge: Edge,
}

/// The shown lines: removed and added lines of each change, with unchanged lines around them.
#[derive(Debug, Clone)]
pub struct DiffHunk {
    pub rows: Vec<DiffLine>,
    /// `None` at the top of the file or next to another change.
    pub more_above: Option<MoreContext>,
    pub more_below: Option<MoreContext>,
}

/// Everything that decides how a piece's lines are shown. None of it changes pieces, coverage or ticks.
pub(super) struct DiffDisplay<'a> {
    pub highlights: &'a Highlights,
    pub files_at_head: &'a FilesAtHead,
    pub more_context: &'a HashMap<MoreContext, usize>,
    pub ignores_whitespace: bool,
    pub links: &'a CodeLinks<'a>,
}

impl DiffDisplay<'_> {
    pub(super) fn more_lines(more_context: &mut HashMap<MoreContext, usize>, more: &MoreContext) {
        *more_context.entry(more.clone()).or_default() += MORE_CONTEXT_LINES;
    }

    pub(super) fn hunks(
        &self,
        view: &PieceView,
        file: &ChangedFile,
        removed: &[&Line],
        added: &[&Line],
        within: Option<RangeInclusive<usize>>,
    ) -> Vec<DiffHunk> {
        let file_lines = FileLines {
            file,
            at_head: self.files_at_head.files.get(&file.path),
            changed_at_head: file
                .hunks
                .iter()
                .flat_map(|hunk| hunk.added.iter().map(|line| usize::from(line.number)))
                .collect(),
            highlights: self.highlights,
            links: self.links,
            within,
        };
        let segments = file
            .hunks
            .iter()
            .filter_map(|hunk| {
                let in_hunk = |piece_lines: &[&Line], hunk_lines: &[Line]| -> Vec<Line> {
                    piece_lines
                        .iter()
                        .filter(|line| hunk_lines.iter().any(|l| l.number == line.number))
                        .map(|line| (*line).clone())
                        .collect()
                };
                let removed = in_hunk(removed, &hunk.removed);
                let added = in_hunk(added, &hunk.added);
                if removed.is_empty() && added.is_empty() {
                    return None;
                }
                let (line_above, line_below) = match (added.first(), added.last()) {
                    (Some(first), Some(last)) => {
                        (usize::from(first.number) - 1, usize::from(last.number) + 1)
                    }
                    _ => (hunk.head_line_above(), hunk.head_line_below()),
                };
                Some((removed, added, line_above, line_below))
            })
            .enumerate()
            .filter_map(|(change, (removed, added, line_above, line_below))| {
                let rows = self.change_rows(&file_lines, &removed, &added);
                if !rows.iter().any(DiffLine::is_change) {
                    return None;
                }
                let more = |edge| MoreContext {
                    piece: view.clone(),
                    change,
                    edge,
                };
                let wanted = |edge| {
                    GITHUB_CONTEXT_LINES + self.more_context.get(&more(edge)).copied().unwrap_or(0)
                };
                let above = file_lines.unchanged_above(line_above, wanted(Edge::Above));
                let below = file_lines.unchanged_below(line_below, wanted(Edge::Below));
                Some(Segment {
                    more_above: file_lines
                        .is_unchanged(above.first().copied().unwrap_or(line_above + 1) - 1)
                        .then(|| more(Edge::Above)),
                    more_below: file_lines
                        .is_unchanged(below.last().copied().unwrap_or(line_below - 1) + 1)
                        .then(|| more(Edge::Below)),
                    first_line: above.first().copied().unwrap_or(line_above + 1),
                    last_line: below.last().copied().unwrap_or(line_below - 1),
                    line_above,
                    line_below,
                    above,
                    rows,
                    below,
                })
            });

        let mut hunks: Vec<(DiffHunk, Segment)> = Vec::new();
        for segment in segments {
            match hunks.last_mut() {
                Some((hunk, previous)) if segment.first_line <= previous.last_line + 1 => {
                    hunk.rows.extend(
                        (previous.line_below..=segment.line_above)
                            .filter_map(|line| file_lines.context_row(line)),
                    );
                    hunk.rows.extend(segment.rows.iter().cloned());
                    hunk.more_below.clone_from(&segment.more_below);
                    *previous = segment;
                }
                _ => {
                    let rows = segment
                        .above
                        .iter()
                        .filter_map(|&line| file_lines.context_row(line))
                        .chain(segment.rows.iter().cloned())
                        .collect();
                    let hunk = DiffHunk {
                        rows,
                        more_above: segment.more_above.clone(),
                        more_below: segment.more_below.clone(),
                    };
                    hunks.push((hunk, segment));
                }
            }
        }
        hunks
            .into_iter()
            .map(|(mut hunk, last)| {
                hunk.rows.extend(
                    last.below
                        .iter()
                        .filter_map(|&line| file_lines.context_row(line)),
                );
                hunk
            })
            .collect()
    }

    /// With whitespace ignored, a removed and an added line that differ only in whitespace
    /// show as one unchanged line, like GitHub's "hide whitespace".
    fn change_rows(
        &self,
        file_lines: &FileLines,
        removed: &[Line],
        added: &[Line],
    ) -> Vec<DiffLine> {
        let mut same_as_removed: HashMap<LineNumber, LineNumber> = HashMap::new();
        if self.ignores_whitespace {
            let mut next_removed = 0;
            for line in added {
                let rest = removed.get(next_removed..).unwrap_or_default();
                if let Some((offset, old)) = rest.iter().enumerate().find(|(_, old)| {
                    without_whitespace(&old.text) == without_whitespace(&line.text)
                }) {
                    same_as_removed.insert(line.number, old.number);
                    next_removed += offset + 1;
                }
            }
        }
        let hidden_removed: BTreeSet<LineNumber> = same_as_removed.values().copied().collect();
        let removed_rows = removed
            .iter()
            .filter(|line| !hidden_removed.contains(&line.number))
            .map(|line| file_lines.changed_row(Side::Base, LineKind::Removed, line));
        let added_rows = added.iter().map(|line| {
            let kind = match same_as_removed.get(&line.number) {
                Some(&base_line) => LineKind::Context {
                    base_line: Some(base_line),
                    is_expanded: false,
                },
                None => LineKind::Added,
            };
            file_lines.changed_row(Side::Head, kind, line)
        });
        removed_rows.chain(added_rows).collect()
    }
}

fn without_whitespace(text: &str) -> String {
    text.chars().filter(|c| !c.is_whitespace()).collect()
}

/// One change of a piece with the unchanged head lines picked around it.
struct Segment {
    /// The head lines right above and below the change; context grows away from them.
    line_above: usize,
    line_below: usize,
    above: Vec<usize>,
    rows: Vec<DiffLine>,
    below: Vec<usize>,
    first_line: usize,
    last_line: usize,
    more_above: Option<MoreContext>,
    more_below: Option<MoreContext>,
}

struct FileLines<'a> {
    file: &'a ChangedFile,
    at_head: Option<&'a FileText>,
    changed_at_head: BTreeSet<usize>,
    highlights: &'a Highlights,
    links: &'a CodeLinks<'a>,
    /// A line range shows no unchanged lines outside it.
    within: Option<RangeInclusive<usize>>,
}

impl FileLines<'_> {
    fn is_unchanged(&self, line: usize) -> bool {
        self.at_head
            .is_some_and(|file| line >= 1 && line <= file.line_count())
            && !self.changed_at_head.contains(&line)
            && self
                .within
                .as_ref()
                .is_none_or(|range| range.contains(&line))
    }

    fn unchanged_above(&self, from: usize, wanted: usize) -> Vec<usize> {
        let mut lines: Vec<usize> = (1..=from)
            .rev()
            .take_while(|&line| self.is_unchanged(line))
            .take(wanted)
            .collect();
        lines.reverse();
        lines
    }

    fn unchanged_below(&self, from: usize, wanted: usize) -> Vec<usize> {
        (from..)
            .take_while(|&line| self.is_unchanged(line))
            .take(wanted)
            .collect()
    }

    /// An unchanged head line sits lower at head by what the hunks above it added, less what they removed.
    /// `None` when the hunks above do not add up to a line of the file.
    fn base_line_of(&self, head_line: usize) -> Option<LineNumber> {
        let line = self
            .file
            .hunks
            .iter()
            .filter(|hunk| hunk.head_line_below() <= head_line)
            .try_fold(head_line, |line, hunk| {
                line.checked_add(hunk.removed.len())?
                    .checked_sub(hunk.added.len())
            })?;
        LineNumber::try_from(line).ok()
    }

    /// `None` for line 0, which a file does not have.
    fn context_row(&self, head_line: usize) -> Option<DiffLine> {
        let number = LineNumber::try_from(head_line).ok()?;
        let text = self
            .at_head
            .and_then(|file| file.line(head_line))
            .map(str::to_owned)
            .unwrap_or_default();
        Some(self.changed_row(
            Side::Head,
            LineKind::Context {
                base_line: self.base_line_of(head_line),
                is_expanded: !is_in_github_diff(&self.file.hunks, head_line),
            },
            &Line { number, text },
        ))
    }

    fn changed_row(&self, side: Side, kind: LineKind, line: &Line) -> DiffLine {
        let location = LineLocation {
            path: self.file.path.clone(),
            side,
            line: line.number,
        };
        DiffLine {
            code_link: self.links.to_line(self.file, &location),
            location,
            kind,
            text: line.text.clone(),
            spans: self
                .highlights
                .spans(side, self.file.path.as_str(), line.number.into())
                .to_vec(),
            view: RowView::PrDiff,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{FileText, is_in_github_diff};
    use crate::code_source::{Hunk, Line};
    use crate::line_number::LineNumber;

    #[test]
    fn a_head_line_is_in_the_github_diff_within_three_lines_of_a_hunk() {
        let hunk = Hunk {
            header: "@@ -10 +10 @@".to_owned(),
            head_start: 10,
            removed: Vec::new(),
            added: vec![Line {
                number: LineNumber::try_from(10_usize).unwrap(),
                text: String::new(),
            }],
        };

        let shown: Vec<usize> = (1..=20)
            .filter(|line| is_in_github_diff(std::slice::from_ref(&hunk), *line))
            .collect();

        assert_eq!(shown, (7..=13).collect::<Vec<_>>());
    }

    #[test]
    fn a_file_text_cuts_lines_as_str_lines_does() {
        for text in ["", "a", "a\n", "a\r\nb", "a\n\nb\r\n", "é\nx\r", "\n\n"] {
            let file = FileText::of(text.to_owned());

            let lines: Vec<&str> = (1..=file.line_count())
                .filter_map(|n| file.line(n))
                .collect();
            assert_eq!(lines, text.lines().collect::<Vec<_>>(), "{text:?}");
            assert_eq!(file.line(0), None);
            assert_eq!(file.line(file.line_count() + 1), None);
        }
    }
}
