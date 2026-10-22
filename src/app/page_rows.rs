//! The screen as rows of one `gpui::list`, built from the review once per change and kept until the next
//! one, so a frame only draws the rows on screen and builds nothing.
//!
//! Rule for every change of the review: build new `PageRows` and tell the list what `change_from` says.
//! A row's key holds what decides its identity and height; rows whose key stayed keep their measured height
//! and the reader's scroll position, rows with a new key are measured again. So a row's look may change
//! freely (colours, ask counts, the drag selection), but whatever changes its height must be in its key.
//! Rows on screen are measured on every frame anyway, so the key matters for the rows off screen.

use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::ops::{Range, RangeInclusive};
use std::rc::Rc;

use gpui::SharedString;

use crate::app::components::ask_panel::lines_label;
use crate::app::components::diff_view::DiffRowLine;
use crate::app::components::step_notes::StepNotes;
use crate::repin::RepinStatus;
use crate::viewer::{
    CommentPlace, GitHubLink, JumpKind, JumpTarget, LineKind, LineLocation, MoreContext, PieceDiff,
    Place, QueuedComment, Review, RowSelection, SafetyNetPage, Screen, StepId, StepPage,
    ThreadView,
};

/// An ask button and what it asks about.
pub struct AskAt {
    pub id: SharedString,
    pub place: Place,
    pub label: String,
}

pub struct StepHeading {
    pub id: StepId,
    pub title: SharedString,
    pub is_ticked: bool,
    /// A step whose pieces all went away in a re-pin.
    pub is_gone: bool,
    /// Skim and ticked steps fold by a click on their title.
    pub folds_on_title_click: bool,
}

pub struct PieceHeading {
    pub piece: SharedString,
    pub path: String,
    pub ask: Option<AskAt>,
    /// "changed" or "new" after a re-pin.
    pub repin_label: Option<&'static str>,
    pub open_at: Option<LineLocation>,
}

