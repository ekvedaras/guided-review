//! Lays a screen out as rows: what the summary shows, a step's text, toolbar and file boxes, and
//! what hangs under the lines of those boxes.

use std::collections::HashMap;
use std::sync::Arc;

use super::rows::{
    AskAt, BinaryFileRow, BoxFold, DiagramRow, FoldControl, FullDiffControl, HunkGapRow, PageRow,
    PieceHeading, PieceLabel, ReviewerSummaryRow, RouteStopRow, ScreenFooter, ShownLine,
    StepHeading, StepIntro, StepNotes, SummaryColumns, SummaryStatsRow, ThreadRowPlace, Toolbar,
    line_count,
};
use crate::file_path::FilePath;
use crate::pieces::PieceId;
use crate::repin::RepinStatus;
use crate::viewer::review::ReviewCore;
use crate::viewer::{
    BoxState, CommentPlace, FileBlock, LineLocation, Markdown, NoteSubject, NotesButton, PieceDiff,
    Place, QueuedComment, SafetyNetPage, StepId, StepPage, ThreadView, TourLead,
    hidden_lines_after, hidden_lines_before,
};

/// The rows of a screen, the file blocks they show, and the diagram of a step.
pub(super) struct Laid {
    pub rows: Vec<PageRow>,
    pub blocks: Vec<FileBlock>,
    pub diagram: Option<Arc<str>>,
}

pub(super) fn summary(review: &ReviewCore) -> Laid {
    let mut builder = Builder::new(review);
    builder.summary_rows();
    builder.laid(None)
}

pub(super) fn step(review: &ReviewCore, page: StepPage<'_>) -> Laid {
    let mut builder = Builder::new(review);
    let diagram = builder.step_rows(page);
    builder.laid(diagram)
}

pub(super) fn safety_net(review: &ReviewCore, page: SafetyNetPage) -> Laid {
    let mut builder = Builder::new(review);
    builder.safety_net_rows(page);
    builder.laid(None)
}

/// What a piece box shows of its piece.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DiffShown {
    Full,
    ChangeSinceRead,
}

struct Builder<'a> {
    review: &'a ReviewCore,
    rows: Vec<PageRow>,
    blocks: Vec<FileBlock>,
    /// Taken out as their row is placed; what no row takes is dropped.
    under_lines: UnderLines,
}

impl<'a> Builder<'a> {
    fn new(review: &'a ReviewCore) -> Self {
        Self {
            review,
            rows: Vec::new(),
            blocks: Vec::new(),
            under_lines: UnderLines::default(),
        }
    }

    fn laid(self, diagram: Option<Arc<str>>) -> Laid {
        Laid {
            rows: self.rows,
            blocks: self.blocks,
            diagram,
        }
    }

