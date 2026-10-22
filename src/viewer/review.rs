use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::code_source::CodeSource;
use crate::coverage;
use crate::pieces::{self, ChangedFile};
use crate::pull_request::{PullRequestUrl, Sha};
use crate::tour::{Step, Tour};

use std::path::PathBuf;

use crate::config::AskSettings;

use super::ask::Place;
use super::ask_context;
use super::comments::{
    CommentDraft, CommentOutcome, CommentPlace, CommentRefusal, CommentRequest, CommentResult,
    Origin, Outcome, QueuedComment, SendMode,
};
use super::diff_display::{DiffDisplay, FilesAtHead, FilesAtHeadRequest, MoreContext};
use super::github_link::GitHubLinks;
use super::highlight::{HighlightRequest, Highlights};
use super::ports::{AgentLaunch, GitHubPort, ViewedState, ViewedSyncUnavailable};
use super::review_state::{RepinRecord, ReviewState, ReviewStateFile};
use super::row_selection::{RowSelection, SelectedLines};
use super::step_page::{DiffLine, LineLocation, PieceDiff, SafetyNetPage, SinceRead, StepPage};
use super::threads::{self, ReviewThread, ThreadPlacement};
use crate::repin::RepinStatus;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct StepId(String);

impl From<&str> for StepId {
    fn from(id: &str) -> Self {
        Self(id.to_string())
    }
}

impl fmt::Display for StepId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReadProgress {
    pub read: usize,
    pub total: usize,
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
    files: Vec<ChangedFile>,
    /// Summary, then parent, its children, the next parent (from the big picture to the details),
    /// then the safety net when it holds anything.
    steps_in_reading_order: Vec<Screen>,
    has_uncovered_changes: bool,
    pulled_threads: Vec<ReviewThread>,
    threads: ThreadPlacement,
    /// Threads fold by default when resolved, outdated or started by a bot; these are the ones the reviewer flipped.
    thread_fold_flipped: BTreeSet<String>,
    screen: Screen,
    state: ReviewState,
    state_file: ReviewStateFile,
    /// Skim and ticked steps start folded; these are the steps whose fold the reviewer flipped.
    /// Not saved: a reopened app shows steps folded again.
    fold_flipped: BTreeSet<StepId>,
    highlights: Highlights,
    /// Why viewed sync is switched off for this session; the app shows it as a banner.
    sync_off_because: Option<String>,
    repin_running: bool,
    files_at_head: FilesAtHead,
    more_context: HashMap<MoreContext, usize>,
    ignores_whitespace: bool,
    /// The diff from each older head some step was read at to the tour's head.
    /// A head that cannot be read (gone after a force push) is left out: those steps show the full diff.
    changes_since: BTreeMap<String, Vec<ChangedFile>>,
    /// Steps read before whose full diff the reviewer asked for. Not saved.
    full_diff_shown: BTreeSet<StepId>,
    selection: Option<RowSelection>,
    /// Unknown until GitHub answers; "Comment now" is only offered when it is known to be `false`.
    has_pending_review: Option<bool>,
}

