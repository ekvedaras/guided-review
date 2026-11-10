//! The screen as rows of one `gpui::list`, built from the review once per change and kept until the next
//! one, so a frame only draws the rows on screen and builds nothing.
//!
//! Rule for every change of the review: build new `PageRows` and tell the list what `change_from` says.
//! A row's key holds what decides its identity and height; rows whose key stayed keep their measured height
//! and the reader's scroll position, rows with a new key are measured again. So a row's look may change
//! freely (colours, ask counts, the drag selection), but whatever changes its height must be in its key.
//! Rows on screen are measured on every frame anyway, so the key matters for the rows off screen.

use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::ops::{Range, RangeInclusive};
use std::rc::Rc;

use gpui::SharedString;

use crate::app::components::diff_view::{DiffRowLine, PieceLabel};
use crate::app::components::markdown_view::{OpenDetails, PreparedMarkdown};
use crate::app::components::step_notes::StepNotes;
use crate::file_path::FilePath;
use crate::pieces::PieceId;
use crate::repin::RepinStatus;
use crate::viewer::{
    CodeLink, CommentPlace, CommentProblem, FileBlock, JumpPlace, JumpTarget, LineKind,
    LineLocation, MoreContext, NextScreen, NotesButton, PieceDiff, Place, QueuedComment, Review,
    RowSelection, SafetyNetPage, Screen, StepId, StepPage, ThreadView,
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
    /// "Step 3 of 10"; `None` when the step is not in the reading order.
    pub number: Option<(usize, usize)>,
    pub is_ticked: bool,
    /// A step whose pieces all went away in a re-pin.
    pub is_gone: bool,
    /// Skim and ticked steps fold by a click on their title.
    pub folds_on_title_click: bool,
}

/// The controls over a screen's pieces, in one row.
pub struct Toolbar {
    pub ask: Option<AskAt>,
    pub fold: Option<FoldControl>,
    pub full_diff: Option<FullDiffControl>,
    /// `None` while the pieces are folded away, as whitespace changes nothing then.
    pub ignores_whitespace: Option<bool>,
    /// `None` while the pieces are folded away.
    pub wraps_lines: Option<bool>,
}

#[derive(Hash)]
pub struct FoldControl {
    pub step: StepId,
    pub is_folded: bool,
    pub piece_count: usize,
}

#[derive(Hash)]
pub struct FullDiffControl {
    pub step: StepId,
    pub is_showing_full_diff: bool,
}

/// The way on at the end of a screen.
#[derive(Hash)]
pub struct ScreenFooter {
    /// The step that is ticked on the way on: the open step, if not read yet.
    pub ticks: Option<StepId>,
    pub has_previous: bool,
    pub next: Option<NextScreen>,
    pub is_summary: bool,
}

/// The fold of one file box of a step.
#[derive(Clone, Hash)]
pub struct BoxFold {
    pub step: StepId,
    pub path: FilePath,
    pub is_folded: bool,
}

pub struct PieceHeading {
    /// Set on the header of a file box in a step; the safety net does not fold.
    pub fold: Option<BoxFold>,
    pub piece: PieceId,
    pub label: PieceLabel,
    pub path: FilePath,
    pub ask: Option<AskAt>,
    /// "changed" or "new" after a re-pin.
    pub repin_label: Option<&'static str>,
    pub code_link: Option<CodeLink>,
}

pub struct BinaryFileRow {
    pub piece: SharedString,
    pub link: CodeLink,
}

pub struct DiagramRow {
    pub step: StepId,
    pub notes_button: NotesButton,
}

pub struct HunkGapRow {
    pub id: SharedString,
    pub more: Option<MoreContext>,
}

