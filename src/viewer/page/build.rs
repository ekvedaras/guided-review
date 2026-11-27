//! Lays a screen out as rows: what the summary shows, a step's text, toolbar and file boxes, and
//! what hangs under the lines of those boxes.

use std::collections::HashMap;
use std::sync::Arc;

use super::rows::{
    BinaryFileRow, BoxFold, DiagramRow, FindingRow, FixesOffer, FoldControl, FullDiffControl,
    HunkGapRow, PageRow, PieceHeading, PieceLabel, ReviewerSummaryRow, RouteStopRow, ScreenFooter,
    ShownLine, StepHeading, StepIntro, StepNotes, SummaryColumns, SummaryStatsRow, ThreadRowPlace,
    Toolbar, line_count,
};
use crate::file_path::FilePath;
use crate::pieces::PieceId;
use crate::repin::RepinStatus;
use crate::viewer::finding_placement::FindingView;
use crate::viewer::fix_requests::FixState;
use crate::viewer::focused_change::ChangeFile;
use crate::viewer::rendered_view::section_title_at;
use crate::viewer::review::ReviewCore;
use crate::viewer::{
    BoxState, CommentPlace, FileBlock, FilePage, LineKind, LineLocation, Markdown, NoteSubject,
    PieceDiff, QueuedComment, ReadSubject, SafetyNetPage, Screen, Side, StepPage, ThreadView,
    TourLead, hidden_lines_after, hidden_lines_before,
};
use crate::viewer::{FileView, LabelSwitch, PageChange, PieceLabels, Rendering, ViewSwitch};

/// The rows of a screen, the file blocks they show, and the diagram of a step.
pub(super) struct Laid {
    pub rows: Vec<PageRow>,
    pub blocks: Vec<FileBlock>,
    pub diagram: Option<Arc<str>>,
    /// The files whose box shows them rendered, so none of their lines is on the page.
    pub rendered: Vec<FilePath>,
    pub changes: Vec<PageChange>,
}

pub(super) fn summary(review: &ReviewCore) -> Laid {
    let mut builder = Builder::new(review);
    builder.summary_rows();
    builder.laid(None)
}

pub(super) fn overview(review: &ReviewCore) -> Laid {
    let mut builder = Builder::new(review);
    builder.overview_rows();
    builder.laid(None)
}

pub(super) fn file(review: &ReviewCore, path: &FilePath, page: FilePage) -> Laid {
    let mut builder = Builder::new(review);
    builder.file_rows(path, page);
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
    rendered: Vec<FilePath>,
    changes: Vec<PageChange>,
    /// Taken out as their row is placed; what no row takes is dropped.
    under_lines: UnderLines,
}

impl<'a> Builder<'a> {
    fn new(review: &'a ReviewCore) -> Self {
        Self {
            review,
            rows: Vec::new(),
            blocks: Vec::new(),
            rendered: Vec::new(),
            changes: Vec::new(),
            under_lines: UnderLines::default(),
        }
    }

