//! The open review on the GPUI side: it owns the `Review`, the rows built from it, and every round trip
//! to GitHub, the code source and the tour tools. Layouts draw from it and forward the reader's input to it.

use crate::file_path::FilePath;
use crate::findings::FindingId;
use crate::viewer::{FixNote, FixReply, FixTiming, WhyReply, catch_up_findings};
use std::collections::{HashMap, HashSet};
use std::ops::{ControlFlow, Range};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::{
    App, Context, EventEmitter, ListAlignment, ListOffset, ListState, Subscription, Task, Window,
    px,
};

use crate::app::components::image_row::ImagePictures;
use crate::app::components::markdown_view::OpenDetails;
use crate::app::components::pr_switcher::PrSwitching;
use crate::app::components::toast_stack::post_toast;
use crate::app::frame_stats::FrameStatsHook;
use crate::app::page_rows::{PageRows, RowsChange};
use crate::app::review_agent;
use crate::app::sidebar_rows::SidebarRows;
use crate::app::updater::release_install_in_the_way;
use crate::app_folders::AppFolders;
use crate::code_source::CodeSource;
use crate::github::{GhCloner, GitHub};
use crate::opened_subject::{OpenedChangeSet, writer_folder};
use crate::viewer::{
    AfterJob, AfterLanding, AgentReplied, AiReviewRefusal, Ask, Audience, CloneOffer, CommentId,
    CommentOutcome, CommentPlace, CommentRequest, CommentResult, FindingRefusal, FinishResult,
    GitHubPort, HandOffButton, HandOffId, ImageCache, JobOutcome, JobProgress, LineLocation,
    NoteSubject, OldHead, OwnAgentHandOff, Problem, RefreshBrief, RepinnedTour, ReplacingJob,
    Review, ReviewChange, ReviewVerdict, Screen, SendMode, StartedJob, TerminalId, TextBox,
    ThreadAction, ThreadActionResult, ToastTone, TourAgentState, TourAgents, TourRefresher,
    TourReloader, TourRepinner, TourSwitcher, WorkPorts, announcements, line_count,
};

const TYPING_SAVE_DEBOUNCE: Duration = Duration::from_millis(500);
/// Rows laid out beyond the visible part, so a fast scroll does not show a blank edge.
const LIST_OVERDRAW: f32 = 300.;
/// The file the Changed on disk scene writes into the staged local review's folder.
#[cfg(feature = "snapshots")]
const CHANGED_ON_DISK_FILE: &str = "later.txt";

/// The two buttons that move a tour on: Re-pin (no AI) and Refresh (the local `claude`).
pub struct TourWork {
    pub repinner: Arc<TourRepinner>,
    pub refresher: Arc<TourRefresher>,
    pub reloader: Arc<TourReloader>,
    pub switcher: Arc<TourSwitcher>,
    pub tour_agents: TourAgents,
}

/// Which re-review the snapshot run draws in the sidebar.
#[cfg(feature = "snapshots")]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum StagedReReview {
    Open,
    MarkedDone,
}

/// How long a quitting app waits for the reviewing mark to come off. A thread still running then
/// dies with the process, so the mark may stay on.
const REVIEWING_MARK_OFF_WAIT: Duration = Duration::from_secs(2);

/// Whether closing waits for the reviewing mark to come off: a quit must, as its threads die with it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum OnClose {
    WaitForIt,
    LeaveItRunning,
}

pub enum SessionEvent {
    /// A re-pin or a Refresh replaced the review: what the layout keeps about the old one is stale.
    Reopened,
    /// A comment did not go, and its box takes the keys again.
    CommentBoxTakesKeysAgain,
}

/// Every change of the review goes through `change`, which rebuilds the rows and the sidebar and tells the
/// screen's list which rows changed. Frames only draw from those. A change notifies the session's observers.
pub struct ReviewSession {
    review: Review,
    rows: PageRows,
    sidebar: SidebarRows,
    tour_agent: TourAgentState,
    /// What the sidebar said before a snapshot scene staged another line.
    #[cfg(feature = "snapshots")]
    tour_agent_before_staging: Option<TourAgentState>,
    /// The guided review a snapshot scene put aside to show the PR in Basic mode.
    #[cfg(feature = "snapshots")]
    guided_before_staging: Option<Review>,
    /// The code source of the PR a snapshot scene put aside to show a local review.
    #[cfg(feature = "snapshots")]
    source_before_staging: Option<Arc<dyn CodeSource>>,
    /// What GitHub said of the PR before a snapshot scene staged another title bar.
    #[cfg(feature = "snapshots")]
    pr_status_before_staging: Option<crate::viewer::PrStatus>,
    /// The status a title bar scene shows, held over a GitHub read that lands while it is on screen.
    #[cfg(feature = "snapshots")]
    staged_pr_status: Option<crate::viewer::PrStatus>,
    /// The issues a title bar scene shows, held over a GitHub read that lands while it is on screen.
    #[cfg(feature = "snapshots")]
    staged_related_issues: Vec<(
        crate::viewer::RelatedIssue,
        Option<crate::viewer::IssueDetails>,
    )>,
    /// The author and branches a title bar scene shows, held over a GitHub read that lands while it is on screen.
    #[cfg(feature = "snapshots")]
    staged_pr_origin: Option<crate::viewer::PullRequestOrigin>,
    /// The Refresh a banner scene shows under way.
    #[cfg(feature = "snapshots")]
    staged_refresh: Option<JobProgress>,
    #[cfg(feature = "snapshots")]
    staged_re_review: Option<StagedReReview>,
    #[cfg(feature = "snapshots")]
    is_unread_end_staged: bool,
    tour_agent_follow: Option<Task<()>>,
    show_agent: Option<Task<()>>,
    /// The review agent's start, or the look for one an earlier app left running.
    ai_review_start: Option<Task<()>>,
    /// Follows the review agent's status for as long as its run lasts.
    ai_review_follow: Option<Task<()>>,
    /// Ask why questions being typed into the review agent; one message at a time.
    questions_delivery: Option<Task<()>>,
    is_delivering_questions: bool,
    /// Wakes the session when the review's earliest work wanted later falls due.
    work_timer: Option<Task<()>>,
    old_head: Option<OldHead>,
    refresh_brief: RefreshBrief,
    screen_list: ListState,
    github: Arc<dyn GitHubPort>,
    source: Arc<dyn CodeSource>,
    tour_work: TourWork,
    image_cache: ImageCache,
    image_pictures: HashMap<FilePath, Rc<ImagePictures>>,
    loading_images: HashSet<FilePath>,
    clone_offer: Option<CloneOffer>,
    typing_save: Option<Task<()>>,
    frame_stats: FrameStatsHook,
    _focus_subscription: Subscription,
    open_details: OpenDetails,
}

/// Typing only changes a box's height when its line count changes.
#[derive(Clone, Copy, PartialEq, Eq)]
enum BoxHeight {
    Same,
    Changed,
}

impl BoxHeight {
    /// The finish body is in the finish dialog, not in the rows.
    fn of(text_box: &TextBox, lines_before: Option<usize>, text: &str) -> Self {
        if *text_box == TextBox::FinishBody || lines_before == Some(line_count(text)) {
            Self::Same
        } else {
            Self::Changed
        }
    }
}

/// What a session works with and reports to, all owned by whoever opens the review.
pub struct SessionWiring {
    pub source: Arc<dyn CodeSource>,
    pub github: Arc<dyn GitHubPort>,
    pub tour_work: TourWork,
    pub frame_stats: FrameStatsHook,
}

impl EventEmitter<SessionEvent> for ReviewSession {}

impl ReviewSession {
    pub fn new(
        mut review: Review,
        wiring: SessionWiring,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        if let Some(in_the_way) = release_install_in_the_way() {
            review.raise_problem(Problem::ReleaseInstallInTheWay(in_the_way));
        }
        let folders = cx.global::<PrSwitching>().folders.clone();
        let not_read = AppFolders::xdg().map_or_default(|xdg| folders.files_not_read(&xdg));
        if !not_read.is_empty() {
            review.raise_problem(Problem::FilesNotRead(not_read));
        }
        let SessionWiring {
            source,
            github,
            tour_work,
            frame_stats,
        } = wiring;
        let focus_subscription = cx.observe_window_activation(window, |this, window, cx| {
            if window.is_window_active() {
                this.review.pull_from_github();
                this.run_due_work(cx);
            }
        });
        let open_details = OpenDetails::default();
        cx.on_app_quit(|this, _| {
            this.save_typing_now();
            this.take_reviewing_mark_off(OnClose::WaitForIt);
            async {}
        })
        .detach();
        cx.on_release(|this, _| this.take_reviewing_mark_off(OnClose::LeaveItRunning))
            .detach();
        let rows = PageRows::of(&review, &open_details);
        let clone_offer = review
            .subject()
            .pull_request()
            .and_then(|pr| CloneOffer::for_source(pr.repo.clone(), source.as_ref(), &folders));
        let mut session = Self {
            sidebar: SidebarRows::of(&review, &TourAgentState::Unknown),
            tour_agent: TourAgentState::Unknown,
            #[cfg(feature = "snapshots")]
            tour_agent_before_staging: None,
            #[cfg(feature = "snapshots")]
            guided_before_staging: None,
            #[cfg(feature = "snapshots")]
            source_before_staging: None,
            #[cfg(feature = "snapshots")]
            pr_status_before_staging: None,
            #[cfg(feature = "snapshots")]
            staged_pr_status: None,
            #[cfg(feature = "snapshots")]
            staged_related_issues: Vec::new(),
            #[cfg(feature = "snapshots")]
            staged_pr_origin: None,
            #[cfg(feature = "snapshots")]
            staged_refresh: None,
            #[cfg(feature = "snapshots")]
            staged_re_review: None,
            #[cfg(feature = "snapshots")]
            is_unread_end_staged: false,
            tour_agent_follow: None,
            show_agent: None,
            ai_review_start: None,
            ai_review_follow: None,
            questions_delivery: None,
            is_delivering_questions: false,
            work_timer: None,
            old_head: review.old_head(),
            refresh_brief: review.refresh_brief(),
            screen_list: ListState::new(rows.len(), ListAlignment::Top, px(LIST_OVERDRAW))
                .measure_all(),
            rows,
            review,
            github,
            source,
            tour_work,
            image_cache: ImageCache::in_folders(&folders),
            image_pictures: HashMap::new(),
            loading_images: HashSet::new(),
            clone_offer,
            typing_save: None,
            frame_stats,
            _focus_subscription: focus_subscription,
            open_details,
        };
        session.start_background_work(cx);
        session
    }

