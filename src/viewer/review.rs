use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;

use crate::code_source::CodeSource;
use crate::coverage;
use crate::pieces::{self, ChangedFile, PieceId};
use crate::pull_request::{PullRequestUrl, Sha};
use crate::tour::{Step, Tour};

use std::path::PathBuf;

use crate::config::AskSettings;

use super::ask::{Ask, Place};
use super::ask_context;
use super::ask_target::{AskTarget, Unavailable};
use super::code_link::{CodeLinks, CodeOpener, WorkingCopy};
use super::comment_desk::{CommentDesk, TextBox, TourFreshness};
use super::comments::{
    CommentDraft, CommentOutcome, CommentPlace, CommentRefusal, CommentRequest, CommentResult,
    QueuedComment, SendMode,
};
use super::diff_display::{DiffDisplay, FilesAtHead, FilesAtHeadRequest};
use super::display_options::DisplayOptions;
use super::file_block::FileBlock;
use super::finish_review::{
    ApproveWarning, FinishOutcome, FinishRefusal, FinishRequest, FinishResult, ReviewVerdict,
    SubmittedReview,
};
use super::hand_offs::{Audience, HandOffState, HandOffs};
use super::highlight::{HighlightRequest, Highlights};
use super::image_preview::ImagePreviewRequest;
use super::jump::{self, JumpTarget, PaletteAction, PaletteGroup};
use super::markdown::Markdown;
use super::outdated_lines::FollowedLines;
use super::ports::{AgentLaunch, GitHubError};
use super::pr_notes::{Note, NoteSubject, NotesButton, PrNotes, ToNotes, ToNotesOutcome};
use super::problems::{Problem, ProblemKind, Problems, Read};
use super::refresh::RefreshBrief;
use super::review_state::{RepinRecord, ReviewState, ReviewStateFile};
use super::row_selection::{RowSelection, SelectedLines};
use super::saved_state::SavedState;
use super::step_page::{DiffLine, LineLocation, PieceDiff, SafetyNetPage, SinceRead, StepPage};
use super::thread_actions::{
    ThreadAction, ThreadActionOutcome, ThreadActionRequest, ThreadActionResult,
};
use super::threads::{self, ReviewThread, ThreadId, ThreadPlacement};
use super::typing_save::TypingSave;
use super::viewed_sync::{Coverage, SyncOutcome, SyncRequest, ViewedSync};
use crate::file_path::FilePath;
use crate::ide::{Ide, NoIde};
use crate::repin::RepinStatus;

pub use crate::tour::StepId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Screen {
    Summary,
    Step(StepId),
    /// After the last step: every change no step shows.
    SafetyNet,
}

/// One row of the step tree. Rows come in reading order, so indenting by `depth` draws the tree.
pub struct SidebarEntry<'a> {
    pub id: StepId,
    pub title: &'a str,
    pub depth: usize,
    pub is_current: bool,
    pub is_ticked: bool,
    pub thread_count: usize,
    /// Shown greyed: every piece of the step is gone since the last version.
    pub is_gone: bool,
    /// The step carries a check hint.
    pub has_check: bool,
    pub is_skim: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReadProgress {
    pub read: usize,
    pub total: usize,
}

impl ReadProgress {
    /// How much of the tour is read, from 0 to 1: the length of the trail.
    pub fn share(self) -> f32 {
        if self.total == 0 {
            return 0.;
        }
        self.read.min(self.total) as f32 / self.total as f32
    }

    /// Every step read; a tour without steps has nothing to complete.
    pub fn is_complete(self) -> bool {
        self.total > 0 && self.read >= self.total
    }
}

/// A step the reader has not read yet, named for a button that leads there.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct UnreadStep {
    pub id: StepId,
    /// Its title is markdown.
    pub title: String,
}

/// What the summary leads the reader to: the start of the tour, the first step not read yet, or
/// once every step is read, finishing the review.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TourLead {
    /// A tour with no steps leads nowhere.
    Nowhere,
    Start(UnreadStep),
    Continue(UnreadStep),
    Finish {
        steps: usize,
        /// The PR takes a review from the viewer; an own PR with nothing to send does not.
        can_finish: bool,
    },
}

/// One top-level step of the tour as the summary lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteStop {
    pub id: StepId,
    /// Markdown.
    pub title: String,
    /// Markdown.
    pub why: String,
    /// How many steps sit under it.
    pub details: usize,
    pub is_read: bool,
    pub has_check: bool,
}

/// What the summary counts about the PR, all of it known without GitHub but the threads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SummaryStats {
    pub steps: usize,
    pub files: usize,
    pub added_lines: usize,
    pub removed_lines: usize,
    pub open_threads: usize,
}

/// Where the open screen sits in the reading order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadingPlace {
    /// Counted among the steps from 1; `None` on the summary and the safety net.
    pub step_number: Option<usize>,
    pub step_count: usize,
    pub has_previous: bool,
    pub next: Option<NextScreen>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum NextScreen {
    /// Its title is markdown.
    Step {
        title: String,
    },
    SafetyNet,
}

/// What the sidebar counts for the safety net, cheap enough to ask on every change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SafetyNetSize {
    pub pieces: usize,
    pub threads: usize,
}

/// The tour was written for an older head than the PR has now, so it may be stale.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OldHead {
    pub tour_head: Sha,
    pub pr_head: Sha,
}

pub struct Review {
    tour: Tour,
    pr: PullRequestUrl,
    base: Sha,
    head: Sha,
    /// Unknown until GitHub answers.
    pr_head: Option<Sha>,
    /// What GitHub last said of the viewer's sent review; kept as it was while a read fails.
    last_review: Option<SubmittedReview>,
    files: Vec<ChangedFile>,
    /// Summary, then parent, its children, the next parent (from the big picture to the details),
    /// then the safety net when it holds anything.
    steps_in_reading_order: Vec<Screen>,
    has_uncovered_changes: bool,
    pulled_threads: Vec<ReviewThread>,
    followed_lines: BTreeMap<ThreadId, FollowedLines>,
    threads: ThreadPlacement,
    /// Threads fold by default when resolved, outdated or started by a bot; these are the ones the reviewer flipped.
    thread_fold_flipped: BTreeSet<ThreadId>,
    screen: Screen,
    state: SavedState,
    /// Skim and ticked steps start folded; these are the steps whose fold the reviewer flipped.
    /// Not saved: a reopened app shows steps folded again.
    fold_flipped: BTreeSet<StepId>,
    /// File boxes of a step opened or folded against the step's fold, by step and path.
    box_fold_flipped: BTreeSet<(StepId, FilePath)>,
    /// The tour's own folder, whose pictures its summary shows and whose files its links open.
    tour_files: PathBuf,
    viewed_sync: ViewedSync,
    problems: Problems,
    /// The diff from each older head some step was read at to the tour's head.
    /// A head that cannot be read (gone after a force push) is left out: those steps show the full diff.
    changes_since: BTreeMap<Sha, Vec<ChangedFile>>,
    /// Steps read before whose full diff the reviewer asked for. Not saved.
    full_diff_shown: BTreeSet<StepId>,
    /// Until told, code opens in the IDE with a note that its lines may differ.
    working_copy: WorkingCopy,
    comment_desk: CommentDesk,
    display_options: DisplayOptions,
    pr_notes: PrNotes,
}