/// Rows are stored per line of a page, so a payload much bigger than a pointer goes behind an `Rc`.
#[derive(Clone)]
pub enum PageRow {
    SummaryTitle(SharedString),
    SummaryBullet(SharedString),
    ReviewerSummary(Rc<PreparedMarkdown>),
    PrDescription(Rc<PreparedMarkdown>),
    StepHeading(Rc<StepHeading>),
    StepWhy(SharedString),
    /// Its height follows the drawing; the layout re-measures it when the drawing changes.
    Diagram(Rc<DiagramRow>),
    StepNotes(StepNotes),
    RepinNotes {
        gone_pieces: usize,
        text_may_be_old: bool,
    },
    Toolbar(Rc<Toolbar>),
    SafetyNetIntro,
    PieceHeader(Rc<PieceHeading>),
    /// Inside a file block of several pieces: where the next piece starts.
    PieceMarker(Rc<PieceHeading>),
    /// In the change since the read, a piece without lines was not touched since.
    NoChangeSinceRead,
    BinaryFile(Rc<BinaryFileRow>),
    /// The pictures of an image file; comments on the whole file hang under it.
    Image {
        path: FilePath,
        is_loaded: bool,
    },
    OnlyWhitespace,
    HunkGap(Rc<HunkGapRow>),
    Line(Rc<DiffRowLine>),
    /// Under its line inside the piece box, or after the code.
    Thread {
        view: Rc<ThreadView>,
        place: ThreadRowPlace,
        /// Of the reply box, if open; its text is read from the review when drawn.
        reply_lines: Option<usize>,
        /// The `<details>` of its comments that are open.
        open_details: Rc<[String]>,
    },
    QueuedComment(Rc<QueuedComment>),
    /// A comment box; its text is read from the review when drawn, so only its line count is a part of the row.
    Composer {
        place: Rc<CommentPlace>,
        text_lines: usize,
        problem: Option<CommentProblem>,
        is_sending: bool,
    },
    /// The ask button under the last of the lines that have Asks.
    AskMarker(Rc<AskAt>),
    PieceEnd,
    ScreenFooter(Rc<ScreenFooter>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RowKey(u64);

impl PageRow {
    fn key(&self) -> RowKey {
        let mut hasher = DefaultHasher::new();
        std::mem::discriminant(self).hash(&mut hasher);
        match self {
            Self::SummaryTitle(text) | Self::SummaryBullet(text) | Self::StepWhy(text) => {
                text.hash(&mut hasher);
            }
            Self::StepHeading(heading) => {
                heading.id.to_string().hash(&mut hasher);
                heading.title.hash(&mut hasher);
                heading.number.hash(&mut hasher);
            }
            Self::ReviewerSummary(summary) | Self::PrDescription(summary) => {
                summary.hash(&mut hasher);
            }
            Self::AskMarker(ask) => ask.id.hash(&mut hasher),
            Self::Toolbar(toolbar) => {
                toolbar.ask.as_ref().map(|ask| &ask.id).hash(&mut hasher);
                toolbar.fold.hash(&mut hasher);
                toolbar.full_diff.hash(&mut hasher);
                toolbar.ignores_whitespace.hash(&mut hasher);
                toolbar.wraps_lines.hash(&mut hasher);
            }
            Self::ScreenFooter(footer) => footer.hash(&mut hasher),
            Self::StepNotes(notes) => notes.hash(&mut hasher),
            Self::RepinNotes {
                gone_pieces,
                text_may_be_old,
            } => {
                gone_pieces.hash(&mut hasher);
                text_may_be_old.hash(&mut hasher);
            }
            Self::PieceHeader(heading) | Self::PieceMarker(heading) => {
                heading.fold.hash(&mut hasher);
                heading.piece.hash(&mut hasher);
                heading.repin_label.hash(&mut hasher);
            }
            Self::BinaryFile(binary) => {
                binary.piece.hash(&mut hasher);
                binary.link.hash(&mut hasher);
            }
            Self::Image { path, is_loaded } => {
                path.hash(&mut hasher);
                is_loaded.hash(&mut hasher);
            }
            Self::HunkGap(gap) => {
                gap.id.hash(&mut hasher);
                gap.more.is_some().hash(&mut hasher);
            }
            Self::Line(line) => {
                line.location.path.hash(&mut hasher);
                line.location.side.hash(&mut hasher);
                line.location.line.hash(&mut hasher);
                matches!(line.kind, LineKind::Context { .. }).hash(&mut hasher);
                line.wraps.hash(&mut hasher);
            }
            Self::Thread {
                view,
                place,
                reply_lines,
                open_details,
            } => {
                view.thread.id.hash(&mut hasher);
                view.is_folded.hash(&mut hasher);
                for comment in &view.thread.comments {
                    comment.id.hash(&mut hasher);
                    comment.body.hash(&mut hasher);
                    for count in &comment.reactions {
                        count.reaction.hash(&mut hasher);
                        count.count.hash(&mut hasher);
                        count.viewer_has_reacted.hash(&mut hasher);
                    }
                }
                view.commented_lines.hash(&mut hasher);
                view.hand_off_line.hash(&mut hasher);
                view.failed_hand_off.hash(&mut hasher);
                reply_lines.hash(&mut hasher);
                for (id, text) in &view.edit_drafts {
                    id.hash(&mut hasher);
                    line_count(text).hash(&mut hasher);
                }
                open_details.hash(&mut hasher);
                place.hash(&mut hasher);
            }
            Self::QueuedComment(queued) => {
                queued.draft.place.hash(&mut hasher);
                queued.failed_because.hash(&mut hasher);
            }
            Self::Composer {
                place,
                text_lines,
                problem,
                is_sending,
            } => {
                place.hash(&mut hasher);
                text_lines.hash(&mut hasher);
                problem.hash(&mut hasher);
                is_sending.hash(&mut hasher);
            }
            Self::Diagram(diagram) => {
                diagram.step.hash(&mut hasher);
                diagram.notes_button.hash(&mut hasher);
            }
            Self::SafetyNetIntro
            | Self::NoChangeSinceRead
            | Self::OnlyWhitespace
            | Self::PieceEnd => {}
        }
        RowKey(hasher.finish())
    }
}

/// Where a thread's row sits on the page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ThreadRowPlace {
    /// Hangs under the line it is about.
    UnderLine,
    /// Follows the step's code, for a thread with no line to hang under.
    AfterCode,
}

/// What a piece box shows of its piece.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DiffShown {
    Full,
    ChangeSinceRead,
}