    pub fn is_details_open(&self, key: &str) -> bool {
        self.open_details.is_open(key)
    }

    pub fn toggle_details(&mut self, key: &str, cx: &mut Context<Self>) {
        self.open_details.toggle(key);
        self.rebuild_rows(cx);
    }

    pub fn review(&self) -> &Review {
        &self.review
    }

    pub fn rows(&self) -> &PageRows {
        &self.rows
    }

    pub fn sidebar(&self) -> &SidebarRows {
        &self.sidebar
    }

    pub fn old_head(&self) -> Option<&OldHead> {
        self.old_head.as_ref()
    }

    pub fn refresh_brief(&self) -> &RefreshBrief {
        &self.refresh_brief
    }

    /// The scroll state of the screen's rows; it follows every rebuild of the rows.
    pub fn screen_list(&self) -> &ListState {
        &self.screen_list
    }

    pub fn is_moving(&self) -> bool {
        self.review.jobs().is_running(ReplacingJob::MoveToPr)
    }

    pub fn repin_progress(&self) -> JobProgress {
        self.review.jobs().progress(ReplacingJob::Repin)
    }

    pub fn refresh_progress(&self) -> JobProgress {
        #[cfg(feature = "snapshots")]
        if let Some(staged) = &self.staged_refresh {
            return staged.clone();
        }
        self.review.jobs().progress(ReplacingJob::Refresh)
    }

    pub fn image_pictures(&self, path: &FilePath) -> Option<Rc<ImagePictures>> {
        self.image_pictures.get(path).cloned()
    }

    pub fn change<T>(
        &mut self,
        cx: &mut Context<Self>,
        change: impl FnOnce(&mut Review) -> T,
    ) -> T {
        self.change_session(cx, |session| change(&mut session.review))
    }

    /// For changes of the review together with what the session keeps beside it for the rows.
    /// A change may make work due, so the due work starts after it.
    fn change_session<T>(
        &mut self,
        cx: &mut Context<Self>,
        change: impl FnOnce(&mut Self) -> T,
    ) -> T {
        let result = change(self);
        self.rebuild_rows(cx);
        self.run_due_work(cx);
        result
    }