impl Review {
    /// Code is read at the tour's own base and head, not the PR's current ones.
    pub fn open(
        tour: Tour,
        source: &dyn CodeSource,
        state_file: ReviewStateFile,
    ) -> anyhow::Result<Self> {
        let (state, saved_review_kept_as) = state_file.load_or_set_aside()?;
        let tour_files = state_file.tour_files().to_owned();
        let pr: PullRequestUrl = tour.pr.parse()?;
        let (base, head) = (tour.base.clone(), tour.head.clone());
        let files = pieces::cut(source, &base, &head)?;
        let steps = tour
            .steps()
            .into_iter()
            .map(|step| Screen::Step(step.id.clone()));
        let steps_in_reading_order = std::iter::once(Screen::Summary).chain(steps).collect();
        let has_uncovered_changes = threads::uncovered(&tour, &files);
        let read_heads: BTreeSet<&Sha> = tour
            .steps()
            .iter()
            .filter_map(|step| read_head_of(&state, &step.id.clone()))
            .filter(|read_head| **read_head != tour.head)
            .collect();
        let changes_since = read_heads
            .into_iter()
            .filter_map(|read_head| {
                let files = pieces::cut(source, read_head, &head).ok()?;
                Some((read_head.clone(), files))
            })
            .collect();
        let display_options = DisplayOptions::for_code(base.clone(), head.clone());
        Ok(Self {
            tour,
            pr,
            base,
            head,
            pr_head: None,
            last_review: None,
            files,
            steps_in_reading_order,
            has_uncovered_changes,
            pulled_threads: Vec::new(),
            followed_lines: BTreeMap::new(),
            threads: ThreadPlacement::default(),
            thread_fold_flipped: BTreeSet::new(),
            screen: Screen::Summary,
            state: SavedState::new(state, state_file),
            fold_flipped: BTreeSet::new(),
            box_fold_flipped: BTreeSet::new(),
            viewed_sync: ViewedSync::default(),
            problems: {
                let mut problems = Problems::default();
                if let Some(reason) = source.fallback_reason() {
                    problems.raise(Problem::CodeOnGitHub(reason.to_owned()));
                }
                if let Some(backup) = saved_review_kept_as {
                    problems.raise(Problem::SavedReviewNotRead(backup));
                }
                problems
            },
            changes_since,
            full_diff_shown: BTreeSet::new(),
            working_copy: WorkingCopy::Elsewhere,
            comment_desk: CommentDesk::default(),
            display_options,
            pr_notes: PrNotes::default(),
            tour_files,
        })
    }

    /// Colouring reads whole files, so the app runs this off the main thread and hands the result to `set_highlights`.
    pub fn highlight_request(&self) -> HighlightRequest {
        HighlightRequest::for_files(&self.files, &self.tour.base, &self.tour.head)
    }

    /// Context lines come from the files at head, read off the main thread like the colours.
    pub fn files_at_head_request(&self) -> FilesAtHeadRequest {
        FilesAtHeadRequest::for_files(&self.files, &self.tour.head)
    }

    /// Files read at another head, e.g. before a re-pin, are dropped. Says whether they were kept.
    pub fn set_files_at_head(&mut self, files_at_head: FilesAtHead) -> bool {
        let kept = self.display_options.set_files_at_head(files_at_head);
        if kept {
            self.place_threads();
        }
        kept
    }

    /// The pictures of the screen that are not loaded yet; the app loads them off the main thread
    /// only while their step is shown.
    pub fn image_preview_requests(&self) -> Vec<ImagePreviewRequest> {
        let mut requests: Vec<ImagePreviewRequest> = Vec::new();
        for piece in self.screen_pieces().iter().filter(|piece| piece.is_image) {
            let path = piece.id.path();
            let is_wanted = self.display_options.image_preview(path).is_none()
                && !requests.iter().any(|request| request.path() == path);
            let file = self.files.iter().find(|file| file.path == *path);
            if let (true, Some(file)) = (is_wanted, file) {
                requests.push(ImagePreviewRequest::for_file(file, &self.base, &self.head));
            }
        }
        requests
    }

    pub fn display_options(&self) -> &DisplayOptions {
        &self.display_options
    }

    /// Nothing of it is saved, so it changes freely, even while a re-pin runs.
    pub fn display_options_mut(&mut self) -> &mut DisplayOptions {
        &mut self.display_options
    }

    pub fn screen(&self) -> &Screen {
        &self.screen
    }

    pub fn pr(&self) -> &PullRequestUrl {
        &self.pr
    }

    /// For a failure the reviewer must know about; it stays until its kind is cleared.
    pub fn raise_problem(&mut self, problem: Problem) {
        self.problems.raise(problem);
    }

    pub fn clear_problem(&mut self, kind: ProblemKind) {
        self.problems.clear(kind);
    }

    /// The outcome of a Show agent: a terminal that did not come up is a problem until one does.
    pub fn show_agent_result(&mut self, failure: Option<&anyhow::Error>) {
        match failure {
            Some(error) => {
                self.raise_problem(Problem::ShowAgentFailed(format!("{error:#}")));
            }
            None => self.clear_problem(ProblemKind::ShowAgentFailed),
        }
    }

    pub fn show_agent_failed(&self) -> bool {
        self.problems
            .list()
            .any(|problem| problem.kind() == ProblemKind::ShowAgentFailed)
    }

    pub fn could_not_read(&mut self, read: Read, error: &impl fmt::Display) {
        self.raise_problem(Problem::NotRead(read, format!("{error:#}")));
    }

    pub fn ide_not_started(&mut self, error: &impl fmt::Display) {
        self.raise_problem(Problem::IdeNotStarted(format!("{error:#}")));
    }

    pub fn ide_started(&mut self) {
        self.clear_problem(ProblemKind::IdeNotStarted);
    }

    pub fn pr_head_is(&mut self, head: Sha) {
        self.clear_problem(ProblemKind::NotRead(Read::PrHead));
        self.pr_head = Some(head);
    }

    pub fn old_head(&self) -> Option<OldHead> {
        self.pr_head
            .as_ref()
            .filter(|pr_head| **pr_head != self.head)
            .map(|pr_head| OldHead {
                tour_head: self.head.clone(),
                pr_head: pr_head.clone(),
            })
    }

    pub fn title(&self) -> &str {
        &self.tour.title
    }

    pub fn summary(&self) -> &[String] {
        &self.tour.summary
    }

    /// Under the summary's bullets and above the PR description. `None` when the tour has none.
    pub fn reviewer_summary(&self) -> Option<Markdown> {
        let body = &self.tour.reviewer_summary;
        let markdown = Markdown::parse_with_files(body, &self.tour_files);
        (!body.trim().is_empty()).then(|| markdown.nested_one_level())
    }

    /// The PR's body, under the summary: the one GitHub gave last, until it answers again.
    /// `None` for a PR with no description.
    pub fn pr_description(&self) -> Option<Markdown> {
        let body = self.state.pr_description();
        (!body.trim().is_empty()).then(|| Markdown::parse(body).nested_one_level())
    }