/// The rows a drag selected, as rows of the list: those in `rows` that show a line of `path`.
pub struct SelectedRows {
    rows: RangeInclusive<usize>,
    path: FilePath,
}

impl SelectedRows {
    pub fn contains(&self, row: usize, line: &LineLocation) -> bool {
        self.rows.contains(&row) && line.path == self.path
    }
}

struct PieceAt {
    id: PieceId,
    path: FilePath,
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
    pub fn of(review: &Review, asked_lines: Vec<Place>, facts: &RowFacts) -> Self {
        let screen = review.screen().clone();
        let mut builder = RowsBuilder {
            rows: Vec::new(),
            under_lines: UnderLines::default(),
            facts,
            review,
        };
        let mut diagram = None;
        match &screen {
            Screen::Summary => summary_rows(review, &mut builder.rows),
            Screen::Step(_) => {
                if let Some(page) = review.current_step() {
                    let comments = under_lines(review, &asked_lines);
                    diagram = builder.step_rows(page, comments, review);
                }
            }
            Screen::SafetyNet => {
                if let Some(page) = review.safety_net() {
                    let comments = under_lines(review, &asked_lines);
                    builder.safety_net_rows(page, comments, review);
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
                PageRow::PieceHeader(heading) | PageRow::PieceMarker(heading) => {
                    pieces.push(PieceAt {
                        id: heading.piece.clone(),
                        path: heading.path.clone(),
                        row: index,
                    });
                }
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
            .position(|row| matches!(row, PageRow::Diagram(_)))
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

    /// The most characters a line of this file has on the screen; how far its box can scroll sideways.
    pub fn widest_line(&self, path: &FilePath) -> usize {
        self.rows
            .iter()
            .filter_map(|row| match row {
                PageRow::Line(line) if line.location.path == *path => {
                    Some(line.code().chars().count())
                }
                _ => None,
            })
            .max()
            .unwrap_or(0)
    }

    /// The first row that shows the line.
    pub fn row_of_line(&self, line: &LineLocation) -> Option<usize> {
        self.line_rows.get(line).copied()
    }

    /// The row a jump lands on: a file's first piece, a symbol's piece, else the step's top.
    pub fn row_of_jump(&self, target: &JumpTarget) -> usize {
        let piece = match &target.place {
            JumpPlace::StepTop => None,
            JumpPlace::File(path) => self.pieces.iter().find(|piece| piece.path == *path),
            JumpPlace::Piece(id) => self.pieces.iter().find(|piece| piece.id == *id),
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

    pub fn change_from(&self, old: &Self) -> RowsChange {
        if self.screen != old.screen {
            return RowsChange::OtherScreen;
        }
        changed_span(&old.keys, &self.keys)
    }
}

fn changed_span(old: &[RowKey], new: &[RowKey]) -> RowsChange {
    let same_start = old.iter().zip(new).take_while(|(a, b)| a == b).count();
    let same_end = old
        .iter()
        .skip(same_start)
        .rev()
        .zip(new.iter().skip(same_start).rev())
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
    rows.push(PageRow::SummaryTitle(review.title().to_owned().into()));
    rows.extend(
        review
            .summary()
            .iter()
            .map(|bullet| PageRow::SummaryBullet(bullet.clone().into())),
    );
    rows.extend(
        review
            .reviewer_summary()
            .map(|summary| PageRow::ReviewerSummary(Rc::new(PreparedMarkdown::of(summary)))),
    );
    rows.extend(
        review
            .pr_description()
            .map(|description| PageRow::PrDescription(Rc::new(PreparedMarkdown::of(description)))),
    );
    rows.push(PageRow::Toolbar(Rc::new(Toolbar {
        ask: Some(AskAt {
            id: "ask-tour".into(),
            place: Place::Tour,
            label: "the whole tour".into(),
        }),
        fold: None,
        full_diff: None,
        ignores_whitespace: None,
        wraps_lines: None,
    })));
    rows.push(screen_footer(review, None));
}

fn screen_footer(review: &Review, ticks: Option<StepId>) -> PageRow {
    let place = review.reading_place();
    PageRow::ScreenFooter(Rc::new(ScreenFooter {
        ticks,
        has_previous: place.has_previous,
        next: place.next,
        is_summary: review.screen() == &Screen::Summary,
    }))
}

/// What hangs under a row of code, in this order: threads, queued comments, comment boxes, ask markers.
enum UnderLine {
    Thread(ThreadView),
    Queued(QueuedComment),
    Composer(CommentPlace),
    AskMarker(AskAt),
}

impl UnderLine {
    fn at(&self) -> Option<Hangs> {
        match self {
            Self::Thread(view) => view.anchor.clone().map(Hangs::Line),
            Self::Queued(queued) => Some(Hangs::of_comment(&queued.draft.place)),
            Self::Composer(place) => Some(Hangs::of_comment(place)),
            Self::AskMarker(ask) => match &ask.place {
                Place::Lines { end, .. } => Some(Hangs::Line(end.clone())),
                _ => None,
            },
        }
    }

    fn into_row(self, review: &Review, facts: &RowFacts) -> PageRow {
        match self {
            Self::Thread(view) => thread_row(view, ThreadRowPlace::UnderLine, facts),
            Self::Queued(queued) => PageRow::QueuedComment(Rc::new(queued)),
            Self::Composer(place) => composer_row(place, review, facts),
            Self::AskMarker(ask) => PageRow::AskMarker(Rc::new(ask)),
        }
    }
}

/// Where something hangs: under a line of code, or under the picture of a file.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Hangs {
    Line(LineLocation),
    File(FilePath),
}

impl Hangs {
    fn of_comment(place: &CommentPlace) -> Self {
        match place {
            CommentPlace::File(path) => Self::File(path.clone()),
            CommentPlace::Lines { end, .. } => Self::Line(end.clone()),
        }
    }
}

/// What hangs under the rows of a screen, by the line it hangs under, each with its place in the order it shows.
#[derive(Default)]
struct UnderLines {
    by_line: HashMap<Hangs, Vec<(usize, UnderLine)>>,
}

impl UnderLines {
    /// What hangs under no line is dropped.
    fn of(items: impl IntoIterator<Item = UnderLine>) -> Self {
        let mut by_line: HashMap<Hangs, Vec<(usize, UnderLine)>> = HashMap::new();
        for (order, item) in items.into_iter().enumerate() {
            if let Some(at) = item.at() {
                by_line.entry(at).or_default().push((order, item));
            }
        }
        Self { by_line }
    }

    fn take_at(&mut self, at: &LineLocation) -> Vec<UnderLine> {
        self.take(&Hangs::Line(at.clone()))
    }

    fn take_whole_file(&mut self, path: &FilePath) -> Vec<UnderLine> {
        self.take(&Hangs::File(path.clone()))
    }

    fn take(&mut self, at: &Hangs) -> Vec<UnderLine> {
        self.by_line
            .remove(at)
            .into_iter()
            .flatten()
            .map(|(_, item)| item)
            .collect()
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
            label: place.lines_label()?,
        }))
    });
    queued.chain(composers).chain(markers).collect()
}

/// What the layout knows that changes the height of rows and the review does not.
pub struct RowFacts<'a> {
    pub comment_problems: &'a HashMap<CommentPlace, CommentProblem>,
    pub sending_comments: &'a HashSet<CommentPlace>,
    pub open_details: &'a OpenDetails,
}