impl Review {
    /// Code is read at the tour's own base and head, not the PR's current ones.
    pub fn open(
        tour: Tour,
        source: &dyn CodeSource,
        state_file: ReviewStateFile,
    ) -> anyhow::Result<Self> {
        let state = state_file.load()?;
        let pr: PullRequestUrl = tour.pr.parse()?;
        let (base, head) = (Sha::new(&tour.base), Sha::new(&tour.head));
        let files = pieces::cut(source, &base, &head)?;
        let steps = tour
            .steps()
            .into_iter()
            .map(|step| Screen::Step(StepId::from(step.id.as_str())));
        let steps_in_reading_order = std::iter::once(Screen::Summary).chain(steps).collect();
        let has_uncovered_changes = threads::uncovered(&tour, &files);
        let read_heads: BTreeSet<&str> = tour
            .steps()
            .iter()
            .filter_map(|step| read_head_of(&state, &StepId::from(step.id.as_str())))
            .filter(|read_head| *read_head != tour.head)
            .collect();
        let changes_since = read_heads
            .into_iter()
            .filter_map(|read_head| {
                let files = pieces::cut(source, &Sha::new(read_head), &head).ok()?;
                Some((read_head.to_string(), files))
            })
            .collect();
        Ok(Self {
            tour,
            pr,
            base,
            head,
            pr_head: None,
            files,
            steps_in_reading_order,
            has_uncovered_changes,
            pulled_threads: Vec::new(),
            threads: ThreadPlacement::default(),
            thread_fold_flipped: BTreeSet::new(),
            screen: Screen::Summary,
            state,
            state_file,
            fold_flipped: BTreeSet::new(),
            highlights: Highlights::default(),
            sync_off_because: None,
            repin_running: false,
            files_at_head: FilesAtHead::default(),
            more_context: HashMap::new(),
            ignores_whitespace: false,
            changes_since,
            full_diff_shown: BTreeSet::new(),
            selection: None,
            has_pending_review: None,
        })
    }

    /// Colouring reads whole files, so the app runs this off the main thread and hands the result to `set_highlights`.
    pub fn highlight_request(&self) -> HighlightRequest {
        HighlightRequest::for_files(
            &self.files,
            &Sha::new(&self.tour.base),
            &Sha::new(&self.tour.head),
        )
    }

    pub fn set_highlights(&mut self, highlights: Highlights) {
        self.highlights = highlights;
    }

    /// Context lines come from the files at head, read off the main thread like the colours.
    pub fn files_at_head_request(&self) -> FilesAtHeadRequest {
        FilesAtHeadRequest::for_files(&self.files, &Sha::new(&self.tour.head))
    }

    pub fn set_files_at_head(&mut self, files_at_head: FilesAtHead) {
        self.files_at_head = files_at_head;
        self.place_threads();
    }

    pub fn show_more_context(&mut self, more: &MoreContext) {
        DiffDisplay::more_lines(&mut self.more_context, more);
    }

    pub fn ignores_whitespace(&self) -> bool {
        self.ignores_whitespace
    }

    pub fn toggle_ignore_whitespace(&mut self) {
        self.ignores_whitespace = !self.ignores_whitespace;
    }

    pub fn screen(&self) -> &Screen {
        &self.screen
    }

    pub fn pr(&self) -> &PullRequestUrl {
        &self.pr
    }

    /// Blocking; the app runs it in the background on open and on focus.
    pub fn check_pr_head(&mut self, github: &dyn GitHubPort) -> anyhow::Result<()> {
        let tips = github.pull_request_tips(&self.pr)?;
        self.pr_head_is(tips.head);
        Ok(())
    }