    /// `None` when the tour has no such diagram, picture or summary.
    pub fn notes_button(&self, subject: &NoteSubject) -> Option<NotesButton> {
        let note = self.note(subject)?;
        Some(
            self.pr_notes
                .button(subject, self.state.pr_description(), &note),
        )
    }

    /// `None` while the subject is being added, or when the description holds it.
    pub fn start_adding_to_notes(&mut self, subject: &NoteSubject) -> Option<ToNotes> {
        let note = self.note(subject)?;
        if note.is_in(self.state.pr_description()) {
            return None;
        }
        self.pr_notes.start(&self.pr, subject, note)
    }

    pub fn apply_to_notes(&mut self, outcome: ToNotesOutcome) -> anyhow::Result<()> {
        match self.pr_notes.finish(outcome) {
            Some(description) => self.set_pr_description(&description),
            None => Ok(()),
        }
    }

    fn note(&self, subject: &NoteSubject) -> Option<Note> {
        match subject {
            NoteSubject::StepDiagram(step) => {
                let found = self.step(step)?;
                Some(Note::diagram(&found.title, found.diagram.as_deref()?))
            }
            NoteSubject::SummaryPicture(path) => {
                let summary = self.reviewer_summary()?;
                let (_, alt) = summary
                    .pictures()
                    .into_iter()
                    .find(|(picture, _)| picture == path)?;
                Note::picture(path, alt)
            }
            NoteSubject::ReviewerSummary => {
                let summary = &self.tour.reviewer_summary;
                if summary.trim().is_empty() {
                    return None;
                }
                Note::reviewer_summary(summary, &self.tour_files)
            }
        }
    }

    /// Kept in the state file only when it changed, since every window focus pulls it again.
    pub fn set_pr_description(&mut self, body: &str) -> anyhow::Result<()> {
        self.clear_problem(ProblemKind::NotRead(Read::PrDescription));
        if self.state.pr_description() != body {
            self.state.record(|state| state.saw_pr_description(body))?;
        }
        Ok(())
    }

