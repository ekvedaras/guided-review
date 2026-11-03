use std::collections::{BTreeSet, HashMap};
use std::ops::RangeInclusive;

use crate::code_source::{CodeSource, FileAt, FileStatus, Line};
use crate::pieces::{ChangedFile, PieceId};
use crate::pull_request::Sha;

use super::code_link::CodeLinks;
use super::highlight::Highlights;
use super::step_page::{DiffLine, LineKind, LineLocation, Side};

/// GitHub's diff shows this many unchanged lines around a change; only those take comments.
const GITHUB_CONTEXT_LINES: usize = 3;
const MORE_CONTEXT_LINES: usize = 20;

/// What to read at head for context lines, owned so it can move to a background thread.
#[derive(Debug, Clone)]
pub struct FilesAtHeadRequest {
    head: Sha,
    paths: Vec<String>,
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
                Some((path.clone(), text.lines().map(str::to_string).collect()))
            })
            .collect();
        FilesAtHead { files }
    }
}

#[derive(Debug, Default)]
pub struct FilesAtHead {
    files: HashMap<String, Vec<String>>,
}

impl FilesAtHead {
    /// Lines `first` to `last` (1-based, both included) of a file at head; none when the file was not read.
    pub(super) fn lines(&self, path: &str, first: usize, last: usize) -> Vec<String> {
        self.files.get(path).map_or_else(Vec::new, |lines| {
            lines
                .iter()
                .skip(first.saturating_sub(1))
                .take((last + 1).saturating_sub(first))
                .cloned()
                .collect()
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Edge {
    Above,
    Below,
}

/// The reviewer's ask for more unchanged lines on one edge of one change of a piece.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MoreContext {
    piece: String,
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
        id: &PieceId,
        file: &ChangedFile,
        removed: &[&Line],
        added: &[&Line],
        within: Option<RangeInclusive<usize>>,
    ) -> Vec<DiffHunk> {
        let file_lines = FileLines {
            file,
            at_head: self.files_at_head.files.get(&file.path).map(Vec::as_slice),
            changed_at_head: file
                .hunks
                .iter()
                .flat_map(|hunk| hunk.added.iter().map(|line| line.number))
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
                    (Some(first), Some(last)) => (first.number - 1, last.number + 1),
                    _ => (hunk.head_line_above(), hunk.head_line_below()),
                };
                Some((removed, added, line_above, line_below))
            })
            .enumerate()
            .filter_map(|(change, (removed, added, line_above, line_below))| {
                let rows = self.change_rows(&file_lines, removed, added);
                if !rows.iter().any(DiffLine::is_change) {
                    return None;
                }
                let more = |edge| MoreContext {
                    piece: id.to_string(),
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
                            .map(|line| file_lines.context_row(line)),
                    );
                    hunk.rows.extend(segment.rows.iter().cloned());
                    hunk.more_below = segment.more_below.clone();
                    *previous = segment;
                }
                _ => {
                    let rows = segment
                        .above
                        .iter()
                        .map(|&line| file_lines.context_row(line))
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
                hunk.rows
                    .extend(last.below.iter().map(|&line| file_lines.context_row(line)));
                hunk
            })
            .collect()
    }

    /// With whitespace ignored, a removed and an added line that differ only in whitespace
    /// show as one unchanged line, like GitHub's "hide whitespace".
    fn change_rows(
        &self,
        file_lines: &FileLines,
        removed: Vec<Line>,
        added: Vec<Line>,
    ) -> Vec<DiffLine> {
        let mut same_as_removed: HashMap<usize, usize> = HashMap::new();
        if self.ignores_whitespace {
            let mut next_removed = 0;
            for line in &added {
                let rest = &removed[next_removed..];
                if let Some(offset) = rest
                    .iter()
                    .position(|old| without_whitespace(&old.text) == without_whitespace(&line.text))
                {
                    same_as_removed.insert(line.number, rest[offset].number);
                    next_removed += offset + 1;
                }
            }
        }
        let hidden_removed: BTreeSet<usize> = same_as_removed.values().copied().collect();
        let removed_rows = removed
            .iter()
            .filter(|line| !hidden_removed.contains(&line.number))
            .map(|line| file_lines.changed_row(Side::Base, LineKind::Removed, line));
        let added_rows = added.iter().map(|line| {
            let kind = match same_as_removed.get(&line.number) {
                Some(&base_line) => LineKind::Context {
                    base_line,
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
    at_head: Option<&'a [String]>,
    changed_at_head: BTreeSet<usize>,
    highlights: &'a Highlights,
    links: &'a CodeLinks<'a>,
    /// A line range shows no unchanged lines outside it.
    within: Option<RangeInclusive<usize>>,
}

impl FileLines<'_> {
    fn is_unchanged(&self, line: usize) -> bool {
        self.at_head
            .is_some_and(|lines| line >= 1 && line <= lines.len())
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
    fn base_line_of(&self, head_line: usize) -> usize {
        self.file
            .hunks
            .iter()
            .filter(|hunk| hunk.head_line_below() <= head_line)
            .fold(head_line, |line, hunk| {
                line + hunk.removed.len() - hunk.added.len()
            })
    }

    fn is_in_github_diff(&self, head_line: usize) -> bool {
        self.file.hunks.iter().any(|hunk| {
            head_line + GITHUB_CONTEXT_LINES > hunk.head_line_above()
                && head_line < hunk.head_line_below() + GITHUB_CONTEXT_LINES
        })
    }

    fn context_row(&self, head_line: usize) -> DiffLine {
        let text = self
            .at_head
            .and_then(|lines| lines.get(head_line - 1))
            .cloned()
            .unwrap_or_default();
        self.changed_row(
            Side::Head,
            LineKind::Context {
                base_line: self.base_line_of(head_line),
                is_expanded: !self.is_in_github_diff(head_line),
            },
            &Line {
                number: head_line,
                text,
            },
        )
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
                .spans(side, &self.file.path, line.number)
                .to_vec(),
            is_in_change_since_read: false,
        }
    }
}