    /// The summary is the tour's start page: what the PR is, how big, what it does, the route of the
    /// tour and the way in, then the reviewer summary and the PR's own description.
    fn summary_rows(&mut self) {
        let review = self.review;
        let mut rows = Vec::new();
        rows.push(PageRow::SummaryTitle(review.title().into()));
        rows.push(PageRow::SummaryStats(Arc::new(SummaryStatsRow::of(
            review.summary_stats(),
        ))));
        rows.extend(
            review
                .summary()
                .iter()
                .map(|bullet| PageRow::SummaryBullet(bullet.as_str().into())),
        );
        let route = review.route();
        if !route.is_empty() {
            rows.push(PageRow::RouteHeading);
        }
        let last = route.len().saturating_sub(1);
        rows.extend(route.into_iter().enumerate().map(|(index, stop)| {
            PageRow::RouteStop(Arc::new(RouteStopRow {
                id: stop.id,
                number: (index + 1).to_string().into(),
                title: stop.title.into(),
                why: stop.why.into(),
                details: match stop.details {
                    0 => None,
                    1 => Some("+1 detail".into()),
                    count => Some(format!("+{count} details").into()),
                },
                is_read: stop.is_read,
                has_check: stop.has_check,
                is_first: index == 0,
                is_last: index == last,
            }))
        }));
        let lead = review.tour_lead();
        if lead != TourLead::Nowhere {
            rows.push(PageRow::TourLead(Arc::new(lead)));
        }
        self.rows
            .push(PageRow::SummaryColumns(Arc::new(SummaryColumns {
                summary: rows,
                reviewer_summary: review
                    .reviewer_summary()
                    .map(|summary| Arc::new(reviewer_summary_row(review, summary))),
                description: review.pr_description().map(Arc::new),
            })));
        self.rows.push(PageRow::Toolbar(Arc::new(Toolbar {
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
    }

    /// Returns the diagram the step shows.
    fn step_rows(&mut self, page: StepPage<'_>) -> Option<Arc<str>> {
        let review = self.review;
        let step = page.step;
        let id = step.id.clone();
        let place = review.reading_place();
        self.rows.push(PageRow::StepHeading(Arc::new(StepHeading {
            id: id.clone(),
            title: step.title.as_str().into(),
            number: place.step_number.map(|number| (number, place.step_count)),
            is_ticked: page.is_ticked,
            is_gone: page.is_gone,
            folds_on_title_click: page.is_ticked || step.skim,
        })));
        let diagram = step
            .diagram
            .as_ref()
            .filter(|_| !page.is_folded)
            .map(|source| Arc::<str>::from(source.as_str()));
        let diagram_row = diagram
            .as_ref()
            .and_then(|_| review.notes_button(&NoteSubject::StepDiagram(id.clone())))
            .map(|notes_button| DiagramRow {
                step: id.clone(),
                notes_button,
            });
        self.rows.push(PageRow::StepIntro(Arc::new(StepIntro {
            why: step.why.as_str().into(),
            notes: StepNotes::of(step),
            diagram: diagram_row,
        })));
        if page.gone_pieces > 0 || page.text_may_be_old {
            self.rows.push(PageRow::RepinNotes {
                gone_pieces: page.gone_pieces,
                text_may_be_old: page.text_may_be_old,
            });
        }
        let has_code = !page.pieces.is_empty();
        let folds = page.is_folded || step.skim || page.is_ticked;
        self.rows.push(PageRow::Toolbar(Arc::new(Toolbar {
            ask: Some(AskAt {
                id: format!("ask-step-{id}").into(),
                place: Place::Step { step: id.clone() },
                label: step.title.clone(),
            }),
            fold: (folds && has_code).then(|| FoldControl {
                step: id.clone(),
                is_folded: page.is_folded,
                piece_count: page.pieces.len(),
            }),
            full_diff: page.since_read.as_ref().map(|since| FullDiffControl {
                step: id.clone(),
                is_showing_full_diff: since.showing_full_diff,
            }),
            ignores_whitespace: has_code.then(|| review.display_options().ignores_whitespace()),
            wraps_lines: has_code.then(|| review.display_options().wraps_lines()),
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
        let after_code = self.hang_threads_and_comments(page.threads);
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
            self.block_rows(block, Some(&ask_of), Some(fold), shown);
        }
        self.rows.extend(
            after_code
                .into_iter()
                .map(|view| thread_row(view, ThreadRowPlace::AfterCode)),
        );
        self.rows.push(footer);
        diagram
    }

    fn safety_net_rows(&mut self, page: SafetyNetPage) {
        let review = self.review;
        self.rows.push(PageRow::SafetyNetIntro);
        self.rows.push(PageRow::Toolbar(Arc::new(Toolbar {
            ask: None,
            fold: None,
            full_diff: None,
            ignores_whitespace: Some(review.display_options().ignores_whitespace()),
            wraps_lines: Some(review.display_options().wraps_lines()),
        })));
        let after_code = self.hang_threads_and_comments(page.threads);
        for block in FileBlock::group(page.pieces) {
            self.block_rows(block, None, None, DiffShown::Full);
        }
        self.rows.extend(
            after_code
                .into_iter()
                .map(|view| thread_row(view, ThreadRowPlace::AfterCode)),
        );
        self.rows.push(screen_footer(review, None));
    }

    /// Readies what hangs under the lines of the screen; returns the threads with no line to hang under.
    fn hang_threads_and_comments(&mut self, threads: Vec<ThreadView>) -> Vec<ThreadView> {
        let (under_a_line, after_code): (Vec<_>, Vec<_>) = threads
            .into_iter()
            .partition(|thread| thread.anchor.is_some());
        self.under_lines = UnderLines::of(
            under_a_line
                .into_iter()
                .map(UnderLine::Thread)
                .chain(comments_and_asks(self.review)),
        );
        after_code
    }

    fn block_rows(
        &mut self,
        block: FileBlock,
        ask_of: Option<&dyn Fn(&PieceDiff) -> AskAt>,
        fold: Option<BoxFold>,
        shown: DiffShown,
    ) {
        if fold.as_ref().is_some_and(|fold| fold.is_folded) {
            self.folded_block_rows(&block, ask_of, fold);
            self.blocks.push(block);
            return;
        }
        let heading = |piece: &PieceDiff| heading_of(piece, ask_of);
        let has_no_change_since_read = |piece: &PieceDiff| {
            shown == DiffShown::ChangeSinceRead
                && piece.hunks.is_empty()
                && piece.binary_file.is_none()
        };
        let wraps = self.review.display_options().wraps_lines();
        let row_id = if let [piece] = block.pieces.as_slice() {
            self.rows.push(PageRow::PieceHeader(Arc::new(PieceHeading {
                fold,
                ..heading(piece)
            })));
            if has_no_change_since_read(piece) {
                self.rows.push(PageRow::NoChangeSinceRead);
            }
            if let Some(link) = piece.binary_file.clone() {
                self.rows.push(PageRow::BinaryFile(Arc::new(BinaryFileRow {
                    piece: piece.id.to_string().into(),
                    link,
                })));
            }
            if piece.is_image {
                let path = piece.id.path().clone();
                self.rows.push(PageRow::Image {
                    is_loaded: self.review.display_options().image_preview(&path).is_some(),
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
            self.rows
                .push(PageRow::PieceHeader(Arc::new(whole_file_heading(
                    &block, fold,
                ))));
            for piece in block.without_rows() {
                self.rows
                    .push(PageRow::PieceMarker(Arc::new(heading(piece))));
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
        let hunks = block.hunks();
        for (index, hunk) in hunks.iter().enumerate() {
            if let Some(gap) = hidden_lines_before(hunks, index) {
                self.rows.push(PageRow::HunkGap(Arc::new(HunkGapRow::new(
                    format!("{row_id}-above-{index}"),
                    gap,
                ))));
            }
            for line in &hunk.rows {
                if shows_markers
                    && let Some((piece, is_resumed)) = block.piece_starting_at(&line.location)
                {
                    // A piece that starts again after one inside it keeps its buttons on its first label only.
                    let marker = if is_resumed {
                        PieceHeading {
                            ask: None,
                            code_link: None,
                            ..heading(piece)
                        }
                    } else {
                        heading(piece)
                    };
                    self.rows.push(PageRow::PieceMarker(Arc::new(marker)));
                }
                let here = self.under_lines.take_at(&line.location);
                self.rows
                    .push(PageRow::Line(Arc::new(ShownLine::of(line, wraps))));
                self.hang(here);
            }
        }
        if let Some(gap) = hidden_lines_after(hunks) {
            self.rows.push(PageRow::HunkGap(Arc::new(HunkGapRow::new(
                format!("{row_id}-below-end"),
                gap,
            ))));
        }
        self.rows.push(PageRow::PieceEnd);
        self.blocks.push(block);
    }

    /// A folded box keeps its header and the labels of its pieces, so the step still says what it covers.
    fn folded_block_rows(
        &mut self,
        block: &FileBlock,
        ask_of: Option<&dyn Fn(&PieceDiff) -> AskAt>,
        fold: Option<BoxFold>,
    ) {
        if let [piece] = block.pieces.as_slice() {
            self.rows.push(PageRow::PieceHeader(Arc::new(PieceHeading {
                fold,
                ..heading_of(piece, ask_of)
            })));
        } else {
            self.rows
                .push(PageRow::PieceHeader(Arc::new(whole_file_heading(
                    block, fold,
                ))));
            for piece in block.pieces_in_order() {
                self.rows
                    .push(PageRow::PieceMarker(Arc::new(heading_of(piece, ask_of))));
            }
        }
        self.rows.push(PageRow::PieceEnd);
    }

    /// Hangs `here` under the row above.
    fn hang(&mut self, here: Vec<UnderLine>) {
        let review = self.review;
        self.rows
            .extend(here.into_iter().map(|item| item.into_row(review)));
    }
}

fn heading_of(piece: &PieceDiff, ask_of: Option<&dyn Fn(&PieceDiff) -> AskAt>) -> PieceHeading {
    PieceHeading {
        fold: None,
        piece: piece.id.clone(),
        label: PieceLabel::of(&piece.id),
        path: piece.id.path().clone(),
        ask: ask_of.map(|ask_of| ask_of(piece)),
        repin_label: piece.repin_status.and_then(repin_label),
        code_link: piece.code_link().cloned(),
    }
}

/// The header of a box of several pieces names the file.
fn whole_file_heading(block: &FileBlock, fold: Option<BoxFold>) -> PieceHeading {
    let whole_file = PieceId::whole_file(block.path.clone());
    PieceHeading {
        fold,
        label: PieceLabel::of(&whole_file),
        piece: whole_file,
        path: block.path.clone(),
        ask: None,
        repin_label: None,
        code_link: block.lines().next().map(|line| line.code_link.clone()),
    }
}

fn reviewer_summary_row(review: &ReviewCore, summary: Markdown) -> ReviewerSummaryRow {
    let picture_buttons = summary
        .pictures()
        .into_iter()
        .filter_map(|(path, _)| {
            let button = review.notes_button(&NoteSubject::SummaryPicture(path.to_owned()))?;
            Some((path.to_owned(), button))
        })
        .collect();
    ReviewerSummaryRow {
        notes_button: review
            .notes_button(&NoteSubject::ReviewerSummary)
            .unwrap_or(NotesButton::Add),
        picture_buttons: Arc::new(picture_buttons),
        markdown: summary,
    }
}

fn screen_footer(review: &ReviewCore, ticks: Option<StepId>) -> PageRow {
    let place = review.reading_place();
    let lead = review.tour_lead();
    let leads_on =
        matches!(lead, TourLead::Finish { .. }) && place.next.is_none() && ticks.is_none();
    PageRow::ScreenFooter(Arc::new(ScreenFooter {
        ticks,
        has_previous: place.has_previous,
        next: place.next,
        lead: leads_on.then_some(lead),
    }))
}

fn repin_label(status: RepinStatus) -> Option<&'static str> {
    match status {
        RepinStatus::Changed => Some("changed"),
        RepinStatus::New => Some("new"),
        RepinStatus::Same | RepinStatus::Gone => None,
    }
}

fn thread_row(view: ThreadView, place: ThreadRowPlace) -> PageRow {
    PageRow::Thread {
        reply_lines: view.reply_draft.as_deref().map(line_count),
        view: Arc::new(view),
        place,
    }
}

/// What hangs under a row of code, in this order: threads, queued comments, comment boxes, ask markers.
enum UnderLine {
    Thread(ThreadView),
    Queued(QueuedComment),
    CommentBox(CommentPlace),
    AskMarker(AskAt),
}

impl UnderLine {
    fn at(&self) -> Option<Hangs> {
        match self {
            Self::Thread(view) => view.anchor.clone().map(Hangs::Line),
            Self::Queued(queued) => Some(Hangs::of_comment(&queued.draft.place)),
            Self::CommentBox(place) => Some(Hangs::of_comment(place)),
            Self::AskMarker(ask) => match &ask.place {
                Place::Lines { end, .. } => Some(Hangs::Line(end.clone())),
                _ => None,
            },
        }
    }

    fn into_row(self, review: &ReviewCore) -> PageRow {
        match self {
            Self::Thread(view) => thread_row(view, ThreadRowPlace::UnderLine),
            Self::Queued(queued) => PageRow::QueuedComment(Arc::new(queued)),
            Self::CommentBox(place) => comment_box_row(place, review),
            Self::AskMarker(ask) => PageRow::AskMarker(Arc::new(ask)),
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

/// What hangs under the rows of a screen, by the line it hangs under, in the order it shows.
#[derive(Default)]
struct UnderLines {
    by_line: HashMap<Hangs, Vec<UnderLine>>,
}

impl UnderLines {
    /// What hangs under no line is dropped.
    fn of(items: impl IntoIterator<Item = UnderLine>) -> Self {
        let mut by_line: HashMap<Hangs, Vec<UnderLine>> = HashMap::new();
        for item in items {
            if let Some(at) = item.at() {
                by_line.entry(at).or_default().push(item);
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
        self.by_line.remove(at).unwrap_or_default()
    }
}

/// Comments and asks of the screen; threads come with the page.
fn comments_and_asks(review: &ReviewCore) -> Vec<UnderLine> {
    let queued = review
        .queued_comments()
        .iter()
        .cloned()
        .map(UnderLine::Queued);
    let boxes = review
        .comment_boxes()
        .map(|comment_box| UnderLine::CommentBox(comment_box.draft.place.clone()));
    let markers = review.asked_lines().iter().filter_map(|place| {
        let Place::Lines { start, end, .. } = place else {
            return None;
        };
        Some(UnderLine::AskMarker(AskAt {
            id: format!("ask-lines-{}-{}-{}", end.path, start.line, end.line).into(),
            place: place.clone(),
            label: place.lines_label()?,
        }))
    });
    queued.chain(boxes).chain(markers).collect()
}

fn comment_box_row(place: CommentPlace, review: &ReviewCore) -> PageRow {
    let comment_box = review.comment_box(&place);
    PageRow::Composer {
        text_lines: line_count(comment_box.as_ref().map_or("", |shown| &shown.draft.body)),
        problem: comment_box.as_ref().and_then(|shown| shown.problem.clone()),
        is_sending: comment_box.is_some_and(|shown| shown.state == BoxState::Sending),
        place: Arc::new(place),
    }
}