    /// Starts each piece of work the review says is due on a background thread and lands its
    /// outcome back in the review, which may make more due; then sleeps until the next is due.
    fn run_due_work(&mut self, cx: &mut Context<Self>) {
        for work in self.review.due_work(Instant::now()) {
            let (github, source, agents) = (
                Arc::clone(&self.github),
                Arc::clone(&self.source),
                self.tour_work.tour_agents.clone(),
            );
            self.run_off_thread(
                cx,
                move || {
                    work.run(&WorkPorts {
                        github: github.as_ref(),
                        source: source.as_ref(),
                        agents: &agents,
                    })
                },
                |this, outcome, cx| {
                    let after = this.change(cx, |review| review.land(outcome));
                    #[cfg(feature = "snapshots")]
                    {
                        this.keep_staged_pr_status();
                        this.keep_staged_related_issues();
                        this.keep_staged_pr_origin();
                    }
                    match after {
                        AfterLanding::Nothing => {}
                        AfterLanding::TourAgentIs(state) if state.can_retry() => {
                            this.tour_agent = state;
                            this.show_problems(cx);
                        }
                        // Herdr is back: the follow looks again, tells the sidebar what it finds and keeps it up.
                        AfterLanding::FollowNewTourAgent | AfterLanding::TourAgentIs(_) => {
                            this.follow_tour_agent(cx);
                        }
                    }
                },
            )
            .detach();
        }
        self.work_timer = self.review.next_due().map(|due| {
            cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .timer(due.saturating_duration_since(Instant::now()))
                    .await;
                this.update(cx, Self::run_due_work).ok();
            })
        });
    }

    /// For changes of the Asks, which come on every line an answer streams in: the rows are built
    /// again only when the page changed, that is when the lines with Asks on the screen did.
    pub fn change_asks<T>(
        &mut self,
        cx: &mut Context<Self>,
        change: impl FnOnce(&mut Review) -> T,
    ) -> T {
        let result = change(&mut self.review);
        if self.rows.shows(self.review.page()) {
            cx.notify();
        } else {
            self.rebuild_rows(cx);
        }
        result
    }

    /// The drag selection only changes how rows look, which is read when they are drawn.
    pub fn select_from(&mut self, at: LineLocation, cx: &mut Context<Self>) {
        self.review.begin_selection(at);
        cx.notify();
    }

    /// A click on a line focuses the change it is in.
    pub fn focus_change_of(&mut self, at: &LineLocation, cx: &mut Context<Self>) {
        let Some(row) = self.review.page().row_of_line(at) else {
            return;
        };
        if self.review.focus_change_at_row(row) {
            self.rebuild_rows(cx);
        }
    }

    /// The focus follows the scroll: these rows of the page are in view.
    pub fn follow_scroll(&mut self, visible: Range<usize>, cx: &mut Context<Self>) {
        if self.review.follow_scroll(visible) {
            self.rebuild_rows(cx);
        }
    }

    pub fn extend_selection(&mut self, at: LineLocation, cx: &mut Context<Self>) {
        if self.review.is_selecting() {
            self.review.extend_selection(at);
            cx.notify();
        }
    }

    /// Only the keys move; the rows stay as they are.
    pub fn compose(&mut self, place: CommentPlace, cx: &mut Context<Self>) {
        // Taking the keys is never refused: it changes nothing that is saved.
        self.review.open_box(&TextBox::Draft(place)).ok();
        cx.notify();
    }

    /// A refused or failed save is one of the review's problems, which the rebuilt sidebar shows.
    pub fn leave(&mut self, text_box: &TextBox, cx: &mut Context<Self>) {
        self.change(cx, |review| review.leave(text_box)).ok();
    }

    /// A refused or failed save is one of the review's problems, which the rebuilt sidebar shows.
    pub fn discard(&mut self, text_box: &TextBox, cx: &mut Context<Self>) {
        self.change(cx, |review| review.discard(text_box)).ok();
    }

    pub fn type_in(&mut self, text_box: &TextBox, text: &str, cx: &mut Context<Self>) {
        let lines_before = self.review.text_of(text_box).map(line_count);
        let kept = self.review.type_in(text_box, text);
        self.typed(&kept, BoxHeight::of(text_box, lines_before, text), cx);
    }

    pub fn apply(&mut self, change: ReviewChange, cx: &mut Context<Self>) {
        // A refused or failed tick is one of the review's problems, which the rebuilt sidebar shows.
        self.change(cx, |review| review.apply(change)).ok();
    }

    /// No change of the page, so nothing is drawn again for it.
    pub fn saw_end_of_screen(&mut self) {
        self.review.saw_end_of_screen();
    }

    pub fn dismiss_finding(
        &mut self,
        id: &FindingId,
        cx: &mut Context<Self>,
    ) -> Result<(), FindingRefusal> {
        self.change(cx, |review| review.dismiss_finding(id))
    }

    /// Dismiss in the findings list: each selected finding goes, and a refused one is told back.
    pub fn dismiss_findings(
        &mut self,
        ids: &[FindingId],
        cx: &mut Context<Self>,
    ) -> Vec<(FindingId, FindingRefusal)> {
        self.change(cx, |review| review.dismiss_findings(ids))
            .refused
    }

    /// Restore in the findings list: each dismissed finding is open again, and a refused one is told back.
    pub fn restore_findings(
        &mut self,
        ids: &[FindingId],
        cx: &mut Context<Self>,
    ) -> Vec<(FindingId, FindingRefusal)> {
        self.change(cx, |review| review.restore_findings(ids))
            .refused
    }

    /// Fix, on one finding or in the findings list: the findings go to the tour agent as one message.
    pub fn fix_findings(
        &mut self,
        ids: &[FindingId],
        cx: &mut Context<Self>,
    ) -> Vec<(FindingId, FindingRefusal)> {
        self.change(cx, |review| review.fix_findings(ids)).refused
    }

    pub fn add_to_fixes(
        &mut self,
        ids: &[FindingId],
        cx: &mut Context<Self>,
    ) -> Vec<(FindingId, FindingRefusal)> {
        self.change(cx, |review| review.add_to_fixes(ids)).refused
    }

    pub fn remove_from_fixes(
        &mut self,
        id: &FindingId,
        cx: &mut Context<Self>,
    ) -> Result<(), FindingRefusal> {
        self.change(cx, |review| review.remove_from_fixes(id))
    }

    pub fn fix_with_note(
        &mut self,
        id: &FindingId,
        note: FixNote,
        timing: FixTiming,
        cx: &mut Context<Self>,
    ) -> Result<(), FindingRefusal> {
        self.change(cx, |review| review.fix_with_note(id, note, timing))
    }

    pub fn mark_fixed(
        &mut self,
        id: &FindingId,
        cx: &mut Context<Self>,
    ) -> Result<(), FindingRefusal> {
        self.change(cx, |review| review.mark_fixed(id))
    }

    pub fn send_fixes(&mut self, cx: &mut Context<Self>) -> Vec<(FindingId, FindingRefusal)> {
        self.change(cx, |review| review.send_fixes()).refused
    }

    /// Comment in the findings list: each selected finding goes in as is, as Use as is does.
    pub fn comment_findings(
        &mut self,
        ids: &[FindingId],
        cx: &mut Context<Self>,
    ) -> Vec<(FindingId, FindingRefusal)> {
        let mode = self.review.add_mode();
        let batch = self.change(cx, |review| review.comment_findings(ids, mode));
        for request in batch.done {
            let place = request.place().clone();
            let audience = request.audience();
            self.run_comment_request(&place, request, (mode, audience), cx);
        }
        batch.refused
    }

    pub fn send_comment(
        &mut self,
        place: &CommentPlace,
        mode: SendMode,
        audience: Audience,
        cx: &mut Context<Self>,
    ) {
        let Some(request) = self.review.start_sending(place, mode, audience) else {
            return;
        };
        self.run_comment_request(place, request, (mode, audience), cx);
    }

    /// Use as is on a finding: it goes to Add to review, or Add to tasks in a local review, unread.
    pub fn use_finding_as_is(
        &mut self,
        id: &FindingId,
        cx: &mut Context<Self>,
    ) -> Result<(), FindingRefusal> {
        let mode = self.review.add_mode();
        let request = self.change(cx, |review| review.use_finding_as_is(id, mode))?;
        let place = request.place().clone();
        let audience = request.audience();
        self.run_comment_request(&place, request, (mode, audience), cx);
        Ok(())
    }

    fn run_comment_request(
        &mut self,
        place: &CommentPlace,
        request: CommentRequest,
        (mode, audience): (SendMode, Audience),
        cx: &mut Context<Self>,
    ) {
        let github = Arc::clone(&self.github);
        let state_file = self.review.state_file();
        let place = place.clone();
        cx.spawn(async move |this, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move { request.run(github.as_ref()) })
                .await;
            let mut outcome = Some(outcome);
            let applied = this.update(cx, |this, cx| {
                if let Some(outcome) = outcome.take() {
                    this.comment_sent(&place, outcome, (mode, audience), cx);
                }
            });
            // The window switched to another PR while this was in flight; the comment is not lost.
            if let (Err(_), Some(outcome)) = (applied, outcome)
                && let Err(error) = outcome.land_in(&state_file)
            {
                eprintln!("could not save the result of a comment that was sending: {error:#}");
            }
        })
        .detach();
    }

    /// A sent comment says so in a toast: its box goes away, which is easy to miss. A queued one
    /// shows its own note where the box was.
    fn comment_sent(
        &mut self,
        place: &CommentPlace,
        outcome: CommentOutcome,
        (mode, audience): (SendMode, Audience),
        cx: &mut Context<Self>,
    ) {
        let result = self.change(cx, |review| review.apply_comment(outcome));
        if matches!(result, Ok(CommentResult::Sent)) {
            post_toast(cx, ToastTone::Success, sent_note("Comment", mode, audience));
        }
        if self.review.composing() == Some(place) {
            cx.emit(SessionEvent::CommentBoxTakesKeysAgain);
        }
    }

    /// Runs a thread action in the background, then shows the threads as GitHub has them.
    pub fn act_on_thread(
        &mut self,
        action: ThreadAction,
        on_done: impl FnOnce(&ThreadActionResult, &mut App) + 'static,
        cx: &mut Context<Self>,
    ) {
        let note = thread_action_note(&action);
        let request = self.review.thread_action_request(action);
        let github = Arc::clone(&self.github);
        self.run_off_thread(
            cx,
            move || request.run(github.as_ref()),
            move |this, outcome, cx| {
                let result = this.change(cx, |review| review.apply_thread_action(outcome));
                if let (ThreadActionResult::Done, Some(note)) = (&result, note) {
                    post_toast(cx, ToastTone::Success, note);
                }
                on_done(&result, cx);
            },
        )
        .detach();
    }

    /// A local review's finish: the queued tasks go to the agent as the hand-offs deliver them.
    pub fn send_tasks(&mut self, cx: &mut Context<Self>) {
        let queued = self.review.queued_tasks();
        match self.change(cx, |review| review.send_tasks()) {
            Ok(()) if queued > 0 => {
                let tasks = if queued == 1 { "task" } else { "tasks" };
                post_toast(
                    cx,
                    ToastTone::Success,
                    format!("{queued} {tasks} handed to your agent"),
                );
            }
            Ok(()) => {}
            Err(error) => post_toast(
                cx,
                ToastTone::Danger,
                format!("Could not send the tasks: {error:#}"),
            ),
        }
    }

    /// Open PR of a local review: the agent is asked off the UI thread, since herdr is asked first.
    pub fn open_pr(&mut self, cx: &mut Context<Self>) {
        let agents = self.tour_work.tour_agents.clone();
        let subject = self.review.subject().clone();
        self.run_off_thread(
            cx,
            move || agents.ask_to_open_pr(&subject),
            |_, outcome, cx| {
                let (tone, line) = outcome.open_pr_toast();
                post_toast(cx, tone, line);
            },
        )
        .detach();
    }

    /// Sends the review in the background; `on_done` gets the answer.
    pub fn finish_review(
        &mut self,
        verdict: ReviewVerdict,
        body: &str,
        on_done: impl FnOnce(&FinishResult, &mut App) + 'static,
        cx: &mut Context<Self>,
    ) {
        let request = match self.review.finish_review_request(verdict, body) {
            Ok(request) => request,
            Err(refusal) => return on_done(&FinishResult::Refused(refusal), cx),
        };
        let github = Arc::clone(&self.github);
        self.run_off_thread(
            cx,
            move || request.run(github.as_ref()),
            move |this, outcome, cx| {
                let result = this.change(cx, |review| review.apply_finish(outcome));
                if result == FinishResult::Sent {
                    post_toast(cx, ToastTone::Success, finish_note(verdict));
                }
                on_done(&result, cx);
            },
        )
        .detach();
    }

    /// Drop review of a local review whose branch is gone, on the button only.
    pub fn drop_review(&mut self, cx: &mut Context<Self>) {
        self.start_job(ReplacingJob::DropReview, cx);
    }

    /// The move button, in either review.
    pub fn move_to_pr(&mut self, cx: &mut Context<Self>) {
        self.start_job(ReplacingJob::MoveToPr, cx);
    }

    /// Not now: the offer goes for that PR and the local review stays.
    pub fn decline_move(&mut self, cx: &mut Context<Self>) {
        self.change(cx, |review| review.decline_the_move());
        self.show_problems(cx);
    }

    /// Runs only on the button.
    pub fn repin(&mut self, cx: &mut Context<Self>) {
        self.start_job(ReplacingJob::Repin, cx);
    }

    /// Runs only on the reviewer's click; the basic review stays on screen until the guided one is built.
    pub fn switch_to_guided_tour(&mut self, cx: &mut Context<Self>) {
        self.start_job(ReplacingJob::SwitchToGuided, cx);
    }

    pub fn clone_offer(&self) -> Option<&CloneOffer> {
        self.clone_offer.as_ref()
    }

    pub fn pick_clone_folder(&mut self, folder: PathBuf, cx: &mut Context<Self>) {
        if let Some(offer) = self.clone_offer.as_mut() {
            offer.pick_folder(folder);
        }
        cx.notify();
    }

    pub fn decline_clone(&mut self, cx: &mut Context<Self>) {
        self.clone_offer = None;
        cx.notify();
    }

    /// Once the clone is saved, the tour is read again on it, so the screen stays where it is.
    pub fn start_clone(&mut self, cx: &mut Context<Self>) {
        let Some(job) = self.clone_offer.as_mut().and_then(CloneOffer::start) else {
            return;
        };
        cx.notify();
        self.run_off_thread(
            cx,
            move || job.run(&GhCloner),
            |this, cloned, cx| match cloned {
                Ok(()) => {
                    this.clone_offer = None;
                    this.reload_tour(cx);
                }
                Err(error) => {
                    if let Some(offer) = this.clone_offer.as_mut() {
                        offer.failed(format!("{error:#}"));
                    }
                    cx.notify();
                }
            },
        )
        .detach();
    }

    /// Runs when `open` names the PR already shown, and after a clone: the tour file is read again
    /// and the screen stays.
    pub fn reload_tour(&mut self, cx: &mut Context<Self>) {
        self.start_job(ReplacingJob::Reload, cx);
    }

    /// Runs only on the button: the user's `claude` rewrites the marked steps.
    pub fn refresh_tour(&mut self, cx: &mut Context<Self>) {
        self.start_job(ReplacingJob::Refresh, cx);
    }

    /// The review says whether the job may start; what it is typing goes to disk first, as the job
    /// reads the state file.
    fn start_job(&mut self, job: ReplacingJob, cx: &mut Context<Self>) {
        self.save_typing_now();
        if let Some(started) = self.change(cx, |review| review.start_job(job)) {
            self.run_job(started, cx);
        }
    }

    fn run_job(&mut self, started: StartedJob, cx: &mut Context<Self>) {
        let folders = self.tour_work.repinner.folders.clone();
        let run: Box<dyn FnOnce() -> JobOutcome + Send> = match started {
            StartedJob::Repin => {
                let (repinner, github) = (
                    Arc::clone(&self.tour_work.repinner),
                    Arc::clone(&self.github),
                );
                Box::new(move || JobOutcome::Repin(repinner.run(github.as_ref())))
            }
            StartedJob::Refresh => self.refresh_reporting_activity(cx),
            StartedJob::Reload => {
                let reloader = Arc::clone(&self.tour_work.reloader);
                Box::new(move || JobOutcome::Reload(reloader.run()))
            }
            StartedJob::SwitchToGuided => {
                let switcher = Arc::clone(&self.tour_work.switcher);
                Box::new(move || JobOutcome::SwitchToGuided(switcher.run()))
            }
            StartedJob::MoveToPr(started) => {
                let mover = crate::open_review::mover_of(&folders, &started.change_set);
                Box::new(move || {
                    JobOutcome::MoveToPr(mover.run(&started.offer, &GitHub::from_env()))
                })
            }
            StartedJob::DropReview(change_set) => Box::new(move || {
                JobOutcome::DropReview(OpenedChangeSet::new(&change_set, &folders).drop_review())
            }),
        };
        self.run_off_thread(cx, run, Self::land_job).detach();
    }

    /// Each tool claude uses shows in the banner, so a long refresh is seen to be working.
    fn refresh_reporting_activity(
        &self,
        cx: &mut Context<Self>,
    ) -> Box<dyn FnOnce() -> JobOutcome + Send> {
        let (activities, mut arrived) = futures::channel::mpsc::unbounded();
        cx.spawn(async move |this, cx| {
            while let Some(activity) = futures::StreamExt::next(&mut arrived).await {
                let shown = this.update(cx, |this, cx| {
                    this.review.job_did(ReplacingJob::Refresh, activity);
                    cx.notify();
                });
                if shown.is_err() {
                    break;
                }
            }
        })
        .detach();
        let refresher = Arc::clone(&self.tour_work.refresher);
        Box::new(move || {
            JobOutcome::Refresh(refresher.run(&mut |activity| {
                activities.unbounded_send(activity).ok();
            }))
        })
    }

    fn land_job(&mut self, outcome: JobOutcome, cx: &mut Context<Self>) {
        let switching = cx.global::<PrSwitching>().clone();
        match self.change(cx, |review| review.land_job(outcome)) {
            AfterJob::Nothing | AfterJob::Stays { toast: None } => {}
            AfterJob::Stays { toast: Some(toast) } => post_toast(cx, ToastTone::Danger, toast),
            AfterJob::Reopen {
                replacement,
                screen,
            } => self.reopen_on(*replacement, &screen, cx),
            AfterJob::ShowPr { moved_away, pr } => {
                switching.active.update(cx, |active, cx| {
                    active.moved_away(&moved_away);
                    cx.notify();
                });
                (switching.show)(pr.into());
            }
            AfterJob::DropAndReopen(local) => {
                (switching.dropped)(local);
                (switching.reopen)();
            }
            AfterJob::Dropped(subject) => (switching.dropped)(subject),
        }
        if let Some(due) = self.review.due_job() {
            self.run_job(due, cx);
        }
    }

    /// Runs `request` on a background thread, then hands its outcome to `apply` on the UI thread.
    /// Dropping the returned task cancels the apply.
    fn run_off_thread<Outcome: Send + 'static>(
        &self,
        cx: &mut Context<Self>,
        request: impl FnOnce() -> Outcome + Send + 'static,
        apply: impl FnOnce(&mut Self, Outcome, &mut Context<Self>) + 'static,
    ) -> Task<()> {
        cx.spawn(async move |this, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move { request() })
                .await;
            this.update(cx, |this, cx| apply(this, outcome, cx)).ok();
        })
    }

    /// GPUI's splice moves the view to the top of a replaced row; a reply box opening on a long thread
    /// would throw the reader to the thread's start. The view stays where it was in that row instead.
    fn splice_keeping_place(&self, old: Range<usize>, new_count: usize) {
        let top = self.screen_list.logical_scroll_top();
        self.screen_list.splice(old.clone(), new_count);
        // gpui measures every row only after a reset; spliced rows would count as 0 px until drawn,
        // so the scrollbar thumb shrank while the reader scrolled down past them. Rows already
        // measured are not measured again.
        self.screen_list.clone().measure_all();
        if old.contains(&top.item_ix) && new_count > 0 {
            self.screen_list.scroll_to(ListOffset {
                item_ix: top.item_ix.min(old.start + new_count - 1),
                offset_in_item: top.offset_in_item,
            });
        }
    }

    fn rebuild_rows(&mut self, cx: &mut Context<Self>) {
        let started = Instant::now();
        let rows = PageRows::of(&self.review, &self.open_details);
        match rows.change_from(&self.rows) {
            RowsChange::OtherScreen => self.screen_list.reset(rows.len()),
            RowsChange::Spliced { old, new_count } => self.splice_keeping_place(old, new_count),
            RowsChange::Unchanged => {}
        }
        self.rows = rows;
        #[cfg(feature = "snapshots")]
        if self.is_unread_end_staged {
            self.draw_unread_end();
        }
        self.show_sidebar(cx);
        self.old_head = self.review.old_head();
        self.refresh_brief = self.review.refresh_brief();
        if self.rows.has_pictures_waiting() {
            self.load_images(cx);
        }
        self.frame_stats
            .rows_rebuilt(self.rows.len(), started.elapsed());
        cx.notify();
    }

    /// Typing only redraws; the rows are built again when a box got a line more or less.
    /// Text the review did not keep is named by the review's problems, which the sidebar shows.
    fn typed(&mut self, kept: &anyhow::Result<()>, height: BoxHeight, cx: &mut Context<Self>) {
        self.save_typing_soon(cx);
        if height == BoxHeight::Changed {
            self.rebuild_rows(cx);
        } else if kept.is_err() {
            self.show_problems(cx);
        } else {
            cx.notify();
        }
    }

    /// Shows the open screen and the sidebar as if every step were read, without ticking anything,
    /// so no viewed mark goes to GitHub. The next rebuild of the rows undoes it.
    #[cfg(feature = "snapshots")]
    pub fn stage_all_read(&mut self, cx: &mut Context<Self>) {
        let total = self.sidebar.progress.total;
        self.sidebar.progress.read = total;
        for step in &mut self.sidebar.steps {
            step.is_ticked = true;
        }
        self.rows.stage_all_read(total, &self.open_details);
        self.screen_list.reset(self.rows.len());
        cx.notify();
    }

    /// The sidebar of a re-review of someone else's PR that others reviewed too, drawn only: nothing
    /// is asked of GitHub, so no 👀 goes on or off. The next rebuild of the rows undoes it.
    #[cfg(feature = "snapshots")]
    pub fn stage_re_review(&mut self, stage: StagedReReview, cx: &mut Context<Self>) {
        self.staged_re_review = Some(stage);
        self.sidebar = self.sidebar_rows();
        cx.notify();
    }

    #[cfg(feature = "snapshots")]
    fn draw_re_review(&self, stage: StagedReReview, sidebar: &mut SidebarRows) {
        use crate::app::components::finish_review::Finishing;
        use crate::app::sidebar_rows::{SidebarFinish, SidebarReviews, SidebarYourReview};
        use crate::viewer::{ReviewerKind, ReviewerLine, ReviewerStatus, StanceAge};

        let is_marked_done = stage == StagedReReview::MarkedDone;
        let reviewer = |name: &str, kind, status, age| ReviewerLine {
            name: name.into(),
            avatar_url: None,
            kind,
            status,
            age,
            is_viewer: false,
        };
        sidebar.reviews = Arc::new(SidebarReviews {
            yours: Some(SidebarYourReview {
                title: "You approved".into(),
                since: if is_marked_done {
                    "Last review 2026-11-18 · new commits checked"
                } else {
                    "Last review 2026-11-18 · PR has new commits since"
                }
                .into(),
                body: "Reads well. One naming nit inline, nothing blocking.".into(),
                is_body_long: false,
                status: ReviewerStatus::Approved,
                age: StanceAge::BeforeNewCommits,
                is_marked_done,
                avatar_url: self
                    .review
                    .pr_author()
                    .and_then(|author| author.avatar_url.as_deref())
                    .map(|url| url.to_owned().into()),
            }),
            others: vec![
                reviewer(
                    "alice",
                    ReviewerKind::Person,
                    ReviewerStatus::ChangesRequested,
                    StanceAge::Current,
                ),
                reviewer(
                    "bob",
                    ReviewerKind::Person,
                    ReviewerStatus::Approved,
                    StanceAge::BeforeNewCommits,
                ),
                reviewer(
                    "carol",
                    ReviewerKind::Person,
                    ReviewerStatus::Commented,
                    StanceAge::Current,
                ),
                reviewer(
                    "backend",
                    ReviewerKind::Team,
                    ReviewerStatus::Waiting,
                    StanceAge::Current,
                ),
            ],
        });
        sidebar.finish = SidebarFinish {
            has_last_review: true,
            can_mark_done: !is_marked_done,
            is_marked_done,
        };
        sidebar.finishing = Finishing::AnyVerdict;
    }

    /// The last screen as it ends when steps were passed unread, without ticking anything, so no
    /// viewed mark goes to GitHub. The next rebuild of the rows undoes it.
    #[cfg(feature = "snapshots")]
    pub fn stage_unread_end(&mut self, cx: &mut Context<Self>) {
        self.is_unread_end_staged = true;
        self.draw_unread_end();
        self.screen_list.reset(self.rows.len());
        cx.notify();
    }

    #[cfg(feature = "snapshots")]
    fn draw_unread_end(&mut self) {
        let progress = self.review.progress();
        self.rows.stage_unread_end(
            &self.review.tour_lead(),
            progress.total - progress.read,
            &self.open_details,
        );
    }

    /// The title bar as GitHub would have it say `status`; `unstage_pr_status` puts back what was read.
    #[cfg(feature = "snapshots")]
    pub fn stage_pr_status(&mut self, status: crate::viewer::PrStatus, cx: &mut Context<Self>) {
        if self.pr_status_before_staging.is_none() {
            self.pr_status_before_staging = self.review.pr_status().cloned();
        }
        self.review.pr_status_is(status.clone());
        self.staged_pr_status = Some(status);
        cx.notify();
    }

    /// The title bar as GitHub would have it name `issues`; `unstage_related_issues` takes them off.
    #[cfg(feature = "snapshots")]
    pub fn stage_related_issues(
        &mut self,
        issues: Vec<(
            crate::viewer::RelatedIssue,
            Option<crate::viewer::IssueDetails>,
        )>,
        cx: &mut Context<Self>,
    ) {
        self.review.stage_related_issues(issues.clone());
        self.staged_related_issues = issues;
        cx.notify();
    }

    /// The title bar as GitHub would have it name `origin`; `unstage_pr_origin` takes it off.
    #[cfg(feature = "snapshots")]
    pub fn stage_pr_origin(
        &mut self,
        origin: crate::viewer::PullRequestOrigin,
        cx: &mut Context<Self>,
    ) {
        self.review.stage_pr_origin(Some(origin.clone()));
        self.staged_pr_origin = Some(origin);
        cx.notify();
    }

    #[cfg(feature = "snapshots")]
    fn keep_staged_pr_origin(&mut self) {
        if let Some(origin) = self.staged_pr_origin.clone() {
            self.review.stage_pr_origin(Some(origin));
        }
    }

    #[cfg(feature = "snapshots")]
    pub fn unstage_pr_origin(&mut self, cx: &mut Context<Self>) {
        self.staged_pr_origin = None;
        self.review.stage_pr_origin(None);
        cx.notify();
    }

    #[cfg(feature = "snapshots")]
    fn keep_staged_related_issues(&mut self) {
        if !self.staged_related_issues.is_empty() {
            self.review
                .stage_related_issues(self.staged_related_issues.clone());
        }
    }

    #[cfg(feature = "snapshots")]
    pub fn unstage_related_issues(&mut self, cx: &mut Context<Self>) {
        self.staged_related_issues.clear();
        self.review.unstage_related_issues();
        cx.notify();
    }

    #[cfg(feature = "snapshots")]
    fn keep_staged_pr_status(&mut self) {
        if let Some(status) = self.staged_pr_status.clone() {
            self.review.pr_status_is(status);
        }
    }

    /// With no status read before, the staged one stays: the title bar scenes come last.
    #[cfg(feature = "snapshots")]
    pub fn unstage_pr_status(&mut self, cx: &mut Context<Self>) {
        self.staged_pr_status = None;
        if let Some(before) = self.pr_status_before_staging.take() {
            self.review.pr_status_is(before);
        }
        cx.notify();
    }

    /// The PR opened in Basic mode in place of its guided tour, which `unstage_basic` puts back.
    #[cfg(feature = "snapshots")]
    pub fn stage_basic(&mut self, cx: &mut Context<Self>) {
        let Ok(basic) = Review::open_basic(
            self.review.basic_tour_of_same_pin(),
            self.source.as_ref(),
            self.review.state_file(),
        ) else {
            return;
        };
        let guided = std::mem::replace(&mut self.review, basic);
        self.guided_before_staging.get_or_insert(guided);
        self.swapped_review(cx);
    }

    /// The PR in Basic mode with its first two files read. The ticks go into the state file before
    /// the review opens on it, so no viewed mark is wished for and nothing reaches GitHub.
    #[cfg(feature = "snapshots")]
    pub fn stage_basic_read(&mut self, cx: &mut Context<Self>) {
        let Ok(probe) = Review::open_basic(
            self.review.basic_tour_of_same_pin(),
            self.source.as_ref(),
            self.review.state_file(),
        ) else {
            return;
        };
        let head = probe.head().clone();
        let read: Vec<FilePath> = probe
            .overview_page()
            .files
            .into_iter()
            .take(2)
            .map(|file| file.path)
            .collect();
        let state_file = probe.state_file();
        if state_file
            .update(|state| {
                for path in &read {
                    state.tick_file(path, &head);
                }
            })
            .is_err()
        {
            return;
        }
        self.stage_basic(cx);
    }

    /// The PR in Basic mode with its guided tour ready: the banner in the page header and the sidebar.
    #[cfg(feature = "snapshots")]
    pub fn stage_basic_guided_ready(&mut self, cx: &mut Context<Self>) {
        self.stage_basic(cx);
        self.change(cx, |review| {
            review.guided_tour_is(crate::viewer::GuidedTourProgress::Ready);
        });
    }

    /// The PR in Basic mode while its guided tour is written: the stage, the agent's last line and one
    /// Show agent in the sidebar.
    #[cfg(feature = "snapshots")]
    pub fn stage_basic_guided_writing(&mut self, cx: &mut Context<Self>) {
        self.stage_basic(cx);
        let terminal = TerminalId::new("t11");
        let before = std::mem::replace(
            &mut self.tour_agent,
            TourAgentState::Agent {
                terminal,
                status: crate::viewer::AgentStatus::Working,
            },
        );
        self.tour_agent_before_staging.get_or_insert(before);
        self.change(cx, |review| {
            let mut writing = crate::viewer::TourWriting::starting();
            writing.status_is(crate::viewer::AgentStatus::Working);
            writing.screen_is(vec![
                String::new(),
                "⏺ Reading src/orders.rs".to_owned(),
                "⏺ Writing the tour: 4 steps, one diagram".to_owned(),
            ]);
            review.guided_tour_is(crate::viewer::GuidedTourProgress::Writing(writing));
        });
    }

    /// The local review of the throwaway clone on disk, as `open <folder>` leaves it, so the switcher
    /// lists it; the review on screen stays.
    #[cfg(feature = "snapshots")]
    pub fn stage_local_review_on_disk(cx: &App) {
        let folders = cx.global::<PrSwitching>().folders.clone();
        let made = crate::app::snapshots::local_repo_in(folders.home()).and_then(|folder| {
            let change_set = OpenedChangeSet::change_set_at(&folder)?;
            let subject = crate::review_subject::ReviewSubject::from(change_set);
            crate::open_review::open_review(&subject, &folder, &folders).map(drop)
        });
        if let Err(error) = made {
            eprintln!("could not stage a local review on disk: {error:#}");
        }
    }

    /// A local review of a throwaway clone in place of the PR: one commit ahead of `main`, a changed
    /// README and cart, and a note git does not know yet. `unstage_basic` puts the PR back.
    #[cfg(feature = "snapshots")]
    pub fn stage_local(&mut self, cx: &mut Context<Self>) {
        self.stage_local_then(cx, |_| Ok(()));
    }

    /// The local review with a task on the README's new line that its agent answered, kept in the
    /// review's state file as Send now and `agent-reply` keep it, and a task on the README added to
    /// tasks, waiting for Send tasks.
    #[cfg(feature = "snapshots")]
    pub fn stage_local_with_task(&mut self, cx: &mut Context<Self>) {
        let folders = cx.global::<PrSwitching>().folders.clone();
        let staged = crate::app::snapshots::local_repo_in(folders.home()).and_then(|folder| {
            let change_set = OpenedChangeSet::change_set_at(&folder)?;
            let subject = crate::review_subject::ReviewSubject::from(change_set);
            let opened = crate::open_review::open_review(&subject, &folder, &folders)?;
            let head = opened.review.head().clone();
            let store = crate::viewer::LocalThreadStore::in_file(opened.review.state_file());
            let line = crate::line_number::LineNumber::try_from(3_u32)?;
            let at = LineLocation {
                path: FilePath::new("README.md")?,
                side: crate::viewer::Side::Head,
                line,
            };
            let place = CommentPlace::Lines {
                start: at.clone(),
                end: at,
            };
            store.stage_answered_task(
                &place,
                &head,
                (
                    "Say that tax is added on top, so nobody reads the total as the price.",
                    "Done: the README now says the total is before tax, and `tax` names the rate.",
                ),
            )?;
            store.stage_queued_task(
                &CommentPlace::File(FilePath::new("README.md")?),
                &head,
                "Add a line on how to run the shop's tests.",
            )
        });
        if let Err(error) = staged {
            eprintln!("could not stage a task in the local review: {error:#}");
        }
        self.stage_local(cx);
    }

    /// The local review with a task held back from an agent that does not report its state, which offers
    /// Send anyway.
    #[cfg(feature = "snapshots")]
    pub fn stage_local_with_held_back_task(&mut self, cx: &mut Context<Self>) {
        let folders = cx.global::<PrSwitching>().folders.clone();
        let staged = crate::app::snapshots::local_repo_in(folders.home()).and_then(|folder| {
            let change_set = OpenedChangeSet::change_set_at(&folder)?;
            let subject = crate::review_subject::ReviewSubject::from(change_set);
            let opened = crate::open_review::open_review(&subject, &folder, &folders)?;
            let head = opened.review.head().clone();
            let at = LineLocation {
                path: FilePath::new("README.md")?,
                side: crate::viewer::Side::Head,
                line: crate::line_number::LineNumber::try_from(3_u32)?,
            };
            crate::viewer::LocalThreadStore::in_file(opened.review.state_file())
                .stage_held_back_task(
                    &CommentPlace::Lines {
                        start: at.clone(),
                        end: at,
                    },
                    &head,
                    "Name the currency next to every price in the README.",
                )
        });
        if let Err(error) = staged {
            eprintln!("could not stage a held back task in the local review: {error:#}");
        }
        self.stage_local(cx);
    }

    /// The local review whose branch has an open PR, so its sidebar offers Move to the PR. Looking for
    /// the PR on GitHub would answer nothing for the sample repo, so the answer is given here.
    #[cfg(feature = "snapshots")]
    pub fn stage_local_move_offer(&mut self, cx: &mut Context<Self>) {
        self.stage_local(cx);
        let Ok(pr) = "https://github.com/acme/shop/pull/7".parse() else {
            return;
        };
        self.change(cx, |review| {
            review.pull_of_branch_is(Some(crate::viewer::PullOfBranch {
                pr,
                title: "Add the cart".to_owned(),
            }));
        });
    }

    /// The local review with a file written in its folder after it opened, so its look at the folder
    /// says Changed on disk. `unstage_local_changed_on_disk` takes the file away again.
    #[cfg(feature = "snapshots")]
    pub fn stage_local_changed_on_disk(&mut self, cx: &mut Context<Self>) {
        self.stage_local_then(cx, |folder| {
            Ok(std::fs::write(
                folder.join(CHANGED_ON_DISK_FILE),
                "written after the snapshot\n",
            )?)
        });
    }

    #[cfg(feature = "snapshots")]
    pub fn unstage_local_changed_on_disk(&mut self, cx: &mut Context<Self>) {
        let folders = cx.global::<PrSwitching>().folders.clone();
        if let Ok(folder) = crate::app::snapshots::local_repo_in(folders.home()) {
            // Only a later scene of the same run could see it.
            std::fs::remove_file(folder.join(CHANGED_ON_DISK_FILE)).ok();
        }
        self.unstage_basic(cx);
    }

    #[cfg(feature = "snapshots")]
    fn stage_local_then(
        &mut self,
        cx: &mut Context<Self>,
        after_open: impl FnOnce(&std::path::Path) -> anyhow::Result<()>,
    ) {
        let folders = cx.global::<PrSwitching>().folders.clone();
        let opened = crate::app::snapshots::local_repo_in(folders.home()).and_then(|folder| {
            let change_set = OpenedChangeSet::change_set_at(&folder)?;
            let subject = crate::review_subject::ReviewSubject::from(change_set);
            let opened = crate::open_review::open_review(&subject, &folder, &folders)?;
            after_open(&folder)?;
            Ok(opened)
        });
        let opened = match opened {
            Ok(opened) => opened,
            Err(error) => {
                eprintln!("could not stage a local review: {error:#}");
                return;
            }
        };
        let pr_review = std::mem::replace(&mut self.review, opened.review);
        self.guided_before_staging.get_or_insert(pr_review);
        let pr_source = std::mem::replace(&mut self.source, opened.source);
        self.source_before_staging.get_or_insert(pr_source);
        self.swapped_review(cx);
    }

    #[cfg(feature = "snapshots")]
    pub fn unstage_basic(&mut self, cx: &mut Context<Self>) {
        if let Some(before) = self.tour_agent_before_staging.take() {
            self.tour_agent = before;
        }
        if let Some(source) = self.source_before_staging.take() {
            self.source = source;
        }
        if let Some(guided) = self.guided_before_staging.take() {
            self.review = guided;
            self.swapped_review(cx);
        }
    }

    #[cfg(feature = "snapshots")]
    fn swapped_review(&mut self, cx: &mut Context<Self>) {
        self.image_pictures.clear();
        self.loading_images.clear();
        self.rows = PageRows::none();
        self.change(cx, |_| {});
        cx.emit(SessionEvent::Reopened);
        self.start_background_work(cx);
    }

    /// Three reads that fail the way a GitHub outage fails them, through the review's own path.
    #[cfg(feature = "snapshots")]
    pub fn stage_outage(&mut self, cx: &mut Context<Self>) {
        use crate::viewer::{GitHubError, OutageCause, Read};

        let outage = GitHubError::NotAnswering(OutageCause::ServerError(503));
        self.review.could_not_read(Read::PrHead, &outage);
        self.review.could_not_read(Read::PrDescription, &outage);
        self.review.threads_not_read(&outage);
        self.show_problems(cx);
    }

    #[cfg(feature = "snapshots")]
    pub fn unstage_outage(&mut self, cx: &mut Context<Self>) {
        self.review
            .clear_problem(crate::viewer::ProblemKind::GitHubNotAnswering);
        self.show_problems(cx);
    }

    #[cfg(feature = "snapshots")]
    pub fn unstage(&mut self, cx: &mut Context<Self>) {
        self.staged_re_review = None;
        self.is_unread_end_staged = false;
        self.screen_list.reset(0);
        self.rows = PageRows::none();
        self.rebuild_rows(cx);
    }

    /// A refresh under way on a tour with many new pieces, as the banner shows it mid-run.
    #[cfg(feature = "snapshots")]
    pub fn stage_refresh_running(&mut self, cx: &mut Context<Self>) {
        self.refresh_brief = RefreshBrief {
            stale_steps: vec![crate::viewer::StaleStep {
                id: "staged".into(),
                title: "Staged step".to_owned(),
            }],
            unplaced_new_pieces: Vec::new(),
            new_or_changed_pieces: 48,
            pieces: 157,
        };
        self.staged_refresh = Some(JobProgress::Running {
            activity: Some("Read app/Pricing.php".into()),
        });
        cx.notify();
    }

    #[cfg(feature = "snapshots")]
    pub fn unstage_refresh_running(&mut self, cx: &mut Context<Self>) {
        self.refresh_brief = self.review.refresh_brief();
        self.staged_refresh = None;
        cx.notify();
    }

    /// Shows herdr running but failing in the sidebar, as it did when its registry writer stopped.
    #[cfg(feature = "snapshots")]
    pub fn stage_agent_link_off(&mut self, cx: &mut Context<Self>) {
        let off = TourAgentState::LinkOff(crate::viewer::AgentLinkOff::HerdrFailed(
            "cannot save terminals: registry writer stopped".to_owned(),
        ));
        let before = std::mem::replace(&mut self.tour_agent, off);
        self.tour_agent_before_staging.get_or_insert(before);
        self.show_problems(cx);
    }

    #[cfg(feature = "snapshots")]
    pub fn unstage_agent_link_off(&mut self, cx: &mut Context<Self>) {
        if let Some(before) = self.tour_agent_before_staging.take() {
            self.tour_agent = before;
            self.show_problems(cx);
        }
    }

    pub fn switch_failed(&mut self, reason: String, cx: &mut Context<Self>) {
        self.review.switch_failed(reason);
        self.show_problems(cx);
    }

    fn show_problems(&mut self, cx: &mut Context<Self>) {
        self.show_sidebar(cx);
        cx.notify();
    }

    /// The sidebar lists what went wrong in small red lines far from where the reader acted, so a
    /// problem that was not there before is also a toast that stays until dismissed.
    fn show_sidebar(&mut self, cx: &mut Context<Self>) {
        let sidebar = self.sidebar_rows();
        let before: Vec<Problem> = self
            .sidebar
            .problems
            .iter()
            .map(|shown| shown.problem.clone())
            .collect();
        let after: Vec<Problem> = sidebar
            .problems
            .iter()
            .map(|shown| shown.problem.clone())
            .collect();
        for announcement in announcements(&before, &after) {
            post_toast(cx, announcement.tone, announcement.text);
        }
        self.sidebar = sidebar;
    }

    /// The review a job built takes the place of the one shown, on `screen`.
    fn reopen_on(&mut self, repinned: RepinnedTour, screen: &Screen, cx: &mut Context<Self>) {
        self.save_typing_now();
        let replaced = std::mem::replace(&mut self.review, repinned.review);
        self.review.carry_on_from(replaced);
        self.source = repinned.source;
        self.image_pictures.clear();
        self.loading_images.clear();
        self.rows = PageRows::none();
        self.change(cx, |review| match screen {
            Screen::Step(id) => review.go_to(id),
            Screen::File(path) => review.go_to_file(path),
            _ => {}
        });
        cx.emit(SessionEvent::Reopened);
        self.start_background_work(cx);
    }

    /// What a freshly opened review needs: the step shows plain code at once, the colours and
    /// the context lines follow from the background, then the PR head, viewed marks and threads.
    fn start_background_work(&mut self, cx: &mut Context<Self>) {
        let highlights = self.review.highlight_request();
        let files_at_head = self.review.files_at_head_request();
        let source = Arc::clone(&self.source);
        self.run_off_thread(
            cx,
            move || {
                (
                    files_at_head.read(source.as_ref()),
                    highlights.compute(source.as_ref()),
                )
            },
            |this, (files_at_head, highlights), cx| {
                this.frame_stats.code_arrived();
                this.change(cx, |review| {
                    review.set_files_at_head(files_at_head);
                    review.display_options_mut().set_highlights(highlights);
                });
            },
        )
        .detach();
        self.follow_tour_agent(cx);
        self.resume_ai_review(cx);
        self.review.pull_from_github();
        self.run_due_work(cx);
    }

    /// Run AI review, Run again and Try again: the review takes the asking, and the review agent starts where
    /// `ReviewPlace` says, off the UI thread as it runs `git` and `hc`.
    pub fn run_ai_review(&mut self, cx: &mut Context<Self>) -> Result<(), AiReviewRefusal> {
        self.change(cx, |review| review.ask_to_run_ai_review())?;
        // The agent of an earlier run ends when the new one starts, which its follow must not take for
        // the end of the new run.
        self.ai_review_follow = None;
        let agents = review_agent::agents_of(cx.global::<PrSwitching>());
        let tour_agents = self.tour_work.tour_agents.clone();
        let folders = cx.global::<PrSwitching>().folders.clone();
        let (subject, head) = (self.review.subject().clone(), self.review.head().clone());
        self.ai_review_start = Some(self.run_off_thread(
            cx,
            move || {
                let repo = writer_folder(&subject, &folders);
                let tour_agent_folder = tour_agents.working_folder_of(&subject);
                agents.start(
                    &subject,
                    tour_agent_folder.as_deref(),
                    &repo,
                    &head,
                    &subject,
                )
            },
            |this, started, cx| {
                let has_started = started.is_ok();
                this.change(cx, |review| review.ai_review_agent_started(started));
                if has_started {
                    this.follow_ai_review(cx);
                }
            },
        ));
        Ok(())
    }

    /// After a restart, or a review opened again: a review agent still alive for the subject is the run.
    fn resume_ai_review(&mut self, cx: &mut Context<Self>) {
        let agents = review_agent::agents_of(cx.global::<PrSwitching>());
        let subject = self.review.subject().clone();
        self.ai_review_start = Some(self.run_off_thread(
            cx,
            move || agents.lookup(&subject),
            |this, found, cx| {
                if let TourAgentState::Agent { terminal, status } = found {
                    this.change(cx, |review| {
                        review.ai_review_agent_resumed(terminal, Some(status));
                    });
                    this.follow_ai_review(cx);
                    this.deliver_questions(cx);
                }
            },
        ));
    }

    /// Keeps the review agent's status in the run until its terminal ends. A
    /// thread of its own blocks on herdr's events, like the tour agent's follow.
    fn follow_ai_review(&mut self, cx: &mut Context<Self>) {
        let (changes, mut arrived) = futures::channel::mpsc::unbounded();
        let agents = review_agent::agents_of(cx.global::<PrSwitching>());
        let (followed, ended_by) = (agents.clone(), self.review.subject().clone());
        let subject = ended_by.clone();
        std::thread::spawn(move || {
            followed.follow(&subject, &mut |state| match state {
                TourAgentState::Agent { status, .. } => {
                    match changes.unbounded_send(Some(status)) {
                        Ok(()) => ControlFlow::Continue(()),
                        Err(_) => ControlFlow::Break(()),
                    }
                }
                _ => ControlFlow::Continue(()),
            });
            // The receiver is gone when the run ended first: nobody is left to tell the stream is over.
            changes.unbounded_send(None).ok();
        });
        self.ai_review_follow = Some(cx.spawn(async move |this, cx| {
            while let Some(status) = futures::StreamExt::next(&mut arrived).await {
                let Some(status) = status else {
                    this.update(cx, |this, cx| {
                        this.change(cx, |review| review.ai_review_agent_gone());
                    })
                    .ok();
                    cx.background_executor()
                        .spawn(async move {
                            if let Err(error) = agents.end(&ended_by) {
                                eprintln!(
                                    "could not end the review agent of {ended_by}: {error:#}"
                                );
                            }
                        })
                        .await;
                    break;
                };
                let shown = this.update(cx, |this, cx| {
                    this.change(cx, |review| review.ai_review_agent_status(status));
                    this.deliver_questions(cx);
                });
                if shown.is_err() {
                    break;
                }
            }
        }));
    }

    /// Ask why questions that wait go to the review agent once it is idle, off the UI thread as it runs `hc`.
    pub fn deliver_questions(&mut self, cx: &mut Context<Self>) {
        if self.is_delivering_questions {
            return;
        }
        let Some(delivery) = self.review.why_delivery() else {
            return;
        };
        self.is_delivering_questions = true;
        let agents = review_agent::agents_of(cx.global::<PrSwitching>());
        self.questions_delivery = Some(self.run_off_thread(
            cx,
            move || delivery.run(&agents),
            |this, outcome, cx| {
                this.is_delivering_questions = false;
                this.change(cx, |review| review.why_delivered(outcome));
            },
        ));
    }

    pub fn show_review_agent(&mut self, cx: &mut Context<Self>) {
        let agents = review_agent::agents_of(cx.global::<PrSwitching>());
        let subject = self.review.subject().clone();
        self.show_agent = Some(self.run_off_thread(
            cx,
            move || agents.show(&subject),
            |this, shown, cx| {
                this.review.show_agent_result(shown.as_ref().err());
                this.show_problems(cx);
            },
        ));
    }

    /// Looks for the PR's tour agent and keeps its status. The follow is a thread of its own because it
    /// blocks on herdr's events for as long as the session lives, and ends when the session is gone;
    /// it is a stream of news for the review, not work with one outcome, so it is not on the agenda.
    fn follow_tour_agent(&mut self, cx: &mut Context<Self>) {
        let (changes, mut arrived) = futures::channel::mpsc::unbounded();
        let agents = self.tour_work.tour_agents.clone();
        let subject = self.review.subject().clone();
        std::thread::spawn(move || {
            agents.follow(&subject, &mut |state| match changes.unbounded_send(state) {
                Ok(()) => ControlFlow::Continue(()),
                Err(_) => ControlFlow::Break(()),
            });
        });
        self.tour_agent_follow = Some(cx.spawn(async move |this, cx| {
            while let Some(state) = futures::StreamExt::next(&mut arrived).await {
                let shown = this.update(cx, |this, cx| {
                    this.review.tour_agent_is(&state);
                    this.tour_agent = state;
                    this.show_problems(cx);
                    this.run_due_work(cx);
                });
                if shown.is_err() {
                    break;
                }
            }
        }));
    }

    pub fn is_delivering_hand_offs(&self) -> bool {
        self.review.is_delivering_hand_offs()
    }

    /// Types the Ask into the reviewer's own tour agent off the UI thread, since herdr is asked first.
    /// Ends with what the Ask panel says of it.
    pub fn hand_ask_to_agent(
        &self,
        ask: &Ask,
        place: &str,
        anyway: bool,
        cx: &App,
    ) -> Task<OwnAgentHandOff> {
        let agents = self.tour_work.tour_agents.clone();
        let subject = self.review.subject().clone();
        let text = self.review.ask_hand_off_text(ask, place);
        cx.background_executor().spawn(async move {
            if anyway {
                agents.hand_to_own_agent_anyway(&subject, &text)
            } else {
                agents.hand_to_own_agent(&subject, &text)
            }
        })
    }

    /// Retry beside the message of an off link.
    pub fn retry_agent_link(&mut self, cx: &mut Context<Self>) {
        self.change(cx, |review| review.retry_agent_link());
    }

    /// Whether the comment box offers Ask agent.
    pub fn agent_link_is_on(&self) -> bool {
        !matches!(self.tour_agent, TourAgentState::LinkOff(_))
    }

    /// The button is offered only for a hand-off in a thread that can take it, so an error other than a
    /// save the sidebar names is a bug.
    pub fn press_hand_off_button(&mut self, button: HandOffButton, cx: &mut Context<Self>) {
        if let Err(error) = self.change(cx, |review| review.press_hand_off_button(button)) {
            eprintln!("could not {} for {button:?}: {error:#}", button.label());
        }
    }

    /// Someone else's comment goes to the tour agent; a refusal says why in a toast.
    pub fn hand_to_agent(&mut self, comment: &CommentId, cx: &mut Context<Self>) {
        match self.change(cx, |review| review.hand_to_agent(comment)) {
            Ok(()) => post_toast(cx, ToastTone::Success, "Handed to your agent"),
            Err(refused) => post_toast(cx, ToastTone::Danger, refused.to_string()),
        }
    }

    /// Makes the terminal the change set's agent, then looks for the agent again so the sidebar and the
    /// hand-offs follow it.
    pub fn use_tour_agent(&mut self, terminal: TerminalId, cx: &mut Context<Self>) {
        let agents = self.tour_work.tour_agents.clone();
        let subject = self.review.subject().clone();
        self.show_agent = Some(self.run_off_thread(
            cx,
            move || agents.claim(&subject, &terminal),
            |this, claimed, cx| {
                this.review.use_agent_result(claimed.as_ref().err());
                this.show_problems(cx);
                this.follow_tour_agent(cx);
            },
        ));
    }

    pub fn show_tour_agent(&mut self, cx: &mut Context<Self>) {
        let agents = self.tour_work.tour_agents.clone();
        let subject = self.review.subject().clone();
        self.show_agent = Some(self.run_off_thread(
            cx,
            move || agents.show(&subject),
            |this, shown, cx| {
                this.review.show_agent_result(shown.as_ref().err());
                this.show_problems(cx);
            },
        ));
    }

    fn sidebar_rows(&self) -> SidebarRows {
        #[cfg_attr(
            not(feature = "snapshots"),
            expect(unused_mut, reason = "staged below")
        )]
        let mut rows = SidebarRows::of(&self.review, &self.tour_agent);
        #[cfg(feature = "snapshots")]
        if let Some(stage) = self.staged_re_review {
            self.draw_re_review(stage, &mut rows);
        }
        rows
    }

    pub fn add_to_notes(&mut self, subject: &NoteSubject, cx: &mut Context<Self>) {
        let Some(request) = self.change(cx, |review| review.start_adding_to_notes(subject)) else {
            return;
        };
        let github = Arc::clone(&self.github);
        self.run_off_thread(
            cx,
            move || request.run(github.as_ref()),
            |this, outcome, cx| {
                // A save that is refused or fails is named by the review's problems.
                this.change(cx, |review| review.apply_to_notes(outcome))
                    .ok();
            },
        )
        .detach();
    }

    pub fn hand_off_replied(
        &mut self,
        id: HandOffId,
        replied: &AgentReplied,
        cx: &mut Context<Self>,
    ) -> anyhow::Result<()> {
        self.change(cx, |review| review.hand_off_replied(id, replied))
    }

    pub fn fix_replied(
        &mut self,
        finding: &FindingId,
        reply: &FixReply,
        cx: &mut Context<Self>,
    ) -> anyhow::Result<()> {
        let taken = self.change(cx, |review| review.fix_replied(finding, reply));
        if taken.is_ok() && matches!(reply, FixReply::Question { .. }) {
            post_toast(
                cx,
                ToastTone::Neutral,
                format!("The tour agent asks you something about finding {finding}"),
            );
        }
        taken
    }

    /// The reviewer's answer to the tour agent's question on a finding; it goes to the agent when it is idle.
    pub fn answer_fix_question(
        &mut self,
        id: &FindingId,
        answer: &str,
        cx: &mut Context<Self>,
    ) -> Result<(), FindingRefusal> {
        self.change(cx, |review| review.answer_fix_question(id, answer))
    }

    pub fn why_replied(
        &mut self,
        finding: &FindingId,
        reply: &WhyReply,
        cx: &mut Context<Self>,
    ) -> anyhow::Result<()> {
        self.change(cx, |review| review.why_replied(finding, reply))
    }

    /// What a review agent's word that it is done may first need: a findings file pinned to older
    /// commits than the review's (the agent finished after a re-pin) follows to them. Reads code, so
    /// it runs off the main thread.
    pub fn findings_catch_up(&self) -> impl FnOnce() -> anyhow::Result<()> + Send + use<> {
        let repinner = Arc::clone(&self.tour_work.repinner);
        let state_file = self.review.state_file();
        let tips = self.review.pinned_tips();
        move || {
            let source = (repinner.pick_code_source)(&tips)?;
            catch_up_findings(
                &repinner.folders,
                &repinner.subject,
                &state_file,
                source.as_ref(),
                &tips.base_branch_tip,
                &tips.head,
            )
        }
    }

    /// The review agent says its findings are on disk: how many the review shows now.
    pub fn findings_done(&mut self, cx: &mut Context<Self>) -> usize {
        let folders = self.tour_work.repinner.folders.clone();
        self.change(cx, |review| {
            review.findings_done(&folders);
            review.ai_review().counts().total()
        })
    }

    /// Loads the pictures of the screen once each, off the main thread. A load that ends after `reopen`
    /// is dropped: `loading_images` was cleared, and its picture may not match the new review.
    fn load_images(&mut self, cx: &mut Context<Self>) {
        for request in self.review.image_preview_requests() {
            if !self.loading_images.insert(request.path().clone()) {
                continue;
            }
            let (source, cache) = (Arc::clone(&self.source), self.image_cache.clone());
            self.run_off_thread(
                cx,
                move || {
                    let preview = request.load(source.as_ref(), &cache);
                    let pictures = ImagePictures::of(&preview);
                    (preview, pictures)
                },
                |this, (preview, pictures), cx| {
                    this.change_session(cx, |session| {
                        let path = preview.path.clone();
                        if session
                            .review
                            .display_options_mut()
                            .set_image_preview(preview)
                        {
                            session.loading_images.remove(&path);
                            session.image_pictures.insert(path, Rc::new(pictures));
                        }
                    });
                },
            )
            .detach();
        }
    }

    fn save_typing_soon(&mut self, cx: &mut Context<Self>) {
        self.typing_save = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(TYPING_SAVE_DEBOUNCE).await;
            this.update(cx, Self::save_typing).ok();
        }));
    }

    /// A failed save shows in the sidebar until one goes through.
    fn save_typing(&mut self, cx: &mut Context<Self>) {
        self.typing_save = None;
        let Some(save) = self.review.typing_save() else {
            return;
        };
        self.run_off_thread(
            cx,
            move || save.run(),
            |this, saved, cx| {
                if saved.is_err() || !this.sidebar.problems.is_empty() {
                    this.show_problems(cx);
                }
            },
        )
        .detach();
    }

    /// The review closes, so the 👀 on the PR comes off. Nobody is left to see a failure, so it is
    /// only logged.
    fn take_reviewing_mark_off(&mut self, on_close: OnClose) {
        let Some(removal) = self.review.close() else {
            return;
        };
        let github = Arc::clone(&self.github);
        let (landed, has_landed) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            if let Err(error) = removal.run(github.as_ref()) {
                eprintln!("could not take the reviewing mark off: {error:#}");
            }
            // The quit stopped waiting once the wait ran out, so nobody may be listening.
            landed.send(()).ok();
        });
        if on_close == OnClose::WaitForIt {
            // Past the wait the app quits anyway; the mark staying on is the price.
            has_landed.recv_timeout(REVIEWING_MARK_OFF_WAIT).ok();
        }
    }

    /// For when the file is about to be read or the app to end: what was typed goes to disk now.
    pub fn save_typing_now(&mut self) {
        self.typing_save = None;
        if let Some(save) = self.review.typing_save()
            && let Err(error) = save.run()
        {
            eprintln!("could not save the text: {error:#}");
        }
    }
}