#[derive(Clone)]
pub enum PageRow {
    SummaryTitle(SharedString),
    SummaryBullet(SharedString),
    StepHeading(Rc<StepHeading>),
    StepWhy(SharedString),
    /// Its height follows the drawing; the layout re-measures it when the drawing changes.
    Diagram,
    Ask(Rc<AskAt>),
    StepNotes(StepNotes),
    WhitespaceToggle {
        is_on: bool,
    },
    RepinNotes {
        gone_pieces: usize,
        text_may_be_old: bool,
    },
    FullDiffToggle {
        step: StepId,
        is_showing_full_diff: bool,
    },
    FoldToggle {
        step: StepId,
        is_folded: bool,
        piece_count: usize,
    },
    SafetyNetIntro,
    PieceHeader(Rc<PieceHeading>),
    /// In the change since the read, a piece without lines was not touched since.
    NoChangeSinceRead,
    BinaryFile(GitHubLink),
    /// The pictures of an image file; comments on the whole file hang under it.
    Image {
        path: SharedString,
        is_loaded: bool,
    },
    OnlyWhitespace,
    HunkGap {
        id: SharedString,
        more: Option<MoreContext>,
    },
    Line(Rc<DiffRowLine>),
    /// Under its line inside the piece box, or after the code.
    Thread {
        view: Rc<ThreadView>,
        is_under_a_line: bool,
    },
    QueuedComment(Rc<QueuedComment>),
    /// A comment box; its text is read from the review when drawn.
    Composer(Rc<CommentPlace>),
    /// The ask button under the last of the lines that have Asks.
    AskMarker(Rc<AskAt>),
    PieceEnd,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RowKey(u64);

impl PageRow {
    fn key(&self) -> RowKey {
        let mut hasher = DefaultHasher::new();
        std::mem::discriminant(self).hash(&mut hasher);
        match self {
            Self::SummaryTitle(text) | Self::SummaryBullet(text) | Self::StepWhy(text) => {
                text.hash(&mut hasher)
            }
            Self::StepHeading(heading) => {
                heading.id.to_string().hash(&mut hasher);
                heading.title.hash(&mut hasher);
            }
            Self::Ask(ask) | Self::AskMarker(ask) => ask.id.hash(&mut hasher),
            Self::StepNotes(notes) => notes.hash(&mut hasher),
            Self::RepinNotes {
                gone_pieces,
                text_may_be_old,
            } => {
                gone_pieces.hash(&mut hasher);
                text_may_be_old.hash(&mut hasher);
            }
            Self::FullDiffToggle {
                step,
                is_showing_full_diff,
            } => {
                step.to_string().hash(&mut hasher);
                is_showing_full_diff.hash(&mut hasher);
            }
            Self::FoldToggle {
                step, is_folded, ..
            } => {
                step.to_string().hash(&mut hasher);
                is_folded.hash(&mut hasher);
            }
            Self::PieceHeader(heading) => {
                heading.piece.hash(&mut hasher);
                heading.repin_label.hash(&mut hasher);
            }
            Self::BinaryFile(link) => link.as_str().hash(&mut hasher),
            Self::Image { path, is_loaded } => {
                path.hash(&mut hasher);
                is_loaded.hash(&mut hasher);
            }
            Self::HunkGap { id, more } => {
                id.hash(&mut hasher);
                more.is_some().hash(&mut hasher);
            }
            Self::Line(line) => {
                line.location.path.hash(&mut hasher);
                line.location.side.hash(&mut hasher);
                line.location.line.hash(&mut hasher);
                matches!(line.kind, LineKind::Context { .. }).hash(&mut hasher);
            }
            Self::Thread {
                view,
                is_under_a_line,
            } => {
                view.thread.id.hash(&mut hasher);
                view.is_folded.hash(&mut hasher);
                view.thread.comments.len().hash(&mut hasher);
                view.reply_draft.hash(&mut hasher);
                is_under_a_line.hash(&mut hasher);
            }
            Self::QueuedComment(queued) => {
                queued.draft.place.hash(&mut hasher);
                queued.failed_because.hash(&mut hasher);
            }
            Self::Composer(place) => place.hash(&mut hasher),
            Self::Diagram
            | Self::WhitespaceToggle { .. }
            | Self::SafetyNetIntro
            | Self::NoChangeSinceRead
            | Self::OnlyWhitespace
            | Self::PieceEnd => {}
        }
        RowKey(hasher.finish())
    }
}

/// The rows a drag selected, as rows of the list: those in `rows` that show a line of `path`.
pub struct SelectedRows {
    rows: RangeInclusive<usize>,
    path: String,
}

impl SelectedRows {
    pub fn contains(&self, row: usize, line: &LineLocation) -> bool {
        self.rows.contains(&row) && line.path == self.path
    }
}

struct PieceAt {
    id: String,
    path: String,
    row: usize,
}

pub struct PageRows {
    screen: Option<Screen>,
    rows: Vec<PageRow>,
    keys: Vec<RowKey>,
    diagram: Option<SharedString>,
    asked_lines: Vec<Place>,
    line_rows: HashMap<LineLocation, usize>,
    pieces: Vec<PieceAt>,
}

/// What the list must do to show new rows in place of old ones.
#[derive(Debug, PartialEq, Eq)]
pub enum RowsChange {
    /// Another screen, or another review: start at its top.
    OtherScreen,
    Unchanged,
    /// The rows in `old` became `new_count` rows; the rest kept their place and height.
    Spliced {
        old: Range<usize>,
        new_count: usize,
    },
}

impl PageRows {
    /// Rows of no screen, so whatever comes next starts at its top.
    pub fn none() -> Self {
        Self {
            screen: None,
            rows: Vec::new(),
            keys: Vec::new(),
            diagram: None,
            asked_lines: Vec::new(),
            line_rows: HashMap::new(),
            pieces: Vec::new(),
        }
    }