    pub fn pr_head_is(&mut self, head: Sha) {
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

    pub fn current_step(&self) -> Option<StepPage<'_>> {
        let Screen::Step(id) = &self.screen else {
            return None;
        };
        let step = self.step(id)?;
        let links = self.links();
        let display = self.display(&links);
        let mut page = StepPage::of(step, &self.tour, &self.files, &display, self.is_folded(id));
        page.is_ticked = self.is_ticked(id);
        let shown: Vec<_> = step
            .pieces
            .iter()
            .flat_map(|pointer| pointer.shown_parts(&self.tour, &self.files))
            .collect();
        page.threads = threads::views(
            self.threads.in_step(id),
            &page.pieces,
            &self.thread_fold_flipped,
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
                let links_from_read_head = GitHubLinks {
                    pr: &self.pr,
                    base: &read_head,
                    head: &self.head,
                };
                let since_display = self.display(&links_from_read_head);
                page.pieces = shown
                    .into_iter()
                    .map(|part| PieceDiff::since_read(part, since_files, &since_display))
                    .collect();
                self.mark_repin_status(&mut page.pieces);
            }
        }
        Some(page)
    }

    fn changes_since_read(&self, id: &StepId) -> Option<(Sha, &[ChangedFile])> {
        let read_head = read_head_of(&self.state, id)?;
        let since_files = self.changes_since.get(read_head)?;
        Some((Sha::new(read_head), since_files))
    }

    /// Flips a step read before between the change since reading and the full diff, for the session.
    pub fn toggle_full_diff(&mut self, id: &StepId) {
        if !self.full_diff_shown.remove(id) {
            self.full_diff_shown.insert(id.clone());
        }
    }

    fn step(&self, id: &StepId) -> Option<&Step> {
        self.tour.steps().into_iter().find(|step| step.id == id.0)
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
                piece.repin_status = record.status_of(&piece.id.to_string());
            }
        }
    }

    fn is_gone(&self, step: &Step) -> bool {
        let id = StepId::from(step.id.as_str());
        step.diagram.is_none()
            && self
                .repin_record()
                .is_some_and(|record| record.gone_count(&id) > 0)
            && step
                .pieces
                .iter()
                .all(|pointer| pointer.shown_parts(&self.tour, &self.files).is_empty())
    }

    pub fn head(&self) -> &str {
        &self.tour.head
    }

    /// What the AI is told about a place: pointers, the shown diff lines, step title and why.
    pub fn ask_context(&self, place: &Place) -> Option<String> {
        ask_context::context_for(&self.tour, &self.files, &self.links(), place)
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
                let id = StepId::from(step.id.as_str());
                into.push(SidebarEntry {
                    is_current: current == Some(&id),
                    is_ticked: review.is_ticked(&id),
                    thread_count: review.threads.in_step(&id).len(),
                    is_gone: review.is_gone(step),
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
            );
            page
        })
    }

    fn display<'a>(&'a self, links: &'a GitHubLinks<'a>) -> DiffDisplay<'a> {
        DiffDisplay {
            highlights: &self.highlights,
            files_at_head: &self.files_at_head,
            more_context: &self.more_context,
            ignores_whitespace: self.ignores_whitespace,
            links,
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

    /// Blocking; the app runs it in the background on open and on focus.
    pub fn pull_threads(&mut self, github: &dyn GitHubPort) -> anyhow::Result<()> {
        let threads = github.review_threads(&self.pr)?;
        self.set_threads(&threads);
        Ok(())
    }

    pub fn set_threads(&mut self, threads: &[ReviewThread]) {
        self.pulled_threads = threads.to_vec();
        self.place_threads();
    }

    /// Placed by the rows a step shows, which depend on the files read at head, so again when those arrive.
    fn place_threads(&mut self) {
        let links = self.links();
        let display = DiffDisplay {
            highlights: &Highlights::default(),
            files_at_head: &self.files_at_head,
            more_context: &HashMap::new(),
            ignores_whitespace: false,
            links: &links,
        };
        self.threads = ThreadPlacement::of(&self.tour, &self.files, &self.pulled_threads, &display);
    }

    pub fn toggle_thread_fold(&mut self, thread_id: &str) {
        if !self.thread_fold_flipped.remove(thread_id) {
            self.thread_fold_flipped.insert(thread_id.to_string());
        }
    }

    /// Blocking; the app runs it in the background on open and on focus.
    pub fn pull_pending_review(&mut self, github: &dyn GitHubPort) -> anyhow::Result<()> {
        self.has_pending_review = Some(github.has_pending_review(&self.pr)?);
        Ok(())
    }

    pub fn pending_review_is(&mut self, exists: bool) {
        self.has_pending_review = Some(exists);
    }

    /// "Comment now" shows only when there is surely no pending review.
    pub fn can_comment_now(&self) -> bool {
        self.has_pending_review == Some(false)
    }

    pub fn begin_selection(&mut self, row: LineLocation) {
        self.selection = Some(RowSelection::start_at(row));
    }

    pub fn extend_selection(&mut self, row: LineLocation) {
        if let Some(selection) = &mut self.selection {
            selection.extend_to(row);
        }
    }

    pub fn is_selecting(&self) -> bool {
        self.selection.is_some()
    }

    /// The rows being dragged over, for drawing them.
    pub fn selected_lines(&self) -> Option<SelectedLines> {
        self.selection.as_ref()?.resolve(&self.screen_lines())
    }

    pub fn cancel_selection(&mut self) {
        self.selection = None;
    }

    /// The drag ends: when the rows can take a comment, a comment box opens on them.
    pub fn finish_selection(&mut self) -> Result<Option<CommentPlace>, CommentRefusal> {
        let Some(selected) = self.selected_lines() else {
            self.selection = None;
            return Ok(None);
        };
        self.selection = None;
        if !selected.take_comment() {
            return Err(CommentRefusal::NotInsideHunk);
        }
        if self.old_head().is_some() {
            return Err(CommentRefusal::TourIsOld);
        }
        let place = CommentPlace::of(&selected);
        self.state.open_draft(place.clone());
        self.save_viewer_sections()
            .map_err(|error| CommentRefusal::NoAccess(format!("{error:#}")))?;
        Ok(Some(place))
    }

    /// Every row the current screen shows, top to bottom.
    fn screen_lines(&self) -> Vec<DiffLine> {
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
        .iter()
        .flat_map(|piece| piece.lines().cloned())
        .collect()
    }

    pub fn comment_drafts(&self) -> &[CommentDraft] {
        self.state.comment_drafts()
    }

    pub fn queued_comments(&self) -> &[QueuedComment] {
        self.state.comment_queue()
    }

    /// Saved at once, so a crash loses nothing.
    pub fn edit_draft(&mut self, place: &CommentPlace, body: &str) -> anyhow::Result<()> {
        self.state.write_draft(place, body);
        self.save_viewer_sections()
    }

    pub fn discard_draft(&mut self, place: &CommentPlace) -> anyhow::Result<()> {
        self.state.drop_draft(place);
        self.state.drop_queued(place);
        self.save_viewer_sections()
    }

    /// `None` when there is no such draft.
    pub fn comment_request(&self, place: &CommentPlace, mode: SendMode) -> Option<CommentRequest> {
        let draft = self.state.draft_at(place)?.clone();
        Some(CommentRequest::new(
            Origin::Draft,
            draft,
            mode,
            Sha::new(&self.tour.head),
        ))
    }

    /// The oldest comment in the queue, to send again.
    pub fn queued_comment_request(&self) -> Option<CommentRequest> {
        let queued = self.state.comment_queue().first()?;
        Some(CommentRequest::new(
            Origin::Queue,
            queued.draft.clone(),
            queued.mode,
            Sha::new(&self.tour.head),
        ))
    }

    /// Runs the whole send here and now; the app runs `CommentRequest::run` in the background
    /// and then `apply_comment` instead.
    pub fn send_comment(
        &mut self,
        place: &CommentPlace,
        mode: SendMode,
        github: &dyn GitHubPort,
    ) -> anyhow::Result<Option<CommentResult>> {
        let Some(request) = self.comment_request(place, mode) else {
            return Ok(None);
        };
        let outcome = request.run(github, &self.pr);
        self.apply_comment(outcome).map(Some)
    }

    /// Sends the queue oldest first and stops at the first comment that does not go.
    pub fn send_queued_comments(&mut self, github: &dyn GitHubPort) -> anyhow::Result<()> {
        while let Some(request) = self.queued_comment_request() {
            let outcome = request.run(github, &self.pr);
            if self.apply_comment(outcome)? != CommentResult::Sent {
                break;
            }
        }
        Ok(())
    }

    pub fn apply_comment(&mut self, outcome: CommentOutcome) -> anyhow::Result<CommentResult> {
        let CommentOutcome { request, how } = outcome;
        let place = request.draft.place.clone();
        let result = match how {
            Outcome::Sent => {
                self.has_pending_review = Some(request.mode == SendMode::AddToReview);
                self.leave(&request.origin, &place);
                CommentResult::Sent
            }
            Outcome::Failed(reason) => {
                self.keep_for_retry(&request, &reason);
                CommentResult::Queued(reason)
            }
            Outcome::Rejected(reason) => {
                self.has_pending_review = Some(true);
                self.keep_as_draft(&request);
                CommentResult::Rejected(reason)
            }
            Outcome::Refused(refusal) => {
                if refusal == CommentRefusal::PendingReviewExists {
                    self.has_pending_review = Some(true);
                    if let Some(queued) = self.state.queued_at(&place) {
                        queued.mode = SendMode::AddToReview;
                    }
                }
                CommentResult::Refused(refusal)
            }
        };
        self.save_viewer_sections()?;
        Ok(result)
    }

    fn leave(&mut self, origin: &Origin, place: &CommentPlace) {
        match origin {
            Origin::Draft => self.state.drop_draft(place),
            Origin::Queue => self.state.drop_queued(place).map(|queued| queued.draft),
        };
    }

    fn keep_for_retry(&mut self, request: &CommentRequest, reason: &str) {
        let place = &request.draft.place;
        match request.origin {
            Origin::Draft => {
                self.state.drop_draft(place);
                self.state.queue_comment(QueuedComment {
                    draft: request.draft.clone(),
                    mode: request.mode,
                    failed_because: reason.to_string(),
                });
            }
            Origin::Queue => {
                if let Some(queued) = self.state.queued_at(place) {
                    queued.failed_because = reason.to_string();
                }
            }
        }
    }

    fn keep_as_draft(&mut self, request: &CommentRequest) {
        let place = &request.draft.place;
        if request.origin == Origin::Queue {
            self.state.drop_queued(place);
            self.state.open_draft(place.clone());
            self.state.write_draft(place, &request.draft.body);
        }
    }

    fn links(&self) -> GitHubLinks<'_> {
        GitHubLinks {
            pr: &self.pr,
            base: &self.base,
            head: &self.head,
        }
    }

    /// Ids the tour does not hold are ignored: the sidebar only offers real steps.
    pub fn go_to(&mut self, id: &StepId) {
        self.go_to_screen(Screen::Step(id.clone()));
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

    /// False on the summary, the first screen.
    pub fn go_back(&mut self) -> bool {
        self.step_by(-1)
    }

    pub fn is_ticked(&self, id: &StepId) -> bool {
        self.state.read_tick(id).is_some()
    }

    /// The tick is kept in memory even when saving fails, so the reviewer loses nothing this session.
    /// The re-pin writes the state file from another thread and this review is replaced when it
    /// is done, so from now on nothing it holds may be saved over the re-pin's record.
    pub fn start_repin(&mut self) {
        self.repin_running = true;
    }

    pub fn repin_failed(&mut self) {
        self.repin_running = false;
    }

    pub fn tick(&mut self, id: &StepId) -> anyhow::Result<()> {
        self.refuse_changes_while_repinning()?;
        if !self.reading_order().contains(&Screen::Step(id.clone())) {
            return Ok(());
        }
        let head = Sha::new(&self.tour.head);
        self.state.tick(id, &head);
        self.fold_flipped.remove(id);
        if self.sync_off_because.is_none() {
            let coverage = self.coverage();
            for file in coverage.files_of(id) {
                if coverage.is_fully_ticked(file, |step| self.is_ticked(step)) {
                    self.state.want_viewed(file, true);
                }
            }
        }
        self.save_viewer_sections()
    }

    pub fn untick(&mut self, id: &StepId) -> anyhow::Result<()> {
        self.refuse_changes_while_repinning()?;
        self.state.untick(id);
        self.fold_flipped.remove(id);
        if self.sync_off_because.is_none() {
            for file in self.coverage().files_of(id) {
                self.state.want_viewed(file, false);
            }
        }
        self.save_viewer_sections()
    }

    /// Skim, ticked and gone steps fold; the reviewer can flip that for the session without losing the tick.
    pub fn is_folded(&self, id: &StepId) -> bool {
        let folds_by_default = self.is_ticked(id)
            || self
                .step(id)
                .is_some_and(|step| step.skim || self.is_gone(step));
        folds_by_default != self.fold_flipped.contains(id)
    }

    pub fn toggle_fold(&mut self, id: &StepId) {
        if !self.fold_flipped.remove(id) {
            self.fold_flipped.insert(id.clone());
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
        self.sync_off_because.as_deref()
    }

    /// Drives the "not synced" dot.
    pub fn not_synced(&self) -> bool {
        self.sync_off_because.is_none() && !self.state.unsent_viewed().is_empty()
    }

    /// Everything the network part of a sync needs, owned, so it can run on a background thread.
    pub fn sync_request(&self) -> SyncRequest {
        let baseline_wishes = self
            .state
            .github_baseline_due()
            .then(|| self.baseline_wishes());
        SyncRequest {
            tour_head: Sha::new(&self.tour.head),
            unsent: self.state.unsent_viewed().clone(),
            baseline_wishes,
            is_off: self.sync_off_because.is_some(),
        }
    }

    /// After a re-pin: viewed for every file whose steps are all still ticked, and the queued
    /// "not viewed" edits for the rest. Queued "viewed" edits are dropped: the ticks say it again
    /// where it is still true.
    fn baseline_wishes(&self) -> BTreeMap<String, bool> {
        let coverage = self.coverage();
        let is_fully_ticked =
            |path: &str| coverage.is_fully_ticked(path, |step| self.is_ticked(step));
        let still_viewed = coverage
            .files_by_step
            .values()
            .flatten()
            .filter(|path| is_fully_ticked(path))
            .map(|path| (path.clone(), true));
        let still_not_viewed = self
            .state
            .unsent_viewed()
            .iter()
            .filter(|(path, viewed)| !**viewed && !is_fully_ticked(path))
            .map(|(path, _)| (path.clone(), false));
        still_viewed.chain(still_not_viewed).collect()
    }

    /// Runs the whole sync here and now; the app runs `SyncRequest::run` in the background
    /// and then `apply_sync` instead.
    pub fn sync(&mut self, github: &dyn GitHubPort, pr: &PullRequestUrl) -> anyhow::Result<()> {
        let outcome = self.sync_request().run(github, pr);
        self.apply_sync(outcome)
    }

    /// An error means something stays queued or unread; the next sync tries again.
    pub fn apply_sync(&mut self, outcome: SyncOutcome) -> anyhow::Result<()> {
        if self.sync_off_because.is_some() {
            return Ok(());
        }
        match (&outcome.baseline_wishes, &outcome.on_github) {
            (Some(wishes), Some(on_github)) => {
                self.state.take_github_baseline(on_github, wishes);
                for (path, viewed) in &outcome.sent {
                    self.state.sent_viewed(path, *viewed);
                }
            }
            (Some(_), None) => {}
            (None, on_github) => {
                for (path, viewed) in &outcome.sent {
                    self.state.sent_viewed(path, *viewed);
                }
                if let Some(on_github) = on_github {
                    self.pull_viewed(on_github);
                }
            }
        }
        let saved = self.save_viewer_sections();
        match outcome.error.map(Err).unwrap_or(Ok(())).and(saved) {
            Err(error) => match error.downcast_ref::<ViewedSyncUnavailable>() {
                Some(unavailable) => {
                    self.sync_off_because = Some(unavailable.to_string());
                    Ok(())
                }
                None => Err(error),
            },
            ok => ok,
        }
    }

    /// Reacts only to files whose state changed since the last sync, and never to files with an
    /// unsent local change: those win.
    fn pull_viewed(&mut self, on_github: &BTreeMap<String, bool>) {
        let unsent = self.state.unsent_viewed().clone();
        let is_viewed = |path: &str| {
            unsent
                .get(path)
                .or_else(|| on_github.get(path))
                .copied()
                .unwrap_or(false)
        };
        let changed: Vec<(&String, bool)> = on_github
            .iter()
            .filter(|(path, now)| {
                !unsent.contains_key(*path) && self.state.was_viewed_on_github(path) != **now
            })
            .map(|(path, now)| (path, *now))
            .collect();

        let coverage = self.coverage();
        let head = Sha::new(&self.tour.head);
        for (path, _) in changed.iter().filter(|(_, now)| !now) {
            for step in coverage.steps_holding(path) {
                self.state.untick(step);
                self.fold_flipped.remove(step);
            }
        }
        for (path, _) in changed.iter().filter(|(_, now)| *now) {
            for step in coverage.steps_holding(path) {
                let files = coverage.files_of(step);
                if !self.is_ticked(step) && files.iter().all(|file| is_viewed(file)) {
                    self.state.tick(step, &head);
                }
            }
        }
        for (path, now) in on_github.iter() {
            if !unsent.contains_key(path) {
                self.state.saw_on_github(path, *now);
            }
        }
    }

    /// A step holds a file when it shows any lines of it; a file is partly unheld when the
    /// safety net still has lines of it.
    fn coverage(&self) -> Coverage {
        let mut coverage = Coverage::default();
        for step in self.tour.steps() {
            let paths = step
                .pieces
                .iter()
                .flat_map(|pointer| pointer.shown_parts(&self.tour, &self.files))
                .map(|part| part.piece.id.path.clone());
            coverage
                .files_by_step
                .entry(StepId::from(step.id.as_str()))
                .or_default()
                .extend(paths);
        }
        coverage.partly_unheld = coverage::safety_net(&self.tour, &self.files)
            .into_iter()
            .map(|part| part.piece.id.path.clone())
            .collect();
        coverage
    }

    /// The viewer owns ticks and viewed sections; asks are saved by the Ask panel, so only
    /// these are written into a freshly loaded file.
    fn save_viewer_sections(&self) -> anyhow::Result<()> {
        self.refuse_changes_while_repinning()?;
        self.state_file
            .update(|saved| saved.take_viewer_sections(&self.state))
    }

    fn refuse_changes_while_repinning(&self) -> anyhow::Result<()> {
        match self.repin_running {
            true => Err(RepinRunning.into()),
            false => Ok(()),
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

#[derive(Debug)]
pub struct RepinRunning;

impl std::fmt::Display for RepinRunning {
    fn fmt(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter
            .write_str("the tour is being updated to the new commits; try again when it is done")
    }
}

impl std::error::Error for RepinRunning {}

/// The head a step was last read at: its tick, or the tick a re-pin took away.
fn read_head_of<'a>(state: &'a ReviewState, step: &StepId) -> Option<&'a str> {
    state
        .read_tick(step)
        .or_else(|| state.read_before(step))
        .map(|tick| tick.head.as_str())
}

/// Which files each step holds, and which files have changes no step holds.
#[derive(Default)]
struct Coverage {
    files_by_step: BTreeMap<StepId, BTreeSet<String>>,
    partly_unheld: BTreeSet<String>,
}

impl Coverage {
    fn files_of(&self, step: &StepId) -> &BTreeSet<String> {
        static NONE: BTreeSet<String> = BTreeSet::new();
        self.files_by_step.get(step).unwrap_or(&NONE)
    }

    fn steps_holding<'a>(&'a self, path: &'a str) -> impl Iterator<Item = &'a StepId> {
        self.files_by_step
            .iter()
            .filter(move |(_, files)| files.contains(path))
            .map(|(step, _)| step)
    }

    /// A file counts as read only when no change of it is left outside the steps.
    fn is_fully_ticked(&self, path: &str, is_ticked: impl Fn(&StepId) -> bool) -> bool {
        !self.partly_unheld.contains(path) && self.steps_holding(path).all(is_ticked)
    }
}

pub struct SyncRequest {
    tour_head: Sha,
    unsent: BTreeMap<String, bool>,
    /// Set for the first sync after a re-pin; it replaces `unsent`.
    baseline_wishes: Option<BTreeMap<String, bool>>,
    is_off: bool,
}

/// What the network part of a sync learned and did; the review takes it in with `apply_sync`.
pub struct SyncOutcome {
    sent: Vec<(String, bool)>,
    on_github: Option<BTreeMap<String, bool>>,
    baseline_wishes: Option<BTreeMap<String, bool>>,
    error: Option<anyhow::Error>,
}

impl SyncOutcome {
    fn nothing() -> Self {
        Self {
            sent: Vec::new(),
            on_github: None,
            baseline_wishes: None,
            error: None,
        }
    }
}

impl SyncRequest {
    /// No sync while sync is off, or while the tour is behind the PR head.
    pub fn run(self, github: &dyn GitHubPort, pr: &PullRequestUrl) -> SyncOutcome {
        if self.is_off {
            return SyncOutcome::nothing();
        }
        match github.pull_request_tips(pr) {
            Ok(tips) if tips.head == self.tour_head => {}
            Ok(_) => return SyncOutcome::nothing(),
            Err(error) => {
                return SyncOutcome {
                    error: Some(error),
                    ..SyncOutcome::nothing()
                };
            }
        }
        match self.baseline_wishes {
            Some(wishes) => Self::take_baseline(github, pr, wishes),
            None => {
                let mut outcome = SyncOutcome::nothing();
                outcome.error = push(github, pr, &self.unsent, &mut outcome.sent).err();
                match read_viewed(github, pr) {
                    Ok(on_github) => outcome.on_github = Some(on_github),
                    Err(error) => outcome.error = outcome.error.or(Some(error)),
                }
                outcome
            }
        }
    }

    /// GitHub is read first, so only wishes it does not already match are pushed.
    fn take_baseline(
        github: &dyn GitHubPort,
        pr: &PullRequestUrl,
        wishes: BTreeMap<String, bool>,
    ) -> SyncOutcome {
        let on_github = match read_viewed(github, pr) {
            Ok(on_github) => on_github,
            Err(error) => {
                return SyncOutcome {
                    error: Some(error),
                    ..SyncOutcome::nothing()
                };
            }
        };
        let differing: BTreeMap<String, bool> = wishes
            .iter()
            .filter(|(path, viewed)| on_github.get(*path).copied().unwrap_or(false) != **viewed)
            .map(|(path, viewed)| (path.clone(), *viewed))
            .collect();
        let mut outcome = SyncOutcome::nothing();
        outcome.error = push(github, pr, &differing, &mut outcome.sent).err();
        outcome.on_github = Some(on_github);
        outcome.baseline_wishes = Some(wishes);
        outcome
    }
}

/// Stops at the first failure; what was not sent stays queued.
fn push(
    github: &dyn GitHubPort,
    pr: &PullRequestUrl,
    wishes: &BTreeMap<String, bool>,
    sent: &mut Vec<(String, bool)>,
) -> anyhow::Result<()> {
    for (path, viewed) in wishes {
        github.set_file_viewed(pr, path, *viewed)?;
        sent.push((path.clone(), *viewed));
    }
    Ok(())
}

fn read_viewed(
    github: &dyn GitHubPort,
    pr: &PullRequestUrl,
) -> anyhow::Result<BTreeMap<String, bool>> {
    Ok(github
        .viewed_files(pr)?
        .into_iter()
        .map(|file| (file.path, file.state == ViewedState::Viewed))
        .collect())
}