    pub fn current_step(&self) -> Option<StepPage<'_>> {
        let Screen::Step(id) = &self.screen else {
            return None;
        };
        let step = self.step(id)?;
        let links = self.links();
        let display = self.display(&links);
        let mut page = StepPage::of(step, &self.tour, &self.files, &display, self.is_folded(id));
        page.is_ticked = self.is_ticked(id);
        page.threads = threads::views(
            self.threads.in_step(id),
            &page.pieces,
            &self.thread_fold_flipped,
            self.display_options.files_at_head(),
            self.state.reply_drafts(),
            self.comment_desk.comment_edits(),
            self.state.hand_offs(),
        );
        if let Some(record) = self.repin_record() {
            self.mark_repin_status(&mut page.pieces);
            page.gone_pieces = record.gone_count(id);
            page.text_may_be_old = page
                .pieces
                .iter()
                .any(|piece| piece.repin_status == Some(RepinStatus::Changed));
        }
        page.is_gone = self.is_gone(step);
        if let Some((read_head, since_files)) = self.changes_since_read(id) {
            let showing_full_diff = self.full_diff_shown.contains(id);
            page.since_read = Some(SinceRead {
                read_at: read_head.clone(),
                showing_full_diff,
            });
            if !showing_full_diff {
                let links_from_read_head = CodeLinks {
                    pr: &self.pr,
                    base: &read_head,
                    head: &self.head,
                };
                let since_display = self.display(&links_from_read_head);
                page.pieces = step
                    .pieces
                    .iter()
                    .flat_map(|pointer| coverage::shown_parts(pointer, &self.tour, &self.files))
                    .map(|part| {
                        let pr_file = self
                            .files
                            .iter()
                            .find(|file| file.path == *part.piece.id.path());
                        PieceDiff::since_read(&part, since_files, pr_file, &since_display)
                    })
                    .collect();
                self.mark_repin_status(&mut page.pieces);
            }
        }
        Some(page)
    }

    pub fn refresh_brief(&self) -> RefreshBrief {
        RefreshBrief::of(&self.tour, &self.files, self.state.last_repin())
    }

    fn changes_since_read(&self, id: &StepId) -> Option<(Sha, &[ChangedFile])> {
        let read_head = read_head_of(&self.state, id)?;
        let since_files = self.changes_since.get(read_head)?;
        Some((read_head.clone(), since_files))
    }

    /// Flips a step read before between the change since reading and the full diff, for the session.
    pub fn toggle_full_diff(&mut self, id: &StepId) {
        if !self.full_diff_shown.remove(id) {
            self.full_diff_shown.insert(id.clone());
        }
    }

    fn step(&self, id: &StepId) -> Option<&Step> {
        self.tour.find_step(id)
    }

    /// A record of a re-pin to another head says nothing about this tour any more.
    fn repin_record(&self) -> Option<&RepinRecord> {
        self.state
            .last_repin()
            .filter(|record| record.to_head == self.tour.head)
    }

    fn mark_repin_status(&self, pieces: &mut [PieceDiff]) {
        if let Some(record) = self.repin_record() {
            for piece in pieces {
                piece.repin_status = record.status_of(&piece.id);
            }
        }
    }

    fn is_gone(&self, step: &Step) -> bool {
        let id = step.id.clone();
        step.diagram.is_none()
            && self
                .repin_record()
                .is_some_and(|record| record.gone_count(&id) > 0)
            && step
                .pieces
                .iter()
                .all(|pointer| coverage::shown_parts(pointer, &self.tour, &self.files).is_empty())
    }

    pub fn head(&self) -> &Sha {
        &self.tour.head
    }

    /// What the AI is told about a place: pointers, the shown diff lines, step title and why.
    pub fn ask_context(&self, place: &Place) -> Option<String> {
        ask_context::context_for(&self.tour, &self.files, &self.links(), place)
    }

    /// A re-pin since the Ask changed, added or took away a piece it is about: its answer may be old.
    pub fn code_changed_since(&self, ask: &Ask) -> bool {
        if ask.head == self.tour.head {
            return false;
        }
        let Some(record) = self.repin_record() else {
            return false;
        };
        let moved = |piece: &PieceId| record.status_of(piece) != Some(RepinStatus::Same);
        match &ask.place {
            Place::Tour => false,
            Place::Step { step } => {
                record.gone_count(step) > 0
                    || self.step(step).is_some_and(|step| {
                        step.pieces
                            .iter()
                            .flat_map(|pointer| {
                                coverage::shown_parts(pointer, &self.tour, &self.files)
                            })
                            .any(|part| moved(&part.piece.id))
                    })
            }
            Place::Piece { piece, .. } => moved(piece),
            Place::Lines { pieces, .. } => pieces.iter().any(moved),
        }
    }

    /// Puts the Ask's latest answer into the comment box on its lines, after any text already
    /// there. The reviewer edits and sends it; nothing goes to GitHub here.
    pub fn use_as_comment(&mut self, ask: &Ask) -> Result<CommentPlace, CommentRefusal> {
        if matches!(ask.place, Place::Tour | Place::Step { .. }) {
            return Err(CommentRefusal::NoLines);
        }
        if ask.head != self.tour.head {
            return Err(CommentRefusal::AskIsOnOlderCode);
        }
        if self.tour_freshness() == TourFreshness::Behind {
            return Err(CommentRefusal::TourIsOld);
        }
        let selected = self.lines_of(&ask.place).ok_or(CommentRefusal::NoLines)?;
        if !selected.take_comment() {
            return Err(CommentRefusal::NotInsideHunk);
        }
        let answer = ask.latest_answer().ok_or(CommentRefusal::NoAnswerYet)?;
        CommentDesk::add_to_draft(&mut self.state, &selected, &self.head, answer)
    }

    /// The rows an Ask about lines or a piece is on, in its step at the tour head.
    fn lines_of(&self, place: &Place) -> Option<SelectedLines> {
        let (step, only_piece) = match place {
            Place::Tour | Place::Step { .. } => return None,
            Place::Piece { step, piece } => (Some(step), Some(piece)),
            Place::Lines { step, .. } => (step.as_ref(), None),
        };
        let pieces = match step {
            Some(id) => {
                let links = self.links();
                let display = self.display(&links);
                StepPage::of(self.step(id)?, &self.tour, &self.files, &display, false).pieces
            }
            None => self.safety_net()?.pieces,
        };
        let rows: Vec<DiffLine> = match only_piece {
            Some(id) => pieces
                .iter()
                .filter(|shown| shown.id == *id)
                .flat_map(|shown| shown.lines().cloned())
                .collect(),
            None => in_shown_order(pieces),
        };
        let (start, end) = match place {
            Place::Lines { start, end, .. } => (start.clone(), end.clone()),
            _ => (
                rows.first()?.location.clone(),
                rows.last()?.location.clone(),
            ),
        };
        let mut selection = RowSelection::start_at(start);
        selection.extend_to(end);
        selection.resolve(&rows)
    }

    /// The place an Ask about selected rows is about: the rows and the pieces holding them.
    /// What the comment box's Ask is about: its lines, on this screen. `None` for a comment on a
    /// file, or lines not on this screen.
    pub fn ask_place_of(&self, comment: &CommentPlace) -> Option<Place> {
        let CommentPlace::Lines { start, end } = comment else {
            return None;
        };
        let lines = self.screen_lines();
        let first = lines.iter().position(|line| &line.location == start)?;
        let last = lines.iter().position(|line| &line.location == end)?;
        let selected = SelectedLines::new(lines.get(first..=last)?.to_vec())?;
        Some(self.place_of_lines(&selected))
    }

    pub fn place_of_lines(&self, selected: &SelectedLines) -> Place {
        let (step, pieces) = match &self.screen {
            Screen::Step(id) => (
                Some(id.clone()),
                self.current_step()
                    .map(|page| page.pieces)
                    .unwrap_or_default(),
            ),
            _ => (
                None,
                self.safety_net()
                    .map(|page| page.pieces)
                    .unwrap_or_default(),
            ),
        };
        Place::Lines {
            step,
            start: selected.first().clone(),
            end: selected.last().clone(),
            pieces: pieces
                .iter()
                .filter(|piece| piece.lines().any(|line| selected.contains(&line.location)))
                .map(|piece| piece.id.clone())
                .collect(),
        }
    }

    /// The place the Ask panel opens on, ready only when an AI has code to read and the place is still in the tour.
    pub fn ask_target(
        &self,
        place: Place,
        label: String,
        clone: Option<PathBuf>,
        settings: &AskSettings,
    ) -> AskTarget {
        let Some(clone) = clone else {
            return AskTarget::Unavailable {
                place,
                label,
                why: Unavailable::NoClone,
            };
        };
        let Some(context) = self.ask_context(&place) else {
            return AskTarget::Unavailable {
                place,
                label,
                why: Unavailable::PlaceGone,
            };
        };
        AskTarget::Ready {
            launch: self.agent_launch(clone, settings, None),
            place,
            label,
            context,
        }
    }

    pub fn agent_launch(
        &self,
        cwd: PathBuf,
        settings: &AskSettings,
        resume: Option<String>,
    ) -> AgentLaunch {
        AgentLaunch {
            cwd,
            model: settings.model.clone(),
            effort: settings.effort.clone(),
            system_prompt: ask_context::system_prompt(&self.tour.pr),
            resume,
        }
    }

    pub fn sidebar(&self) -> Vec<SidebarEntry<'_>> {
        fn walk<'a>(
            steps: &'a [Step],
            depth: usize,
            current: Option<&StepId>,
            review: &Review,
            into: &mut Vec<SidebarEntry<'a>>,
        ) {
            for step in steps {
                let id = step.id.clone();
                into.push(SidebarEntry {
                    is_current: current == Some(&id),
                    is_ticked: review.is_ticked(&id),
                    thread_count: review.threads.in_step(&id).len(),
                    is_gone: review.is_gone(step),
                    has_check: step.check.is_some(),
                    is_skim: step.skim,
                    id,
                    title: &step.title,
                    depth,
                });
                walk(&step.children, depth + 1, current, review, into);
            }
        }
        let current = match &self.screen {
            Screen::Step(id) => Some(id),
            Screen::Summary | Screen::SafetyNet => None,
        };
        let mut entries = Vec::new();
        walk(&self.tour.steps, 0, current, self, &mut entries);
        entries
    }

    /// `None` when every change is in a step.
    pub fn safety_net(&self) -> Option<SafetyNetPage> {
        self.reading_order().contains(&Screen::SafetyNet).then(|| {
            let links = self.links();
            let mut page = SafetyNetPage::of(&self.tour, &self.files, &self.display(&links));
            self.mark_repin_status(&mut page.pieces);
            page.threads = threads::views(
                self.threads.in_safety_net(),
                &page.pieces,
                &self.thread_fold_flipped,
                self.display_options.files_at_head(),
                self.state.reply_drafts(),
                self.comment_desk.comment_edits(),
                self.state.hand_offs(),
            );
            page
        })
    }

    /// `None` when every change is in a step. Does not build the page.
    pub fn safety_net_size(&self) -> Option<SafetyNetSize> {
        self.reading_order()
            .contains(&Screen::SafetyNet)
            .then(|| SafetyNetSize {
                pieces: coverage::safety_net(&self.tour, &self.files).len(),
                threads: self.threads.in_safety_net().len(),
            })
    }

    fn display<'a>(&'a self, links: &'a CodeLinks<'a>) -> DiffDisplay<'a> {
        self.display_options.display(links)
    }

    /// Where a screen sits in the reading order, without building the order; the safety net is last.
    pub fn reading_position(&self, screen: &Screen) -> Option<usize> {
        match screen {
            Screen::SafetyNet => Some(self.steps_in_reading_order.len()),
            Screen::Summary | Screen::Step(_) => self
                .steps_in_reading_order
                .iter()
                .position(|shown| shown == screen),
        }
    }

    fn reading_order(&self) -> Vec<Screen> {
        let has_safety_net = self.has_uncovered_changes || !self.threads.in_safety_net().is_empty();
        self.steps_in_reading_order
            .iter()
            .cloned()
            .chain(has_safety_net.then_some(Screen::SafetyNet))
            .collect()
    }

    pub fn set_threads(&mut self, threads: &[ReviewThread]) {
        self.clear_problem(ProblemKind::ThreadsNotRead);
        self.pulled_threads = threads.to_vec();
        self.place_threads();
    }

    /// Outdated threads hang under the lines they were followed to; the others stay after the code.
    pub fn set_followed_lines(&mut self, followed: BTreeMap<ThreadId, FollowedLines>) {
        self.followed_lines = followed;
        self.place_threads();
    }

    /// The threads shown stay as they were, named as possibly old until a read goes through.
    pub fn threads_not_read(&mut self, error: &GitHubError) {
        self.raise_problem(Problem::ThreadsNotRead(format!("{error:#}")));
    }

    /// Placed by the rows a step shows, which depend on the files read at head, so again when those arrive.
    fn place_threads(&mut self) {
        let links = self.links();
        let display = DiffDisplay {
            highlights: &Highlights::default(),
            files_at_head: self.display_options.files_at_head(),
            more_context: &HashMap::new(),
            ignores_whitespace: false,
            links: &links,
        };
        self.threads = ThreadPlacement::of(
            &self.tour,
            &self.files,
            &self.pulled_threads,
            &self.followed_lines,
            &display,
        );
    }

    pub fn toggle_thread_fold(&mut self, thread_id: &ThreadId) {
        if !self.thread_fold_flipped.remove(thread_id) {
            self.thread_fold_flipped.insert(thread_id.clone());
        }
    }

    pub fn pending_review_is(&mut self, exists: bool) {
        self.clear_problem(ProblemKind::NotRead(Read::PendingReview));
        self.comment_desk.pending_review_is(exists);
    }

    pub fn last_review_is(&mut self, last: Option<SubmittedReview>) {
        self.clear_problem(ProblemKind::NotRead(Read::LastReview));
        self.last_review = last;
    }

    /// The viewer's latest sent review, from this app or the GitHub web page.
    pub fn last_review(&self) -> Option<&SubmittedReview> {
        self.last_review.as_ref()
    }

    /// The PR has commits the last review did not see. Not so while the PR's head is unknown.
    pub fn last_review_is_old(&self) -> bool {
        match (&self.last_review, &self.pr_head) {
            (Some(review), Some(pr_head)) => review.commit != *pr_head,
            _ => false,
        }
    }

    /// "Comment now" shows only when there is surely no pending review.
    fn tour_freshness(&self) -> TourFreshness {
        if self.old_head().is_some() {
            TourFreshness::Behind
        } else {
            TourFreshness::Current
        }
    }

    pub fn can_comment_now(&self) -> bool {
        self.comment_desk.can_comment_now()
    }

    pub fn thread_action_request(&self, action: ThreadAction) -> ThreadActionRequest {
        let reply_body = match &action {
            ThreadAction::Reply { thread_id, .. } => {
                self.state.reply_draft(thread_id).map(str::to_owned)
            }
            _ => None,
        };
        self.comment_desk
            .thread_action_request(&self.pr, action, reply_body, self.tour_freshness())
    }

    /// The text in the box as typed so far; `None` while it is not open.
    pub fn text_of(&self, text_box: &TextBox) -> Option<&str> {
        self.comment_desk.text_of(&self.state, text_box)
    }

    /// Opens a reply box, or the edit of one of the viewer's own comments on its current text;
    /// nothing opens for a comment someone else wrote.
    pub fn open_box(&mut self, text_box: &TextBox) -> anyhow::Result<()> {
        self.comment_desk
            .open(&mut self.state, &self.pulled_threads, text_box)
    }

    /// For every key: changes the text in memory only. `typing_save` takes it to disk, off the UI thread.
    pub fn type_in(&mut self, text_box: &TextBox, text: &str) -> anyhow::Result<()> {
        self.comment_desk.type_in(&mut self.state, text_box, text)
    }

    /// The reviewer leaves the box (Escape): a box with nothing written goes, a written one stays.
    pub fn leave(&mut self, text_box: &TextBox) -> anyhow::Result<()> {
        self.comment_desk.leave(&mut self.state, text_box)
    }

    pub fn discard(&mut self, text_box: &TextBox) -> anyhow::Result<()> {
        self.comment_desk.discard(&mut self.state, text_box)
    }

    pub fn apply_thread_action(&mut self, outcome: ThreadActionOutcome) -> ThreadActionResult {
        let step = match &self.screen {
            Screen::Step(id) => Some(id.clone()),
            _ => None,
        };
        self.comment_desk
            .apply_thread_action(&mut self.state, &outcome, step);
        self.show_pulled_threads(outcome.threads.as_ref());
        if outcome.result == ThreadActionResult::Done {
            match &outcome.action {
                ThreadAction::Resolve { thread_id } => self.fold_thread(thread_id, true),
                ThreadAction::Unresolve { thread_id } => self.fold_thread(thread_id, false),
                _ => {}
            }
        }
        outcome.result
    }

    fn fold_thread(&mut self, thread_id: &ThreadId, folded: bool) {
        let Some(thread) = self
            .pulled_threads
            .iter()
            .find(|thread| &thread.id == thread_id)
        else {
            return;
        };
        if thread.is_folded_by_default() == folded {
            self.thread_fold_flipped.remove(thread_id);
        } else {
            self.thread_fold_flipped.insert(thread_id.clone());
        }
    }

    fn show_pulled_threads(&mut self, threads: Option<&Result<Vec<ReviewThread>, String>>) {
        match threads {
            Some(Ok(threads)) => self.set_threads(threads),
            Some(Err(reason)) => self.raise_problem(Problem::ThreadsNotRead(reason.clone())),
            None => {}
        }
    }

    pub fn viewer_authorship_is(&mut self, authored: bool) {
        self.clear_problem(ProblemKind::NotRead(Read::Authorship));
        self.comment_desk.viewer_authorship_is(authored);
    }

    /// GitHub refuses both on the viewer's own PR, so the buttons show only when the PR is surely someone else's.
    pub fn can_approve_or_request_changes(&self) -> bool {
        self.comment_desk.can_approve_or_request_changes()
    }

    /// On the viewer's own PR there is no verdict to give, so finishing only makes sense with comments to send.
    pub fn can_finish_review(&self) -> bool {
        self.comment_desk.can_finish_review()
    }

    /// What the reviewer has not looked at: steps with no read tick and the pieces of the safety net.
    pub fn approve_warning(&self) -> Option<ApproveWarning> {
        let progress = self.progress();
        let unread_safety_net_pieces = coverage::safety_net(&self.tour, &self.files)
            .iter()
            .map(|part| part.piece.id.to_string())
            .collect::<BTreeSet<_>>()
            .len();
        let warning = ApproveWarning {
            unread_steps: progress.total - progress.read,
            unread_safety_net_pieces,
        };
        (warning.unread_steps + warning.unread_safety_net_pieces > 0).then_some(warning)
    }

    pub fn finish_review_request(
        &self,
        verdict: ReviewVerdict,
        body: &str,
    ) -> Result<FinishRequest, FinishRefusal> {
        self.comment_desk
            .finish_review_request(&self.pr, verdict, body)
    }

    pub fn apply_finish(&mut self, outcome: FinishOutcome) -> FinishResult {
        self.comment_desk.apply_finish(&mut self.state, outcome)
    }

    pub fn begin_selection(&mut self, row: LineLocation) {
        self.comment_desk.begin_selection(row);
    }

    pub fn extend_selection(&mut self, row: LineLocation) {
        self.comment_desk.extend_selection(row);
    }

    pub fn is_selecting(&self) -> bool {
        self.comment_desk.row_selection().is_some()
    }

    /// The drag in progress, for a view that knows its rows and draws them without building the page.
    pub fn row_selection(&self) -> Option<&RowSelection> {
        self.comment_desk.row_selection()
    }

    /// The rows being dragged over, for drawing them.
    pub fn selected_lines(&self) -> Option<SelectedLines> {
        self.comment_desk
            .row_selection()?
            .resolve(&self.screen_lines())
    }

    pub fn cancel_selection(&mut self) {
        self.comment_desk.cancel_selection();
    }

    /// The drag ends: when the rows can take a comment, a comment box opens on them.
    pub fn finish_selection(&mut self) -> Result<Option<CommentPlace>, CommentRefusal> {
        let screen_lines = self.screen_lines();
        let freshness = self.tour_freshness();
        self.comment_desk
            .finish_selection(&mut self.state, &screen_lines, freshness, &self.head)
    }

    /// The comment box of a picture opens on the whole file, under the same rules as on lines.
    pub fn begin_file_comment(&mut self, path: &FilePath) -> Result<CommentPlace, CommentRefusal> {
        let freshness = self.tour_freshness();
        CommentDesk::begin_file_comment(&mut self.state, path, freshness, &self.head)
    }

    /// Every row the current screen shows, top to bottom.
    fn screen_lines(&self) -> Vec<DiffLine> {
        in_shown_order(self.screen_pieces())
    }

    fn screen_pieces(&self) -> Vec<PieceDiff> {
        match &self.screen {
            Screen::Step(_) => self
                .current_step()
                .map(|page| page.pieces)
                .unwrap_or_default(),
            Screen::SafetyNet => self
                .safety_net()
                .map(|page| page.pieces)
                .unwrap_or_default(),
            Screen::Summary => Vec::new(),
        }
    }

    pub fn comment_drafts(&self) -> &[CommentDraft] {
        self.state.comment_drafts()
    }

    pub fn queued_comments(&self) -> &[QueuedComment] {
        self.state.comment_queue()
    }

    /// The save of what was typed since the last save, to run in the background; `None` when nothing is unsaved.
    pub fn typing_save(&mut self) -> Option<TypingSave> {
        self.state.typing_save()
    }

    /// "Suggest change" shows only when the range starts and ends on lines of the head.
    pub fn can_suggest_change(&self, place: &CommentPlace) -> bool {
        CommentDesk::can_suggest_change(place)
    }

    /// Adds a suggestion block of the commented lines to the draft, after what is typed already.
    /// `None` when the lines cannot be suggested on, are not on the screen, or there is no draft.
    pub fn suggest_change(&mut self, place: &CommentPlace) -> anyhow::Result<Option<String>> {
        let Some(rows) = self.rows_of(place) else {
            return Ok(None);
        };
        CommentDesk::suggest_change(&mut self.state, place, &rows)
    }

    fn rows_of(&self, place: &CommentPlace) -> Option<Vec<DiffLine>> {
        let CommentPlace::Lines { start, end } = place else {
            return None;
        };
        let mut selection = RowSelection::start_at(start.clone());
        selection.extend_to(end.clone());
        let rows = selection.resolve(&self.screen_lines())?;
        Some(rows.lines().to_vec())
    }

    /// `None` when there is no such draft.
    pub fn comment_request(&self, place: &CommentPlace, mode: SendMode) -> Option<CommentRequest> {
        self.comment_request_for(place, mode, Audience::Author)
    }

    /// `None` when there is no such draft.
    pub fn comment_request_for(
        &self,
        place: &CommentPlace,
        mode: SendMode,
        audience: Audience,
    ) -> Option<CommentRequest> {
        CommentDesk::comment_request(&self.state, place, mode, audience, &self.head, &self.pr)
    }

    pub fn hand_offs(&self) -> &HandOffs {
        self.state.hand_offs()
    }

    /// What the tour agent, GitHub or the reviewer did to the hand-offs. It is made even while a
    /// re-pin runs, so a hand-off is not delivered twice; only its save is refused then.
    pub fn record_hand_offs<T>(
        &mut self,
        change: impl FnOnce(&mut HandOffs) -> T,
    ) -> anyhow::Result<T> {
        self.state.record(|state| change(state.hand_offs_mut()))
    }

    /// The oldest comment in the queue, to send again.
    pub fn queued_comment_request(&self) -> Option<CommentRequest> {
        CommentDesk::queued_comment_request(&self.state, &self.head, &self.pr)
    }

    /// The window was asked for another PR and could not open it; named until another switch succeeds.
    pub fn switch_failed(&mut self, reason: String) {
        self.raise_problem(Problem::SwitchFailed(reason));
    }

    pub fn state_file(&self) -> ReviewStateFile {
        self.state.file().clone()
    }

    pub fn apply_comment(&mut self, outcome: CommentOutcome) -> anyhow::Result<CommentResult> {
        let threads = outcome.threads.clone();
        let step = match &self.screen {
            Screen::Step(id) => Some(id.clone()),
            _ => None,
        };
        let hand_offs_before = self.state.hand_offs().all().len();
        let comment_desk = &mut self.comment_desk;
        let result = self
            .state
            .record(|state| comment_desk.apply_comment(state, outcome, step));
        self.note_new_hand_off(hand_offs_before);
        self.show_pulled_threads(threads.as_ref());
        result
    }

    fn note_new_hand_off(&mut self, hand_offs_before: usize) {
        let Some(newest) = self
            .state
            .hand_offs()
            .all()
            .get(hand_offs_before..)
            .and_then(<[_]>::last)
        else {
            return;
        };
        match (newest.thread(), newest.state()) {
            (None, HandOffState::Failed(reason)) => {
                let comment = newest.comment().chars().take(60).collect();
                let reason = reason.clone();
                self.raise_problem(Problem::HandOffNotSent { comment, reason });
            }
            _ => self.clear_problem(ProblemKind::HandOffNotSent),
        }
    }

    /// Opens code links and the links of the tour's text at the commits this review reads.
    pub fn code_opener(&self, ide: Result<Ide, NoIde>) -> CodeOpener {
        CodeOpener::new(ide, self.working_copy, &self.pr, &self.head)
    }

    /// Asked of the clone on open and on each focus, not on each click.
    pub fn working_copy_is(&mut self, working_copy: WorkingCopy) {
        self.working_copy = working_copy;
    }

    fn links(&self) -> CodeLinks<'_> {
        CodeLinks {
            pr: &self.pr,
            base: &self.base,
            head: &self.head,
        }
    }

    /// Ids the tour does not hold are ignored: the sidebar only offers real steps.
    pub fn go_to(&mut self, id: &StepId) {
        self.go_to_screen(Screen::Step(id.clone()));
    }

    /// The cmd+k list for what the reviewer has typed so far.
    pub fn jump_targets(&self, query: &str) -> Vec<JumpTarget> {
        let safety_net_files: Vec<&FilePath> = if self.reading_order().contains(&Screen::SafetyNet)
        {
            coverage::safety_net(&self.tour, &self.files)
                .iter()
                .map(|part| part.piece.id.path())
                .collect()
        } else {
            Vec::new()
        };
        jump::rank(
            jump::all_targets(&self.tour, &self.files, &safety_net_files),
            query,
        )
    }

    /// The cmd+k palette for what the reviewer has typed so far: places by kind, then actions.
    pub fn palette(&self, query: &str) -> Vec<PaletteGroup> {
        jump::palette(self.jump_targets(""), &self.palette_actions(), query)
    }

    /// What the palette can do on the open screen.
    pub fn palette_actions(&self) -> Vec<PaletteAction> {
        let step = match &self.screen {
            Screen::Step(id) => Some(id),
            Screen::Summary | Screen::SafetyNet => None,
        };
        let tick = step.map(|id| {
            if self.is_ticked(id) {
                PaletteAction::MarkStepUnread
            } else {
                PaletteAction::MarkStepRead
            }
        });
        let wrap = if self.display_options.wraps_lines() {
            PaletteAction::StopWrappingLines
        } else {
            PaletteAction::WrapLines
        };
        let whitespace = if self.display_options.ignores_whitespace() {
            PaletteAction::ShowWhitespace
        } else {
            PaletteAction::HideWhitespace
        };
        let next_unread = self
            .first_unread_step()
            .map(|_| PaletteAction::NextUnreadStep);
        let ask = step.map(|_| PaletteAction::AskAboutStep);
        let finish = self
            .can_finish_review()
            .then_some(PaletteAction::FinishReview);
        next_unread
            .into_iter()
            .chain(tick)
            .chain(ask)
            .chain([wrap, whitespace])
            .chain(finish)
            .chain([
                PaletteAction::OpenOnGitHub,
                PaletteAction::SwitchPr,
                PaletteAction::Settings,
                PaletteAction::Shortcuts,
                PaletteAction::RestartApp,
            ])
            .collect()
    }

    /// A folded step opens for the session so the file or symbol the reviewer asked for is on screen.
    pub fn jump_to(&mut self, target: &JumpTarget) {
        self.go_to_screen(target.screen.clone());
        if let Screen::Step(id) = &target.screen
            && self.is_folded(id)
        {
            self.toggle_fold(id);
        }
    }

    pub fn go_to_summary(&mut self) {
        self.screen = Screen::Summary;
    }

    pub fn go_to_safety_net(&mut self) {
        self.go_to_screen(Screen::SafetyNet);
    }

    fn go_to_screen(&mut self, screen: Screen) {
        if self.reading_order().contains(&screen) {
            self.screen = screen;
        }
    }

    /// False when there is no screen after this one.
    pub fn go_forward(&mut self) -> bool {
        self.step_by(1)
    }

    /// The Next button: on to the next screen that is not a step already read, which the sidebar
    /// still reaches. False when there is none.
    pub fn go_on(&mut self) -> bool {
        match self.onward_screen() {
            Some(screen) => {
                self.screen = screen;
                true
            }
            None => false,
        }
    }

    fn onward_screen(&self) -> Option<Screen> {
        let reading_order = self.reading_order();
        let here = reading_order
            .iter()
            .position(|screen| screen == &self.screen)?;
        reading_order
            .into_iter()
            .skip(here + 1)
            .find(|screen| !matches!(screen, Screen::Step(id) if self.is_ticked(id)))
    }

    /// False on the summary, the first screen.
    pub fn go_back(&mut self) -> bool {
        self.step_by(-1)
    }

    /// The first step after the open screen that is not read yet, starting over from the top.
    /// False when every step is read.
    pub fn go_to_next_unread(&mut self) -> bool {
        let reading_order = self.reading_order();
        let here = reading_order
            .iter()
            .position(|screen| screen == &self.screen)
            .unwrap_or(0);
        let after = reading_order.iter().skip(here + 1);
        let next = after
            .chain(reading_order.iter().take(here + 1))
            .find(|screen| matches!(screen, Screen::Step(id) if !self.is_ticked(id)))
            .cloned();
        match next {
            Some(screen) if screen != self.screen => {
                self.screen = screen;
                true
            }
            _ => false,
        }
    }

    pub fn tour_lead(&self) -> TourLead {
        let progress = self.progress();
        if progress.total == 0 {
            return TourLead::Nowhere;
        }
        if progress.is_complete() {
            return TourLead::Finish {
                steps: progress.total,
                can_finish: self.can_finish_review(),
            };
        }
        match self.first_unread_step() {
            Some(first) if progress.read == 0 => TourLead::Start(first),
            Some(next) => TourLead::Continue(next),
            None => TourLead::Nowhere,
        }
    }

    pub fn route(&self) -> Vec<RouteStop> {
        fn count(steps: &[Step]) -> usize {
            steps.iter().map(|step| 1 + count(&step.children)).sum()
        }
        self.tour
            .steps
            .iter()
            .map(|step| RouteStop {
                id: step.id.clone(),
                title: step.title.clone(),
                why: step.why.clone(),
                details: count(&step.children),
                is_read: self.is_ticked(&step.id),
                has_check: step.check.is_some(),
            })
            .collect()
    }

    pub fn summary_stats(&self) -> SummaryStats {
        SummaryStats {
            steps: self.progress().total,
            files: self.files.len(),
            added_lines: self.files.iter().map(|file| file.added).sum(),
            removed_lines: self.files.iter().map(|file| file.removed).sum(),
            open_threads: self
                .pulled_threads
                .iter()
                .filter(|thread| !thread.is_resolved)
                .count(),
        }
    }

    /// The first step in reading order that is not read yet.
    pub fn first_unread_step(&self) -> Option<UnreadStep> {
        self.steps_in_reading_order.iter().find_map(|screen| {
            let Screen::Step(id) = screen else {
                return None;
            };
            (!self.is_ticked(id)).then(|| UnreadStep {
                id: id.clone(),
                title: self
                    .step(id)
                    .map(|step| step.title.clone())
                    .unwrap_or_default(),
            })
        })
    }

    pub fn is_ticked(&self, id: &StepId) -> bool {
        self.state.read_tick(id).is_some()
    }

    /// The re-pin writes the state file from another thread and this review is replaced when it
    /// is done, so from now on nothing it holds may be saved over the re-pin's record.
    pub fn start_repin(&mut self) {
        self.state.start_repin();
    }

    pub fn refresh_failed(&mut self, error: &anyhow::Error) {
        self.raise_problem(Problem::TourNotRefreshed(format!("{error:#}")));
    }

    pub fn repin_failed(&mut self) {
        self.state.repin_failed();
    }

    /// The tick is kept in memory even when saving fails, so the reviewer loses nothing this session.
    pub fn tick(&mut self, id: &StepId) -> anyhow::Result<()> {
        if !self.reading_order().contains(&Screen::Step(id.clone())) {
            return Ok(());
        }
        let coverage = self.coverage();
        self.state.change(|state| {
            state.tick(id, &self.head);
            self.fold_flipped.remove(id);
            self.viewed_sync.wish_after_tick(state, &coverage, id);
        })
    }

    pub fn untick(&mut self, id: &StepId) -> anyhow::Result<()> {
        let coverage = self.coverage();
        self.state.change(|state| {
            state.untick(id);
            self.fold_flipped.remove(id);
            self.viewed_sync.wish_after_untick(state, &coverage, id);
        })
    }

    /// Skim, ticked and gone steps fold; the reviewer can flip that for the session without losing the tick.
    pub fn is_folded(&self, id: &StepId) -> bool {
        let folds_by_default = self.is_ticked(id)
            || self
                .step(id)
                .is_some_and(|step| step.skim || self.is_gone(step));
        folds_by_default != self.fold_flipped.contains(id)
    }

    /// Opens or folds every file box of the step at once.
    pub fn toggle_fold(&mut self, id: &StepId) {
        if !self.fold_flipped.remove(id) {
            self.fold_flipped.insert(id.clone());
        }
        self.box_fold_flipped.retain(|(step, _)| step != id);
    }

    /// A folded step still lists its file boxes, each folded on its own.
    pub fn is_box_folded(&self, step: &StepId, path: &FilePath) -> bool {
        self.is_folded(step)
            != self
                .box_fold_flipped
                .contains(&(step.clone(), path.clone()))
    }

    pub fn toggle_box_fold(&mut self, step: &StepId, path: &FilePath) {
        let key = (step.clone(), path.clone());
        if !self.box_fold_flipped.remove(&key) {
            self.box_fold_flipped.insert(key);
        }
    }

    /// Only steps of this tour count: ticks of steps a refresh removed stay in the file but not here.
    pub fn progress(&self) -> ReadProgress {
        let reading_order = self.reading_order();
        let steps = reading_order.iter().filter_map(|screen| match screen {
            Screen::Step(id) => Some(id),
            Screen::Summary | Screen::SafetyNet => None,
        });
        ReadProgress {
            read: steps.clone().filter(|id| self.is_ticked(id)).count(),
            total: steps.count(),
        }
    }

    /// The banner text while viewed sync is off; ticks keep working without it.
    pub fn sync_banner(&self) -> Option<&str> {
        self.viewed_sync.banner()
    }

    /// Drives the "not synced" dot.
    pub fn not_synced(&self) -> bool {
        self.viewed_sync.not_synced(&self.state)
    }

    pub fn sync_request(&self) -> SyncRequest {
        self.viewed_sync
            .request(&self.state, &self.coverage(), &self.pr, &self.head)
    }

    /// An error means something stays queued or unread; the next sync tries again.
    /// An answer for another head, e.g. from before a re-pin, is dropped: the next sync asks again.
    pub fn apply_sync(&mut self, outcome: SyncOutcome) -> anyhow::Result<()> {
        if self.viewed_sync.is_off() || !outcome.is_for(&self.head) {
            return Ok(());
        }
        let coverage = self.coverage();
        let mut error = None;
        let saved = self.state.record(|state| {
            let (unticked, sync_error) = self
                .viewed_sync
                .take_in(state, &coverage, &self.head, outcome);
            for step in &unticked {
                self.fold_flipped.remove(step);
            }
            error = sync_error;
        });
        self.clear_problem(ProblemKind::ViewedNotSynced);
        match error {
            Some(error) => {
                let settled = self.viewed_sync.settle(error);
                if let Err(error) = &settled {
                    self.raise_problem(Problem::ViewedNotSynced(format!("{error:#}")));
                }
                settled
            }
            None => saved,
        }
    }

    fn coverage(&self) -> Coverage {
        Coverage::of(&self.tour, &self.files)
    }

    /// Changed code no page can show, by piece id: the reviewer has to read it on GitHub.
    pub fn changes_not_shown(&self) -> Vec<String> {
        let unplaced = self
            .files
            .iter()
            .flat_map(|file| file.not_shown.iter().cloned());
        let outside_their_file = self.files.iter().flat_map(|file| {
            file.pieces
                .iter()
                .filter(|piece| {
                    !self
                        .files
                        .iter()
                        .any(|other| other.path == *piece.id.path())
                })
                .map(|piece| piece.id.to_string())
        });
        unplaced.chain(outside_their_file).collect()
    }

    /// What went wrong that the reviewer must know about, until it is put right.
    pub fn problems(&self) -> Vec<Problem> {
        let not_saved = self.state.not_saved().map(Problem::NotSaved);
        let not_kept = self
            .state
            .refused_while_repinning()
            .then_some(Problem::ChangeNotKeptWhileRepinning);
        not_saved
            .into_iter()
            .chain(not_kept)
            .chain(self.problems.list().cloned())
            .collect()
    }

    pub fn reading_place(&self) -> ReadingPlace {
        let reading_order = self.reading_order();
        let position = reading_order
            .iter()
            .position(|screen| screen == &self.screen);
        let is_step = |screen: &&Screen| matches!(screen, Screen::Step(_));
        let next = self.onward_screen().and_then(|screen| match &screen {
            Screen::Step(id) => Some(NextScreen::Step {
                title: self.step(id)?.title.clone(),
            }),
            Screen::SafetyNet => Some(NextScreen::SafetyNet),
            Screen::Summary => None,
        });
        ReadingPlace {
            step_number: match (&self.screen, position) {
                (Screen::Step(_), Some(position)) => Some(
                    reading_order
                        .iter()
                        .take(position + 1)
                        .filter(is_step)
                        .count(),
                ),
                _ => None,
            },
            step_count: reading_order.iter().filter(is_step).count(),
            has_previous: position.is_some_and(|position| position > 0),
            next,
        }
    }

    fn step_by(&mut self, offset: isize) -> bool {
        let reading_order = self.reading_order();
        let next = reading_order
            .iter()
            .position(|screen| screen == &self.screen)
            .and_then(|position| position.checked_add_signed(offset))
            .and_then(|position| reading_order.get(position));
        match next {
            Some(screen) => {
                self.screen = screen.clone();
                true
            }
            None => false,
        }
    }
}

/// The head a step was last read at: its tick, or the tick a re-pin took away.
fn read_head_of<'a>(state: &'a ReviewState, step: &StepId) -> Option<&'a Sha> {
    state
        .read_tick(step)
        .or_else(|| state.read_before(step))
        .map(|tick| &tick.head)
}

/// Rows as a screen shows them: the pieces of one file in one block, in line order.
fn in_shown_order(pieces: Vec<PieceDiff>) -> Vec<DiffLine> {
    FileBlock::group(pieces)
        .iter()
        .flat_map(|block| block.lines().cloned())
        .collect()
}