    fn laid(self, diagram: Option<Arc<str>>) -> Laid {
        Laid {
            rows: self.rows,
            blocks: self.blocks,
            diagram,
            rendered: self.rendered,
            changes: self.changes,
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
            review.steps_that_started_read(),
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
        self.finding_rows_of_the_whole_change();
        self.rows.push(PageRow::Toolbar(Arc::new(Toolbar {
            run_ai_review: review.ai_review_stage().action,
            comment_on_file: None,
            fold: None,
            full_diff: None,
            ignores_whitespace: None,
            wraps_lines: None,
            change: None,
        })));
    }

    /// What the AI review found about the change as a whole, under the first screen's text.
    fn finding_rows_of_the_whole_change(&mut self) {
        let findings = self.review.findings_here().to_vec();
        self.rows.extend(
            findings
                .into_iter()
                .map(|view| finding_row(view, ThreadRowPlace::AfterCode)),
        );
    }

    fn overview_rows(&mut self) {
        self.rows
            .push(PageRow::Overview(Arc::new(self.review.overview_page())));
        self.finding_rows_of_the_whole_change();
        self.rows.push(PageRow::Toolbar(Arc::new(Toolbar {
            run_ai_review: self.review.ai_review_stage().action,
            comment_on_file: None,
            fold: None,
            full_diff: None,
            ignores_whitespace: None,
            wraps_lines: None,
            change: None,
        })));
    }

    /// A file screen is a step page with no text: its toolbar, the file's boxes, the way on.
    fn file_rows(&mut self, path: &FilePath, page: FilePage) {
        let review = self.review;
        self.rows.push(PageRow::Toolbar(Arc::new(Toolbar {
            run_ai_review: None,
            // A picture has its own button for it.
            comment_on_file: (!page.pieces.iter().any(|piece| piece.is_image))
                .then(|| path.clone()),
            fold: None,
            full_diff: page.since_read.as_ref().map(|since| FullDiffControl {
                subject: ReadSubject::File(path.clone()),
                is_showing_full_diff: since.showing_full_diff,
            }),
            ignores_whitespace: Some(review.display_options().ignores_whitespace()),
            wraps_lines: Some(review.display_options().wraps_lines()),
            change: page.change,
        })));
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
        for block in FileBlock::group(page.pieces) {
            if !self.holds_file_comments(&block) {
                let on_file = self.under_lines.take_whole_file(&block.path);
                self.hang(on_file);
            }
            let screen = review.screen().clone();
            let fold = BoxFold {
                is_folded: review.is_box_folded(&screen, &block.path),
                screen,
                path: block.path.clone(),
            };
            self.block_rows(block, Some(fold), shown);
        }
        self.rows_after_code(after_code);
        self.rows.push(screen_footer(
            review,
            review.file_to_tick_on_screen().map(ReadSubject::File),
        ));
    }

    /// A picture and a rendered file hold what is said of the whole file under themselves.
    fn holds_file_comments(&self, block: &FileBlock) -> bool {
        block.pieces.iter().any(|piece| piece.is_image)
            || self
                .view_switch(block)
                .is_some_and(|switch| switch.view == FileView::Rendered)
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
        let diagram_row = diagram.as_ref().map(|_| DiagramRow {
            step: id.clone(),
            notes_button: review.notes_button(&NoteSubject::StepDiagram(id.clone())),
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
            run_ai_review: None,
            comment_on_file: None,
            fold: (folds && has_code).then(|| FoldControl {
                step: id.clone(),
                is_folded: page.is_folded,
                piece_count: page.pieces.len(),
            }),
            full_diff: page.since_read.as_ref().map(|since| FullDiffControl {
                subject: ReadSubject::Step(id.clone()),
                is_showing_full_diff: since.showing_full_diff,
            }),
            ignores_whitespace: has_code.then(|| review.display_options().ignores_whitespace()),
            wraps_lines: has_code.then(|| review.display_options().wraps_lines()),
            change: None,
        })));
        let footer = screen_footer(
            review,
            (!page.is_ticked).then(|| ReadSubject::Step(id.clone())),
        );
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
        for block in FileBlock::group(page.pieces) {
            let screen = Screen::Step(id.clone());
            let fold = BoxFold {
                is_folded: review.is_box_folded(&screen, &block.path),
                screen,
                path: block.path.clone(),
            };
            self.block_rows(block, Some(fold), shown);
        }
        self.rows_after_code(after_code);
        self.rows.push(footer);
        diagram
    }

    fn safety_net_rows(&mut self, page: SafetyNetPage) {
        let review = self.review;
        self.rows.push(PageRow::SafetyNetIntro);
        self.rows.push(PageRow::Toolbar(Arc::new(Toolbar {
            run_ai_review: None,
            comment_on_file: None,
            fold: None,
            full_diff: None,
            ignores_whitespace: Some(review.display_options().ignores_whitespace()),
            wraps_lines: Some(review.display_options().wraps_lines()),
            change: None,
        })));
        let after_code = self.hang_threads_and_comments(page.threads);
        for block in FileBlock::group(page.pieces) {
            self.block_rows(block, None, DiffShown::Full);
        }
        self.rows_after_code(after_code);
        self.rows.push(screen_footer(review, None));
    }

    /// Readies what hangs under the lines of the screen; returns what has no line to hang under.
    fn hang_threads_and_comments(&mut self, threads: Vec<ThreadView>) -> AfterCode {
        let (under_a_line, threads_after_code): (Vec<_>, Vec<_>) = threads
            .into_iter()
            .partition(|thread| thread.anchor.is_some());
        let (findings_under_a_line, findings_after_code): (Vec<_>, Vec<_>) = self
            .review
            .findings_here()
            .iter()
            .cloned()
            .partition(|view| view.anchor.is_some());
        self.under_lines = UnderLines::of(
            under_a_line
                .into_iter()
                .map(UnderLine::Thread)
                .chain(findings_under_a_line.into_iter().map(UnderLine::Finding))
                .chain(comments_here(self.review)),
        );
        AfterCode {
            threads: threads_after_code,
            findings: findings_after_code,
        }
    }

    /// Threads, then findings, each finding with the comment box and the queued comment made from it,
    /// since no row of code shows their lines either.
    fn rows_after_code(&mut self, after_code: AfterCode) {
        self.rows.extend(
            after_code
                .threads
                .into_iter()
                .map(|view| thread_row(view, ThreadRowPlace::AfterCode)),
        );
        let left = self.under_lines.take_findings_left();
        for view in after_code.findings.into_iter().chain(left) {
            let comments = CommentPlace::of_finding(&view.finding.anchor)
                .map_or_else(Vec::new, |place| {
                    self.under_lines.take_comments(&Hangs::of_comment(&place))
                });
            self.rows.push(finding_row(view, ThreadRowPlace::AfterCode));
            self.hang(comments);
        }
    }

    fn block_rows(&mut self, block: FileBlock, fold: Option<BoxFold>, shown: DiffShown) {
        let switch = self.view_switch(&block);
        if fold.as_ref().is_some_and(|fold| fold.is_folded) {
            self.folded_block_rows(&block, fold, switch);
            self.blocks.push(block);
            return;
        }
        if let Some(switch) = switch
            .clone()
            .filter(|switch| switch.view == FileView::Rendered)
        {
            self.rendered_block_rows(block, fold, switch);
            return;
        }
        let heading = heading_of;
        let has_no_change_since_read = |piece: &PieceDiff| {
            shown == DiffShown::ChangeSinceRead
                && piece.hunks.is_empty()
                && piece.binary_file.is_none()
        };
        let wraps = self.review.display_options().wraps_lines();
        let labels = self.label_switch(&block);
        let shows_labels = labels
            .as_ref()
            .is_some_and(|switch| switch.labels == PieceLabels::Shown);
        let row_id = if let [piece] = block.pieces.as_slice() {
            self.rows.push(PageRow::PieceHeader(Arc::new(PieceHeading {
                fold,
                view: switch.clone(),
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
                if switch.is_none() {
                    self.rows.push(PageRow::Image {
                        is_loaded: self.review.display_options().image_preview(&path).is_some(),
                        path: path.clone(),
                    });
                }
                let here = self.under_lines.take_whole_file(&path);
                self.hang(here);
            }
            if piece.changes_only_whitespace {
                self.rows.push(PageRow::OnlyWhitespace);
            }
            piece.id.to_string()
        } else {
            self.rows.push(PageRow::PieceHeader(Arc::new(PieceHeading {
                view: switch.clone(),
                labels: labels.clone(),
                ..whole_file_heading(&block, fold)
            })));
            for piece in block.without_rows() {
                if shows_labels {
                    self.rows
                        .push(PageRow::PieceMarker(Arc::new(heading(piece))));
                }
                if has_no_change_since_read(piece) {
                    self.rows.push(PageRow::NoChangeSinceRead);
                }
                if piece.changes_only_whitespace {
                    self.rows.push(PageRow::OnlyWhitespace);
                }
            }
            block.path.to_string()
        };
        let shows_markers = shows_labels;
        let hunks = block.hunks();
        for (index, hunk) in hunks.iter().enumerate() {
            if let Some(gap) = hidden_lines_before(hunks, index) {
                self.rows.push(PageRow::HunkGap(Arc::new(HunkGapRow::new(
                    format!("{row_id}-above-{index}"),
                    gap,
                ))));
            }
            let first_row = self.rows.len();
            for line in &hunk.rows {
                if shows_markers
                    && let Some((piece, is_resumed)) = block.piece_starting_at(&line.location)
                {
                    // A piece that starts again after one inside it keeps its buttons on its first label only.
                    let marker = if is_resumed {
                        PieceHeading {
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
            self.changes.push(PageChange::new(
                ChangeFile {
                    path: block.path.clone(),
                    view: switch.clone(),
                    labels: labels.clone(),
                },
                hunk.changes.clone(),
                &self.changes,
                first_row..self.rows.len(),
                (hunk.more_above.clone(), hunk.more_below.clone()),
            ));
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

    /// The piece labels' toggle of a code box of several pieces, set to what the box shows.
    fn label_switch(&self, block: &FileBlock) -> Option<LabelSwitch> {
        if block.has_one_piece() {
            return None;
        }
        Some(LabelSwitch {
            path: block.path.clone(),
            labels: self
                .review
                .display_options()
                .piece_labels(&block.path, &block.pieces),
        })
    }

    /// The Code / Rendered switch of a box whose file can be rendered, set to the view it shows.
    fn view_switch(&self, block: &FileBlock) -> Option<ViewSwitch> {
        let is_image = block.pieces.iter().any(|piece| piece.is_image);
        let rendering = Rendering::of(&block.path, is_image, block.lines().next().is_some())?;
        Some(ViewSwitch {
            path: block.path.clone(),
            view: self
                .review
                .display_options()
                .file_view(&block.path, rendering),
        })
    }

    /// A rendered box shows its file instead of its lines, and takes no line comments. What hangs
    /// under its lines follows the file, so a thread stays readable and answerable in either view.
    fn rendered_block_rows(&mut self, block: FileBlock, fold: Option<BoxFold>, switch: ViewSwitch) {
        let header = match block.pieces.as_slice() {
            [piece] => PieceHeading {
                fold,
                view: Some(switch.clone()),
                ..heading_of(piece)
            },
            _ => PieceHeading {
                view: Some(switch.clone()),
                ..whole_file_heading(&block, fold)
            },
        };
        let header_row = self.rows.len();
        self.rows.push(PageRow::PieceHeader(Arc::new(header)));
        let path = block.path.clone();
        let display_options = self.review.display_options();
        if block.pieces.iter().any(|piece| piece.is_image) {
            self.rows.push(PageRow::Image {
                is_loaded: display_options.image_preview(&path).is_some(),
                path: path.clone(),
            });
        } else {
            match display_options.rendered_markdown(&path) {
                Some(blocks) => self
                    .rows
                    .extend(blocks.iter().cloned().map(PageRow::RenderedBlock)),
                None => self.rows.push(PageRow::RenderedFileWaiting),
            }
        }
        let mut here = self.under_lines.take_whole_file(&path);
        for line in block.lines() {
            here.extend(self.under_lines.take_at(&line.location));
        }
        self.hang(here);
        self.changes.push(PageChange::new(
            ChangeFile {
                path: path.clone(),
                view: Some(switch),
                labels: None,
            },
            Vec::new(),
            &self.changes,
            header_row..self.rows.len(),
            (None, None),
        ));
        self.rows.push(PageRow::PieceEnd);
        self.rendered.push(path);
        self.blocks.push(block);
    }

    /// A folded box keeps its header and the labels of its pieces, so the step still says what it covers.
    fn folded_block_rows(
        &mut self,
        block: &FileBlock,
        fold: Option<BoxFold>,
        switch: Option<ViewSwitch>,
    ) {
        if let [piece] = block.pieces.as_slice() {
            self.rows.push(PageRow::PieceHeader(Arc::new(PieceHeading {
                fold,
                view: switch,
                ..heading_of(piece)
            })));
        } else {
            self.rows.push(PageRow::PieceHeader(Arc::new(PieceHeading {
                view: switch,
                ..whole_file_heading(block, fold)
            })));
            for piece in block.pieces_in_order() {
                let mut marker = heading_of(piece);
                marker.label = marker.label.named_as_part(self.part_name(piece));
                self.rows.push(PageRow::PieceMarker(Arc::new(marker)));
            }
        }
        self.rows.push(PageRow::PieceEnd);
    }

    /// What a folded box calls a numbered part, which has no code under it to say where it is: the
    /// Markdown section it sits in, else the line it starts on.
    fn part_name(&self, piece: &PieceDiff) -> Option<Arc<str>> {
        if !piece.id.is_hunk() {
            return None;
        }
        let first_line = piece
            .hunks
            .iter()
            .flat_map(|hunk| &hunk.rows)
            .skip_while(|row| matches!(row.kind, LineKind::Context { .. }))
            .find(|row| row.location.side == Side::Head)
            .map(|row| usize::from(row.location.line))?;
        let is_markdown = Rendering::of(piece.id.path(), false, true) == Some(Rendering::Markdown);
        let section = is_markdown
            .then(|| {
                let text = self
                    .review
                    .display_options()
                    .files_at_head()
                    .text(piece.id.path())?;
                section_title_at(text, first_line)
            })
            .flatten();
        Some(
            section
                .map_or_else(|| format!("line {first_line}"), str::to_owned)
                .into(),
        )
    }

    /// Hangs `here` under the row above.
    fn hang(&mut self, here: Vec<UnderLine>) {
        let review = self.review;
        self.rows
            .extend(here.into_iter().map(|item| item.into_row(review)));
    }
}

fn heading_of(piece: &PieceDiff) -> PieceHeading {
    PieceHeading {
        fold: None,
        piece: piece.id.clone(),
        label: PieceLabel::of(&piece.id),
        path: piece.id.path().clone(),
        repin_label: piece.repin_status.and_then(repin_label),
        code_link: piece.code_link().cloned(),
        view: None,
        labels: None,
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
        repin_label: None,
        code_link: block.lines().next().map(|line| line.code_link.clone()),
        view: None,
        labels: None,
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
        notes_button: review.notes_button(&NoteSubject::ReviewerSummary),
        picture_buttons: Arc::new(picture_buttons),
        markdown: summary,
    }
}

fn screen_footer(review: &ReviewCore, ticks: Option<ReadSubject>) -> PageRow {
    let place = review.reading_place();
    let lead = review.tour_lead();
    let progress = review.progress();
    let leads_on = lead != TourLead::Nowhere && place.next.is_none() && ticks.is_none();
    PageRow::ScreenFooter(Arc::new(ScreenFooter {
        ticks,
        has_previous: place.has_previous,
        next: place.next,
        file_place: place.file_number.map(|number| (number, place.file_count)),
        lead: leads_on.then_some(lead),
        unread_left: progress.total - progress.read,
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

fn finding_row(view: FindingView, place: ThreadRowPlace) -> PageRow {
    let can_fix = !view.fix.as_ref().is_some_and(FixState::is_in_progress);
    let can_decide = !view.fix.as_ref().is_some_and(FixState::blocks_decisions);
    let fix_line = view.fix.as_ref().map(|state| match &view.fix_note {
        Some(note) => format!("{}. Your note: {}", state.line(), note.as_str()),
        None => state.line(),
    });
    let fixes_offer = match &view.fix {
        Some(FixState::ToSend) => FixesOffer::Remove,
        Some(state) if state.is_in_progress() => FixesOffer::None,
        Some(_) | None => FixesOffer::Add,
    };
    let can_answer_fix = view.fix.as_ref() == Some(&FixState::Asked);
    let can_ask_why = !view
        .thread
        .as_ref()
        .is_some_and(|thread| thread.state().is_in_progress());
    PageRow::Finding(Arc::new(FindingRow {
        id: view.finding.id,
        severity: view.finding.severity,
        body: view.finding.body.into(),
        place,
        fix: fix_line.map(Into::into),
        can_fix,
        can_decide,
        fixes_offer,
        can_mark_fixed: matches!(view.fix, Some(FixState::Fixed { .. })),
        can_answer_fix,
        asked: view
            .thread
            .as_ref()
            .and_then(crate::viewer::FindingThread::asked_line)
            .map(Into::into),
        thread: view
            .thread
            .as_ref()
            .map_or_else(|| Arc::from([]), |thread| thread.messages().into()),
        can_ask_why,
        outdated: view.finding.outdated.map(|outdated| outdated.before.into()),
    }))
}

/// What follows the code of a screen: what has no line to hang under.
struct AfterCode {
    threads: Vec<ThreadView>,
    findings: Vec<FindingView>,
}

/// What hangs under a row of code, in this order: threads, findings, queued comments, comment boxes.
enum UnderLine {
    Thread(ThreadView),
    Finding(FindingView),
    Queued(QueuedComment),
    CommentBox(CommentPlace),
}

impl UnderLine {
    fn at(&self) -> Option<Hangs> {
        match self {
            Self::Thread(view) => view.anchor.clone().map(Hangs::Line),
            Self::Finding(view) => view.anchor.clone().map(Hangs::Line),
            Self::Queued(queued) => Some(Hangs::of_comment(&queued.draft.place)),
            Self::CommentBox(place) => Some(Hangs::of_comment(place)),
        }
    }

    fn into_row(self, review: &ReviewCore) -> PageRow {
        match self {
            Self::Thread(view) => thread_row(view, ThreadRowPlace::UnderLine),
            Self::Finding(view) => finding_row(view, ThreadRowPlace::UnderLine),
            Self::Queued(queued) => PageRow::QueuedComment(Arc::new(queued)),
            Self::CommentBox(place) => comment_box_row(place, review),
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

    /// The findings whose line no row of the screen shows, in the order of their ids.
    fn take_findings_left(&mut self) -> Vec<FindingView> {
        let mut left: Vec<FindingView> = Vec::new();
        for items in self.by_line.values_mut() {
            let (findings, others): (Vec<_>, Vec<_>) = std::mem::take(items)
                .into_iter()
                .partition(|item| matches!(item, UnderLine::Finding(_)));
            *items = others;
            left.extend(findings.into_iter().filter_map(|item| match item {
                UnderLine::Finding(view) => Some(view),
                _ => None,
            }));
        }
        left.sort_by_key(|view| view.finding.id.to_string());
        left
    }

    /// The comment boxes and queued comments at `at` that no row took.
    fn take_comments(&mut self, at: &Hangs) -> Vec<UnderLine> {
        let Some(items) = self.by_line.get_mut(at) else {
            return Vec::new();
        };
        let (comments, others): (Vec<_>, Vec<_>) = std::mem::take(items)
            .into_iter()
            .partition(|item| matches!(item, UnderLine::CommentBox(_) | UnderLine::Queued(_)));
        *items = others;
        comments
    }

    fn take(&mut self, at: &Hangs) -> Vec<UnderLine> {
        self.by_line.remove(at).unwrap_or_default()
    }
}

/// Comments of the screen; threads come with the page.
fn comments_here(review: &ReviewCore) -> Vec<UnderLine> {
    let queued = review
        .queued_comments()
        .iter()
        .cloned()
        .map(UnderLine::Queued);
    let boxes = review
        .comment_boxes()
        .map(|comment_box| UnderLine::CommentBox(comment_box.draft.place.clone()));
    queued.chain(boxes).collect()
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