/// What a toast says of a sent comment or reply.
fn sent_note(what: &str, mode: SendMode, audience: Audience) -> String {
    let done = match mode {
        SendMode::AddToReview => "added to your review",
        SendMode::CommentNow => "posted on GitHub",
        SendMode::SendNow => "kept as a task",
        SendMode::AddToTasks => return format!("{what} queued for Send tasks"),
    };
    match audience {
        Audience::Author => format!("{what} {done}"),
        Audience::Agent => format!("{what} {done} and handed to your agent"),
    }
}

/// What a toast says once a thread action went through; an edit or a reaction shows in place,
/// a deleted comment is gone, so a toast says it was.
fn thread_action_note(action: &ThreadAction) -> Option<String> {
    match action {
        ThreadAction::Reply { mode, audience, .. } => Some(sent_note("Reply", *mode, *audience)),
        ThreadAction::Resolve { .. } => Some("Thread resolved".to_owned()),
        ThreadAction::Unresolve { .. } => Some("Thread unresolved".to_owned()),
        ThreadAction::DeleteComment { .. } => Some("Comment deleted".to_owned()),
        ThreadAction::EditComment { .. } | ThreadAction::SetReaction { .. } => None,
    }
}

fn finish_note(verdict: ReviewVerdict) -> &'static str {
    match verdict {
        ReviewVerdict::Comment => "Review sent with your comments",
        ReviewVerdict::Approve => "Review sent: approved",
        ReviewVerdict::RequestChanges => "Review sent: changes requested",
    }
}