struct RowsBuilder<'a> {
    rows: Vec<PageRow>,
    facts: &'a RowFacts<'a>,
    review: &'a Review,
    /// Taken out as their row is placed; what no row takes is dropped.
    under_lines: UnderLines,
}

impl RowsBuilder<'_> {
    /// Returns the diagram the step shows.
    fn step_rows(
        &mut self,
        page: StepPage<'_>,
        comments: Vec<UnderLine>,
        review: &Review,
    ) -> Option<SharedString> {
        let step = page.step;
        let id = step.id.clone();
        let place = review.reading_place();
        self.rows.push(PageRow::StepHeading(Rc::new(StepHeading {
            id: id.clone(),
            title: step.title.clone().into(),
            number: place.step_number.map(|number| (number, place.step_count)),
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
        let notes = StepNotes::of(step);
        if !notes.is_empty() {
            self.rows.push(PageRow::StepNotes(notes));
        }
        if let Some(notes_button) = diagram
            .as_ref()
            .and_then(|_| review.diagram_notes_button(&id))
        {
            self.rows.push(PageRow::Diagram(Rc::new(DiagramRow {
                step: id.clone(),
                notes_button,
            })));
        }
        if page.gone_pieces > 0 || page.text_may_be_old {
            self.rows.push(PageRow::RepinNotes {
                gone_pieces: page.gone_pieces,
                text_may_be_old: page.text_may_be_old,
            });
        }
        self.rows.push(PageRow::Toolbar(Rc::new(Toolbar {
            ask: Some(AskAt {
                id: format!("ask-step-{id}").into(),
                place: Place::Step { step: id.clone() },
                label: step.title.clone(),
            }),
            fold: (page.is_folded || step.skim || page.is_ticked).then(|| FoldControl {
                step: id.clone(),
                is_folded: page.is_folded,
                piece_count: page.pieces.len(),
            }),
            full_diff: page.since_read.as_ref().map(|since| FullDiffControl {
                step: id.clone(),
                is_showing_full_diff: since.showing_full_diff,
            }),
            ignores_whitespace: Some(review.ignores_whitespace()),
            wraps_lines: Some(review.wraps_lines()),
        })));
        let footer = screen_footer(review, (!page.is_ticked).then(|| id.clone()));
        let shown = if page
            .since_read
            .as_ref()
            .is_some_and(|since| !since.showing_full_diff)
        {
            DiffShown::ChangeSinceRead
        } else {
            DiffShown::Full
        };
        let (under_a_line, after_code): (Vec<_>, Vec<_>) = page
            .threads
            .into_iter()
            .partition(|thread| thread.anchor.is_some());
        self.hang_threads_and(under_a_line, comments);
        let ask_of = |piece: &PieceDiff| AskAt {
            id: format!("ask-piece-{id}-{}", piece.id).into(),
            place: Place::Piece {
                step: id.clone(),
                piece: piece.id.clone(),
            },
            label: piece.id.to_string(),
        };
        for block in FileBlock::group(page.pieces) {
            let fold = BoxFold {
                step: id.clone(),
                is_folded: review.is_box_folded(&id, &block.path),
                path: block.path.clone(),
            };
            self.block_rows(&block, Some(&ask_of), Some(fold), shown, review);
        }
        self.rows.extend(
            after_code
                .into_iter()
                .map(|view| thread_row(view, ThreadRowPlace::AfterCode, self.facts)),
        );
        self.rows.push(footer);
        diagram
    }

    fn safety_net_rows(&mut self, page: SafetyNetPage, comments: Vec<UnderLine>, review: &Review) {
        self.rows.push(PageRow::SafetyNetIntro);
        self.rows.push(PageRow::Toolbar(Rc::new(Toolbar {
            ask: None,
            fold: None,
            full_diff: None,
            ignores_whitespace: Some(review.ignores_whitespace()),
            wraps_lines: Some(review.wraps_lines()),
        })));
        let (under_a_line, after_code): (Vec<_>, Vec<_>) = page
            .threads
            .into_iter()
            .partition(|thread| thread.anchor.is_some());
        self.hang_threads_and(under_a_line, comments);
        for block in FileBlock::group(page.pieces) {
            self.block_rows(&block, None, None, DiffShown::Full, review);
        }
        self.rows.extend(
            after_code
                .into_iter()
                .map(|view| thread_row(view, ThreadRowPlace::AfterCode, self.facts)),
        );
        self.rows.push(screen_footer(review, None));
    }

    fn hang_threads_and(&mut self, threads: Vec<ThreadView>, comments: Vec<UnderLine>) {
        self.under_lines =
            UnderLines::of(threads.into_iter().map(UnderLine::Thread).chain(comments));
    }

    fn block_rows(
        &mut self,
        block: &FileBlock,
        ask_of: Option<&dyn Fn(&PieceDiff) -> AskAt>,
        fold: Option<BoxFold>,
        shown: DiffShown,
        review: &Review,
    ) {
        if fold.as_ref().is_some_and(|fold| fold.is_folded) {
            self.folded_block_rows(block, ask_of, fold);
            return;
        }
        let heading = |piece: &PieceDiff| PieceHeading {
            fold: None,
            piece: piece.id.clone(),
            label: PieceLabel::of(&piece.id),
            path: piece.id.path().clone(),
            ask: ask_of.map(|ask_of| ask_of(piece)),
            repin_label: piece.repin_status.and_then(repin_label),
            code_link: piece.code_link().cloned(),
        };
        let has_no_change_since_read = |piece: &PieceDiff| {
            shown == DiffShown::ChangeSinceRead
                && piece.hunks.is_empty()
                && piece.binary_file.is_none()
        };
        let row_id = if let [piece] = block.pieces.as_slice() {
            self.rows.push(PageRow::PieceHeader(Rc::new(PieceHeading {
                fold,
                ..heading(piece)
            })));
            if has_no_change_since_read(piece) {
                self.rows.push(PageRow::NoChangeSinceRead);
            }
            if let Some(link) = piece.binary_file.clone() {
                self.rows.push(PageRow::BinaryFile(Rc::new(BinaryFileRow {
                    piece: piece.id.to_string().into(),
                    link,
                })));
            }
            if piece.is_image {
                let path = piece.id.path().clone();
                self.rows.push(PageRow::Image {
                    is_loaded: review.image_preview(&path).is_some(),
                    path: path.clone(),
                });
                let here = self.under_lines.take_whole_file(&path);
                self.hang(here);
            }
            if piece.changes_only_whitespace {
                self.rows.push(PageRow::OnlyWhitespace);
            }
            piece.id.to_string()
        } else {
            let whole_file = PieceId::whole_file(block.path.clone());
            self.rows.push(PageRow::PieceHeader(Rc::new(PieceHeading {
                fold,
                piece: whole_file.clone(),
                label: PieceLabel::of(&whole_file),
                path: block.path.clone(),
                ask: None,
                repin_label: None,
                code_link: block.lines().next().map(|line| line.code_link.clone()),
            })));
            for piece in block.without_rows() {
                self.rows
                    .push(PageRow::PieceMarker(Rc::new(heading(piece))));
                if has_no_change_since_read(piece) {
                    self.rows.push(PageRow::NoChangeSinceRead);
                }
                if piece.changes_only_whitespace {
                    self.rows.push(PageRow::OnlyWhitespace);
                }
            }
            block.path.to_string()
        };
        let shows_markers = !block.has_one_piece();
        for (index, hunk) in block.hunks.iter().enumerate() {
            if index > 0 || hunk.more_above.is_some() {
                self.rows.push(PageRow::HunkGap(Rc::new(HunkGapRow {
                    id: format!("{row_id}-above-{index}").into(),
                    more: hunk.more_above.clone(),
                })));
            }
            for line in &hunk.rows {
                if shows_markers
                    && let Some((piece, is_resumed)) = block.piece_starting_at(&line.location)
                {
                    let marker = if is_resumed {
                        PieceHeading {
                            ask: None,
                            code_link: None,
                            ..heading(piece)
                        }
                    } else {
                        heading(piece)
                    };
                    self.rows.push(PageRow::PieceMarker(Rc::new(marker)));
                }
                let here = self.under_lines.take_at(&line.location);
                self.rows.push(PageRow::Line(Rc::new(DiffRowLine::of(
                    line.clone(),
                    review.wraps_lines(),
                ))));
                self.hang(here);
            }
            if hunk.more_below.is_some() {
                self.rows.push(PageRow::HunkGap(Rc::new(HunkGapRow {
                    id: format!("{row_id}-below-{index}").into(),
                    more: hunk.more_below.clone(),
                })));
            }
        }
        self.rows.push(PageRow::PieceEnd);
    }

    /// A folded box keeps its header and the labels of its pieces, so the step still says what it covers.
    fn folded_block_rows(
        &mut self,
        block: &FileBlock,
        ask_of: Option<&dyn Fn(&PieceDiff) -> AskAt>,
        fold: Option<BoxFold>,
    ) {
        let heading = |piece: &PieceDiff| PieceHeading {
            fold: None,
            piece: piece.id.clone(),
            label: PieceLabel::of(&piece.id),
            path: piece.id.path().clone(),
            ask: ask_of.map(|ask_of| ask_of(piece)),
            repin_label: piece.repin_status.and_then(repin_label),
            code_link: piece.code_link().cloned(),
        };
        if let [piece] = block.pieces.as_slice() {
            self.rows.push(PageRow::PieceHeader(Rc::new(PieceHeading {
                fold,
                ..heading(piece)
            })));
        } else {
            let whole_file = PieceId::whole_file(block.path.clone());
            self.rows.push(PageRow::PieceHeader(Rc::new(PieceHeading {
                fold,
                piece: whole_file.clone(),
                label: PieceLabel::of(&whole_file),
                path: block.path.clone(),
                ask: None,
                repin_label: None,
                code_link: block.lines().next().map(|line| line.code_link.clone()),
            })));
            for piece in block.pieces_in_order() {
                self.rows
                    .push(PageRow::PieceMarker(Rc::new(heading(piece))));
            }
        }
        self.rows.push(PageRow::PieceEnd);
    }

    fn hang(&mut self, here: Vec<UnderLine>) {
        let (review, facts) = (self.review, self.facts);
        self.rows
            .extend(here.into_iter().map(|item| item.into_row(review, facts)));
    }
}

fn repin_label(status: RepinStatus) -> Option<&'static str> {
    match status {
        RepinStatus::Changed => Some("changed"),
        RepinStatus::New => Some("new"),
        RepinStatus::Same | RepinStatus::Gone => None,
    }
}

fn thread_row(view: ThreadView, place: ThreadRowPlace, facts: &RowFacts) -> PageRow {
    PageRow::Thread {
        reply_lines: view.reply_draft.as_deref().map(line_count),
        open_details: facts
            .open_details
            .open_under(&format!("thread-{}-", view.thread.id))
            .into(),
        view: Rc::new(view),
        place,
    }
}

fn composer_row(place: CommentPlace, review: &Review, facts: &RowFacts) -> PageRow {
    let text = review
        .comment_drafts()
        .iter()
        .find(|draft| draft.place == place)
        .map_or("", |draft| draft.body.as_str());
    PageRow::Composer {
        text_lines: line_count(text),
        problem: facts.comment_problems.get(&place).cloned(),
        is_sending: facts.sending_comments.contains(&place),
        place: Rc::new(place),
    }
}

/// Typing only changes a box's height when its line count changes.
pub fn line_count(text: &str) -> usize {
    text.split('\n').count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::line_number::LineNumber;
    use crate::viewer::{
        CommentId, Reaction, ReactionCount, ReviewThread, Side, ThreadComment, ThreadId,
    };
    use std::sync::Arc;

    #[test]
    fn a_page_row_stays_small_since_there_is_one_per_line() {
        assert_eq!(size_of::<PageRow>(), 56);
    }

    fn line(number: usize) -> LineNumber {
        LineNumber::try_from(number).unwrap()
    }

    fn box_at(path: &str, side: Side, number: usize) -> UnderLine {
        let at = LineLocation {
            path: FilePath::new(path).unwrap(),
            side,
            line: line(number),
        };
        UnderLine::Composer(CommentPlace::Lines {
            start: at.clone(),
            end: at,
        })
    }

    fn box_on_file(path: &str) -> UnderLine {
        UnderLine::Composer(CommentPlace::File(FilePath::new(path).unwrap()))
    }

    fn lines_of(items: &[UnderLine]) -> Vec<(Side, u32)> {
        items
            .iter()
            .filter_map(|item| match item.at()? {
                Hangs::Line(at) => Some((at.side, at.line.get())),
                Hangs::File(_) => None,
            })
            .collect()
    }

    #[test]
    fn what_hangs_under_a_line_is_taken_once_in_its_order() {
        let mut under = UnderLines::of([
            box_at("a.php", Side::Head, 3),
            box_at("a.php", Side::Base, 3),
            box_at("a.php", Side::Head, 3),
        ]);
        let head_3 = LineLocation {
            path: FilePath::new("a.php").unwrap(),
            side: Side::Head,
            line: line(3),
        };

        assert_eq!(lines_of(&under.take_at(&head_3)), [(Side::Head, 3); 2]);
        assert!(under.take_at(&head_3).is_empty());
    }

    #[test]
    fn what_hangs_under_a_whole_file_is_taken_once_and_not_with_its_lines() {
        let mut under = UnderLines::of([
            box_on_file("logo.png"),
            box_on_file("other.png"),
            box_at("logo.png", Side::Head, 1),
            box_on_file("logo.png"),
        ]);

        assert_eq!(
            under
                .take_whole_file(&FilePath::new("logo.png").unwrap())
                .len(),
            2
        );
        assert!(
            under
                .take_whole_file(&FilePath::new("logo.png").unwrap())
                .is_empty()
        );
        assert_eq!(
            under
                .take_whole_file(&FilePath::new("other.png").unwrap())
                .len(),
            1
        );
    }

    fn thread_view() -> ThreadView {
        ThreadView {
            thread: ReviewThread {
                id: ThreadId::new("T1"),
                is_resolved: false,
                is_outdated: false,
                path: FilePath::new("a.rs").unwrap(),
                line: Some(line(1)),
                start_line: None,
                side: Side::Head,
                is_file_comment: false,
                original_line: Some(line(1)),
                original_start_line: None,
                original_commit: None,
                comments: vec![ThreadComment {
                    id: CommentId::new("C1"),
                    author: "alice".into(),
                    is_bot: false,
                    by_viewer: false,
                    body: "why?".into(),
                    url: String::new(),
                    reactions: Vec::new(),
                }],
            },
            is_folded: false,
            anchor: None,
            commented_lines: Arc::new([]),
            reply_draft: None,
            edit_drafts: std::collections::BTreeMap::new(),
            hand_off_line: None,
            failed_hand_off: None,
        }
    }

    fn thread_key(view: ThreadView, reply_lines: Option<usize>, open: Vec<String>) -> RowKey {
        PageRow::Thread {
            view: Rc::new(view),
            place: ThreadRowPlace::UnderLine,
            reply_lines,
            open_details: open.into(),
        }
        .key()
    }

    #[test]
    fn a_thread_row_is_measured_again_when_a_reaction_a_body_or_a_details_changes_its_height() {
        let plain = thread_key(thread_view(), None, Vec::new());

        let mut reacted = thread_view();
        reacted.thread.comments[0].reactions = vec![ReactionCount {
            reaction: Reaction::Heart,
            count: 1,
            viewer_has_reacted: true,
        }];
        let mut edited = thread_view();
        edited.thread.comments[0].body = "why?\n\nreally".into();

        assert_ne!(plain, thread_key(reacted, None, Vec::new()));
        assert_ne!(plain, thread_key(edited, None, Vec::new()));
        assert_ne!(
            plain,
            thread_key(thread_view(), None, vec!["thread-T1-0-2".into()])
        );
        assert_ne!(plain, thread_key(thread_view(), Some(1), Vec::new()));
        assert_ne!(
            thread_key(thread_view(), Some(1), Vec::new()),
            thread_key(thread_view(), Some(2), Vec::new())
        );
    }

    fn thread_key_editing(text: Option<&str>) -> RowKey {
        let mut view = thread_view();
        view.edit_drafts = text
            .map(|text| (CommentId::new("C1"), text.to_owned()))
            .into_iter()
            .collect();
        thread_key(view, None, Vec::new())
    }

    #[test]
    fn a_thread_row_is_measured_again_when_a_comment_edit_opens_or_changes_its_line_count() {
        assert_ne!(thread_key_editing(None), thread_key_editing(Some("a")));
        assert_ne!(
            thread_key_editing(Some("a")),
            thread_key_editing(Some("a\nb"))
        );
        assert_eq!(
            thread_key_editing(Some("a")),
            thread_key_editing(Some("a longer one"))
        );
    }

    #[test]
    fn typing_inside_a_line_leaves_the_thread_row_as_it_was() {
        let mut typed = thread_view();
        typed.reply_draft = Some("because".into());
        let mut typed_more = thread_view();
        typed_more.reply_draft = Some("because it".into());

        assert_eq!(
            thread_key(typed, Some(line_count("because")), Vec::new()),
            thread_key(typed_more, Some(line_count("because it")), Vec::new())
        );
    }

    fn composer_key(text: &str, problem: Option<CommentProblem>) -> RowKey {
        PageRow::Composer {
            place: Rc::new(CommentPlace::on_file(&FilePath::new("a.rs").unwrap())),
            text_lines: line_count(text),
            problem,
            is_sending: false,
        }
        .key()
    }

    #[test]
    fn a_comment_box_row_changes_with_its_line_count_and_its_problem_only() {
        assert_eq!(composer_key("a", None), composer_key("a longer one", None));
        assert_ne!(composer_key("a", None), composer_key("a\nb", None));
        assert_ne!(
            composer_key("a", None),
            composer_key("a", Some(CommentProblem::Rejected("rejected".into())))
        );
    }

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