    /// `asked_lines` are the `Place::Lines` of this screen that have Asks.
    pub fn of(review: &Review, asked_lines: Vec<Place>) -> Self {
        let screen = review.screen().clone();
        let mut builder = RowsBuilder {
            rows: Vec::new(),
            under_lines: Vec::new(),
        };
        let mut diagram = None;
        match &screen {
            Screen::Summary => summary_rows(review, &mut builder.rows),
            Screen::Step(_) => {
                if let Some(page) = review.current_step() {
                    builder.under_lines = under_lines(review, &asked_lines);
                    diagram = builder.step_rows(page, review);
                }
            }
            Screen::SafetyNet => {
                if let Some(page) = review.safety_net() {
                    builder.under_lines = under_lines(review, &asked_lines);
                    builder.safety_net_rows(page, review);
                }
            }
        }
        let rows = builder.rows;
        let keys = rows.iter().map(PageRow::key).collect();
        let mut line_rows = HashMap::new();
        let mut pieces = Vec::new();
        for (index, row) in rows.iter().enumerate() {
            match row {
                PageRow::Line(line) => {
                    line_rows.entry(line.location.clone()).or_insert(index);
                }
                PageRow::PieceHeader(heading) => pieces.push(PieceAt {
                    id: heading.piece.to_string(),
                    path: heading.path.clone(),
                    row: index,
                }),
                _ => {}
            }
        }
        Self {
            screen: Some(screen),
            rows,
            keys,
            diagram,
            asked_lines,
            line_rows,
            pieces,
        }
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn row(&self, index: usize) -> Option<&PageRow> {
        self.rows.get(index)
    }

    /// The mermaid source of the diagram this screen shows, if it shows one.
    pub fn diagram(&self) -> Option<&str> {
        self.diagram.as_deref()
    }

    pub fn diagram_row(&self) -> Option<usize> {
        self.rows
            .iter()
            .position(|row| matches!(row, PageRow::Diagram))
    }

    pub fn asked_lines(&self) -> &[Place] {
        &self.asked_lines
    }

    pub fn has_images_to_load(&self) -> bool {
        self.rows.iter().any(|row| {
            matches!(
                row,
                PageRow::Image {
                    is_loaded: false,
                    ..
                }
            )
        })
    }

    /// The first row that shows the line.
    pub fn row_of_line(&self, line: &LineLocation) -> Option<usize> {
        self.line_rows.get(line).copied()
    }

    /// The row a jump lands on: a file's first piece, a symbol's piece, else the step's top.
    pub fn row_of_jump(&self, target: &JumpTarget) -> usize {
        let piece = match target.kind {
            JumpKind::Step => None,
            JumpKind::File => self.pieces.iter().find(|piece| piece.path == target.label),
            JumpKind::Symbol => self.pieces.iter().find(|piece| piece.id == target.label),
        };
        piece.map_or(0, |piece| piece.row)
    }

    pub fn selected_rows(&self, selection: &RowSelection) -> Option<SelectedRows> {
        let anchor = self.row_of_line(selection.anchor())?;
        let head = self.row_of_line(selection.head())?;
        Some(SelectedRows {
            rows: anchor.min(head)..=anchor.max(head),
            path: selection.anchor().path.clone(),
        })
    }

    pub fn change_from(&self, old: &PageRows) -> RowsChange {
        if self.screen != old.screen {
            return RowsChange::OtherScreen;
        }
        changed_span(&old.keys, &self.keys)
    }
}

fn changed_span(old: &[RowKey], new: &[RowKey]) -> RowsChange {
    let same_start = old.iter().zip(new).take_while(|(a, b)| a == b).count();
    let same_end = old[same_start..]
        .iter()
        .rev()
        .zip(new[same_start..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let old_changed = same_start..old.len() - same_end;
    let new_count = new.len() - same_end - same_start;
    if old_changed.is_empty() && new_count == 0 {
        RowsChange::Unchanged
    } else {
        RowsChange::Spliced {
            old: old_changed,
            new_count,
        }
    }
}

fn summary_rows(review: &Review, rows: &mut Vec<PageRow>) {
    rows.push(PageRow::SummaryTitle(review.title().to_string().into()));
    rows.push(PageRow::Ask(Rc::new(AskAt {
        id: "ask-tour".into(),
        place: Place::Tour,
        label: "the whole tour".into(),
    })));
    rows.extend(
        review
            .summary()
            .iter()
            .map(|bullet| PageRow::SummaryBullet(format!("• {bullet}").into())),
    );
}

/// What hangs under a row of code, in this order: threads, queued comments, comment boxes, ask markers.
enum UnderLine {
    Thread(ThreadView),
    Queued(QueuedComment),
    Composer(CommentPlace),
    AskMarker(AskAt),
}

impl UnderLine {
    fn at(&self) -> Option<&LineLocation> {
        match self {
            Self::Thread(view) => view.anchor.as_ref(),
            Self::Queued(queued) => Some(&queued.draft.place.end),
            Self::Composer(place) => Some(&place.end),
            Self::AskMarker(ask) => match &ask.place {
                Place::Lines { end, .. } => Some(end),
                _ => None,
            },
        }
    }

    fn into_row(self) -> PageRow {
        match self {
            Self::Thread(view) => PageRow::Thread {
                view: Rc::new(view),
                is_under_a_line: true,
            },
            Self::Queued(queued) => PageRow::QueuedComment(Rc::new(queued)),
            Self::Composer(place) => PageRow::Composer(Rc::new(place)),
            Self::AskMarker(ask) => PageRow::AskMarker(Rc::new(ask)),
        }
    }
}

/// Comments and asks of the screen; threads come with the page.
fn under_lines(review: &Review, asked_lines: &[Place]) -> Vec<UnderLine> {
    let queued = review
        .queued_comments()
        .iter()
        .cloned()
        .map(UnderLine::Queued);
    let composers = review
        .comment_drafts()
        .iter()
        .map(|draft| UnderLine::Composer(draft.place.clone()));
    let markers = asked_lines.iter().filter_map(|place| {
        let Place::Lines { start, end, .. } = place else {
            return None;
        };
        Some(UnderLine::AskMarker(AskAt {
            id: format!("ask-lines-{}-{}-{}", end.path, start.line, end.line).into(),
            place: place.clone(),
            label: lines_label(start, end),
        }))
    });
    queued.chain(composers).chain(markers).collect()
}

struct RowsBuilder {
    rows: Vec<PageRow>,
    /// Taken out as their row is placed; what no row takes is dropped.
    under_lines: Vec<UnderLine>,
}

impl RowsBuilder {
    /// Returns the diagram the step shows.
    fn step_rows(&mut self, page: StepPage<'_>, review: &Review) -> Option<SharedString> {
        let step = page.step;
        let id = StepId::from(step.id.as_str());
        self.rows.push(PageRow::StepHeading(Rc::new(StepHeading {
            id: id.clone(),
            title: step.title.clone().into(),
            is_ticked: page.is_ticked,
            is_gone: page.is_gone,
            folds_on_title_click: page.is_ticked || step.skim,
        })));
        self.rows.push(PageRow::StepWhy(step.why.clone().into()));
        let diagram = step
            .diagram
            .as_ref()
            .filter(|_| !page.is_folded)
            .map(|source| SharedString::from(source.clone()));
        if diagram.is_some() {
            self.rows.push(PageRow::Diagram);
        }
        self.rows.push(PageRow::Ask(Rc::new(AskAt {
            id: format!("ask-step-{id}").into(),
            place: Place::Step { step: id.clone() },
            label: step.title.clone(),
        })));
        let notes = StepNotes::of(step);
        if !notes.is_empty() {
            self.rows.push(PageRow::StepNotes(notes));
        }
        self.rows.push(PageRow::WhitespaceToggle {
            is_on: review.ignores_whitespace(),
        });
        if page.gone_pieces > 0 || page.text_may_be_old {
            self.rows.push(PageRow::RepinNotes {
                gone_pieces: page.gone_pieces,
                text_may_be_old: page.text_may_be_old,
            });
        }
        if let Some(since) = &page.since_read {
            self.rows.push(PageRow::FullDiffToggle {
                step: id.clone(),
                is_showing_full_diff: since.showing_full_diff,
            });
        }
        if page.is_folded || step.skim || page.is_ticked {
            self.rows.push(PageRow::FoldToggle {
                step: id.clone(),
                is_folded: page.is_folded,
                piece_count: page.pieces.len(),
            });
        }
        if page.is_folded {
            return diagram;
        }
        let shows_change_since_read = page
            .since_read
            .as_ref()
            .is_some_and(|since| !since.showing_full_diff);
        let (under_a_line, after_code): (Vec<_>, Vec<_>) = page
            .threads
            .into_iter()
            .partition(|thread| thread.anchor.is_some());
        self.add_threads_under_lines(under_a_line);
        for piece in page.pieces {
            let ask = AskAt {
                id: format!("ask-piece-{id}-{}", piece.id).into(),
                place: Place::Piece {
                    step: id.clone(),
                    piece: piece.id.to_string(),
                },
                label: piece.id.to_string(),
            };
            self.piece_rows(piece, Some(ask), shows_change_since_read, review);
        }
        self.rows
            .extend(after_code.into_iter().map(thread_after_code));
        diagram
    }

    fn safety_net_rows(&mut self, page: SafetyNetPage, review: &Review) {
        self.rows.push(PageRow::SafetyNetIntro);
        let (under_a_line, after_code): (Vec<_>, Vec<_>) = page
            .threads
            .into_iter()
            .partition(|thread| thread.anchor.is_some());
        self.add_threads_under_lines(under_a_line);
        for piece in page.pieces {
            self.piece_rows(piece, None, false, review);
        }
        self.rows
            .extend(after_code.into_iter().map(thread_after_code));
    }

    fn add_threads_under_lines(&mut self, threads: Vec<ThreadView>) {
        let rest = std::mem::take(&mut self.under_lines);
        self.under_lines = threads
            .into_iter()
            .map(UnderLine::Thread)
            .chain(rest)
            .collect();
    }

    fn piece_rows(
        &mut self,
        piece: PieceDiff,
        ask: Option<AskAt>,
        shows_change_since_read: bool,
        review: &Review,
    ) {
        let id = piece.id.to_string();
        let no_change_since_read =
            shows_change_since_read && piece.hunks.is_empty() && piece.binary_file.is_none();
        self.rows.push(PageRow::PieceHeader(Rc::new(PieceHeading {
            piece: id.clone().into(),
            path: piece.id.path.clone(),
            ask,
            repin_label: piece.repin_status.and_then(repin_label),
            open_at: piece.open_at().cloned(),
        })));
        if no_change_since_read {
            self.rows.push(PageRow::NoChangeSinceRead);
        }
        if let Some(link) = piece.binary_file {
            self.rows.push(PageRow::BinaryFile(link));
        }
        if piece.is_image {
            let path = piece.id.path.clone();
            self.rows.push(PageRow::Image {
                is_loaded: review.image_preview(&path).is_some(),
                path: path.clone().into(),
            });
            self.hang_under(|at| at.path == path && at.line == 0);
        }
        if piece.changes_only_whitespace {
            self.rows.push(PageRow::OnlyWhitespace);
        }
        for (index, hunk) in piece.hunks.into_iter().enumerate() {
            if index > 0 || hunk.more_above.is_some() {
                self.rows.push(PageRow::HunkGap {
                    id: format!("{id}-above-{index}").into(),
                    more: hunk.more_above,
                });
            }
            for line in hunk.rows {
                let location = line.location.clone();
                self.rows
                    .push(PageRow::Line(Rc::new(DiffRowLine::of(line))));
                self.hang_under(|at| *at == location);
            }
            if hunk.more_below.is_some() {
                self.rows.push(PageRow::HunkGap {
                    id: format!("{id}-below-{index}").into(),
                    more: hunk.more_below,
                });
            }
        }
        self.rows.push(PageRow::PieceEnd);
    }

    fn hang_under(&mut self, is_here: impl Fn(&LineLocation) -> bool) {
        let (here, later): (Vec<_>, Vec<_>) = std::mem::take(&mut self.under_lines)
            .into_iter()
            .partition(|item| item.at().is_some_and(&is_here));
        self.under_lines = later;
        self.rows.extend(here.into_iter().map(UnderLine::into_row));
    }
}

fn repin_label(status: RepinStatus) -> Option<&'static str> {
    match status {
        RepinStatus::Changed => Some("changed"),
        RepinStatus::New => Some("new"),
        RepinStatus::Same | RepinStatus::Gone => None,
    }
}

fn thread_after_code(view: ThreadView) -> PageRow {
    PageRow::Thread {
        view: Rc::new(view),
        is_under_a_line: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(ids: &[u64]) -> Vec<RowKey> {
        ids.iter().copied().map(RowKey).collect()
    }

    #[test]
    fn same_rows_need_no_change() {
        assert_eq!(
            changed_span(&keys(&[1, 2, 3]), &keys(&[1, 2, 3])),
            RowsChange::Unchanged
        );
    }

    #[test]
    fn rows_shown_in_the_middle_splice_only_there() {
        assert_eq!(
            changed_span(&keys(&[1, 2, 3, 4]), &keys(&[1, 2, 7, 8, 9, 3, 4])),
            RowsChange::Spliced {
                old: 2..2,
                new_count: 3
            }
        );
    }

    #[test]
    fn a_changed_row_is_spliced_alone() {
        assert_eq!(
            changed_span(&keys(&[1, 2, 3]), &keys(&[1, 5, 3])),
            RowsChange::Spliced {
                old: 1..2,
                new_count: 1
            }
        );
    }

    #[test]
    fn repeated_rows_are_not_counted_twice() {
        assert_eq!(
            changed_span(&keys(&[1, 1]), &keys(&[1])),
            RowsChange::Spliced {
                old: 1..2,
                new_count: 0
            }
        );
    }
}
