//! The open review on the GPUI side: it owns the `Review`, the rows built from it, and every round trip
//! to GitHub, the code source and the tour tools. Layouts draw from it and forward the reader's input to it.

use crate::file_path::FilePath;
use std::collections::{HashMap, HashSet};
use std::ops::ControlFlow;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::{
    App, Context, EventEmitter, ListAlignment, ListOffset, ListState, Subscription, Task, Window,
    px,
};

use crate::app::components::image_row::ImagePictures;
use crate::app::components::markdown_view::OpenDetails;
use crate::app::components::new_commits_banner::RepinProgress;
use crate::app::components::refresh_banner::RefreshProgress;
use crate::app::components::toast_stack::post_toast;
use crate::app::frame_stats::FrameStatsHook;
use crate::app::page_rows::{PageRows, RowFacts, RowsChange, line_count};
use crate::app::sidebar_rows::SidebarRows;
use crate::code_source::CodeSource;
use crate::viewer::{
    AgentReplied, AgentStatus, Ask, Audience, CommentOutcome, CommentPlace, CommentResult,
    FinishResult, GitHubPort, HandOffId, ImageCache, LineLocation, NoteSubject, OldHead, Place,
    Read, RefreshBrief, RepinnedTour, Review, ReviewVerdict, Screen, SendMode, StepId, SyncState,
    TextBox, ThreadAction, ThreadActionResult, ToastTone, TourAgentState, TourAgents,
    TourRefresher, TourReloader, TourRepinner, WorkingCopy, follow_outdated,
};

const PUSH_DEBOUNCE: Duration = Duration::from_secs(1);
/// How long to wait before handing off again to an agent whose screen had not settled since it started.
const AGENT_START_RETRY: Duration = Duration::from_secs(3);
const TYPING_SAVE_DEBOUNCE: Duration = Duration::from_millis(500);
/// Rows laid out beyond the visible part, so a fast scroll does not show a blank edge.
const LIST_OVERDRAW: f32 = 300.;

/// The two buttons that move a tour on: Re-pin (no AI) and Refresh (the local `claude`).
pub struct TourWork {
    pub repinner: Arc<TourRepinner>,
    pub refresher: Arc<TourRefresher>,
    pub reloader: Arc<TourReloader>,
    pub tour_agents: TourAgents,
}

/// The selected lines with Asks on a screen. They live in the Ask panel, and each gets a row marker.
pub type AskedLinesOf = Box<dyn Fn(&Screen, &App) -> Vec<Place>>;

pub enum SessionEvent {
    /// A re-pin or a Refresh replaced the review: what the layout keeps about the old one is stale.
    Reopened,
}

/// Every change of the review goes through `change`, which rebuilds the rows and the sidebar and tells the
/// screen's list which rows changed. Frames only draw from those. A change notifies the session's observers.
pub struct ReviewSession {
    review: Review,
    rows: PageRows,
    sidebar: SidebarRows,
    tour_agent: TourAgentState,
    /// Why the last Show agent failed, until the next one goes through.
    tour_agent_follow: Option<Task<()>>,
    show_agent: Option<Task<()>>,
    /// A hand-off being typed into the tour agent; the next one waits for it.
    delivering: Option<Task<()>>,
    delivery_retry: Option<Task<()>>,
    old_head: Option<OldHead>,
    refresh_brief: RefreshBrief,
    screen_list: ListState,
    asked_lines_of: AskedLinesOf,
    github: Arc<dyn GitHubPort>,
    source: Arc<dyn CodeSource>,
    tour_work: TourWork,
    repin_progress: RepinProgress,
    refresh_progress: RefreshProgress,
    image_cache: ImageCache,
    image_pictures: HashMap<FilePath, Rc<ImagePictures>>,
    loading_images: HashSet<FilePath>,
    is_retrying_comments: bool,
    sync_state: SyncState,
    pr_head_check: Option<Task<()>>,
    reload: Option<Task<()>>,
    threads_pull: Option<Task<()>>,
    pr_description_pull: Option<Task<()>>,
    taking_seen_marks_off: Option<Task<()>>,
    working_copy_check: Option<Task<()>>,
    push_debounce: Option<Task<()>>,
    pending_review_check: Option<Task<()>>,
    last_review_pull: Option<Task<()>>,
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
    pub asked_lines_of: AskedLinesOf,
    pub frame_stats: FrameStatsHook,
}

impl EventEmitter<SessionEvent> for ReviewSession {}

impl ReviewSession {
    pub fn new(
        review: Review,
        wiring: SessionWiring,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let SessionWiring {
            source,
            github,
            tour_work,
            asked_lines_of,
            frame_stats,
        } = wiring;
        let focus_subscription = cx.observe_window_activation(window, |this, window, cx| {
            if window.is_window_active() {
                this.pull_from_github(cx);
            }
        });
        let open_details = OpenDetails::default();
        cx.on_app_quit(|this, _| {
            this.save_typing_now();
            async {}
        })
        .detach();
        let rows = PageRows::of(
            &review,
            Vec::new(),
            &RowFacts {
                open_details: &open_details,
            },
        );
        let mut session = Self {
            sidebar: SidebarRows::of(&review, &TourAgentState::Unknown),
            tour_agent: TourAgentState::Unknown,
            tour_agent_follow: None,
            show_agent: None,
            delivering: None,
            delivery_retry: None,
            old_head: review.old_head(),
            refresh_brief: review.refresh_brief(),
            screen_list: ListState::new(rows.len(), ListAlignment::Top, px(LIST_OVERDRAW)),
            rows,
            review,
            asked_lines_of,
            github,
            source,
            tour_work,
            repin_progress: RepinProgress::Idle,
            refresh_progress: RefreshProgress::Idle,
            image_cache: ImageCache::under(&std::env::home_dir().unwrap_or_default()),
            image_pictures: HashMap::new(),
            loading_images: HashSet::new(),
            is_retrying_comments: false,
            sync_state: SyncState::default(),
            pr_head_check: None,
            reload: None,
            threads_pull: None,
            pr_description_pull: None,
            taking_seen_marks_off: None,
            working_copy_check: None,
            push_debounce: None,
            pending_review_check: None,
            last_review_pull: None,
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

    pub fn repin_progress(&self) -> &RepinProgress {
        &self.repin_progress
    }

    pub fn refresh_progress(&self) -> &RefreshProgress {
        &self.refresh_progress
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
    fn change_session<T>(
        &mut self,
        cx: &mut Context<Self>,
        change: impl FnOnce(&mut Self) -> T,
    ) -> T {
        let result = change(self);
        self.rebuild_rows(cx);
        result
    }

    /// For when the Asks changed: their lines get row markers.
    pub fn asks_changed(&mut self, cx: &mut Context<Self>) {
        if (self.asked_lines_of)(self.review.screen(), cx) != self.rows.asked_lines() {
            self.rebuild_rows(cx);
        }
    }

    /// The drag selection only changes how rows look, which is read when they are drawn.
    pub fn select_from(&mut self, at: LineLocation, cx: &mut Context<Self>) {
        self.review.begin_selection(at);
        cx.notify();
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

    pub fn toggle_tick(&mut self, id: &StepId, cx: &mut Context<Self>) {
        // A refused or failed tick is one of the review's problems, which the rebuilt sidebar shows.
        self.change(cx, |review| {
            if review.is_ticked(id) {
                review.untick(id)
            } else {
                review.tick(id)
            }
        })
        .ok();
        self.sync_after_debounce(cx);
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
        let github = Arc::clone(&self.github);
        let state_file = self.review.state_file();
        cx.spawn(async move |this, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move { request.run(github.as_ref()) })
                .await;
            let mut outcome = Some(outcome);
            let applied = this.update(cx, |this, cx| {
                if let Some(outcome) = outcome.take() {
                    this.comment_sent(outcome, (mode, audience), cx);
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
        outcome: CommentOutcome,
        (mode, audience): (SendMode, Audience),
        cx: &mut Context<Self>,
    ) {
        let result = self.change(cx, |review| review.apply_comment(outcome));
        if matches!(result, Ok(CommentResult::Sent)) {
            post_toast(cx, ToastTone::Success, sent_note("Comment", mode, audience));
        }
        self.deliver_hand_offs(cx);
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
                this.deliver_hand_offs(cx);
                on_done(&result, cx);
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
                    this.pull_last_review(cx);
                    post_toast(cx, ToastTone::Success, finish_note(verdict));
                }
                on_done(&result, cx);
            },
        )
        .detach();
    }

    /// Runs only on the button. The old `Review` takes no changes while it runs and is replaced after.
    pub fn repin(&mut self, cx: &mut Context<Self>) {
        if self.repin_progress == RepinProgress::Running {
            return;
        }
        self.repin_progress = RepinProgress::Running;
        self.save_typing_now();
        self.change(cx, Review::start_repin);
        let (repinner, github) = (
            Arc::clone(&self.tour_work.repinner),
            Arc::clone(&self.github),
        );
        self.run_off_thread(
            cx,
            move || repinner.run(github.as_ref()),
            |this, repinned, cx| match repinned {
                Ok(repinned) => {
                    this.repin_progress = RepinProgress::Idle;
                    this.reopen(repinned, cx);
                }
                Err(error) => {
                    this.repin_progress = RepinProgress::Failed(format!("{error:#}"));
                    this.change(cx, Review::repin_failed);
                }
            },
        )
        .detach();
    }

    /// Runs when `open` names the PR already shown: the tour file is read again and the screen stays.
    /// A failed read keeps the review on screen and is printed for the app's log.
    pub fn reload_tour(&mut self, cx: &mut Context<Self>) {
        let reloader = Arc::clone(&self.tour_work.reloader);
        self.reload = Some(self.run_off_thread(
            cx,
            move || reloader.run(),
            |this, reloaded, cx| match reloaded {
                Ok(reloaded) => this.reopen(reloaded, cx),
                Err(error) => eprintln!("could not reload the tour: {error:#}"),
            },
        ));
    }

    /// Runs only on the button, in the background: the user's `claude` rewrites the marked steps.
    pub fn refresh_tour(&mut self, cx: &mut Context<Self>) {
        if self.refresh_progress == RefreshProgress::Running {
            return;
        }
        self.refresh_progress = RefreshProgress::Running;
        self.save_typing_now();
        cx.notify();
        let refresher = Arc::clone(&self.tour_work.refresher);
        self.run_off_thread(
            cx,
            move || refresher.run(),
            |this, refreshed, cx| match refreshed {
                Ok(refreshed) => {
                    this.refresh_progress = RefreshProgress::Idle;
                    this.reopen(refreshed, cx);
                }
                Err(error) => {
                    this.refresh_progress = RefreshProgress::Failed;
                    this.review.refresh_failed(&error);
                    this.show_problems(cx);
                }
            },
        )
        .detach();
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
    fn splice_keeping_place(&self, old: std::ops::Range<usize>, new_count: usize) {
        let top = self.screen_list.logical_scroll_top();
        self.screen_list.splice(old.clone(), new_count);
        if old.contains(&top.item_ix) && new_count > 0 {
            self.screen_list.scroll_to(ListOffset {
                item_ix: top.item_ix.min(old.start + new_count - 1),
                offset_in_item: top.offset_in_item,
            });
        }
    }

    fn rebuild_rows(&mut self, cx: &mut Context<Self>) {
        let started = Instant::now();
        let rows = PageRows::of(
            &self.review,
            (self.asked_lines_of)(self.review.screen(), cx),
            &RowFacts {
                open_details: &self.open_details,
            },
        );
        match rows.change_from(&self.rows) {
            RowsChange::OtherScreen => self.screen_list.reset(rows.len()),
            RowsChange::Spliced { old, new_count } => self.splice_keeping_place(old, new_count),
            RowsChange::Unchanged => {}
        }
        self.rows = rows;
        self.show_sidebar(cx);
        self.old_head = self.review.old_head();
        self.refresh_brief = self.review.refresh_brief();
        if self.rows.has_images_to_load() {
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
        self.rows.stage_all_read(total);
        self.screen_list.reset(self.rows.len());
        cx.notify();
    }

    #[cfg(feature = "snapshots")]
    pub fn unstage(&mut self, cx: &mut Context<Self>) {
        self.screen_list.reset(0);
        self.rows = PageRows::none();
        self.rebuild_rows(cx);
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
        for problem in &sidebar.problems {
            if !self.sidebar.problems.contains(problem) {
                post_toast(cx, ToastTone::Danger, problem.to_string());
            }
        }
        self.sidebar = sidebar;
    }

    fn reopen(&mut self, repinned: RepinnedTour, cx: &mut Context<Self>) {
        let screen = self.review.screen().clone();
        self.save_typing_now();
        let replaced = std::mem::replace(&mut self.review, repinned.review);
        self.review.keep_comment_boxes_of(replaced);
        self.source = repinned.source;
        self.image_pictures.clear();
        self.loading_images.clear();
        self.rows = PageRows::none();
        self.change(cx, |review| {
            if let Screen::Step(id) = screen {
                review.go_to(&id);
            }
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
        self.pull_from_github(cx);
    }

    /// Looks for the PR's tour agent and keeps its status. The follow is a thread of its own because it
    /// blocks on herdr's events for as long as the session lives, and ends when the session is gone.
    fn follow_tour_agent(&mut self, cx: &mut Context<Self>) {
        let (changes, mut arrived) = futures::channel::mpsc::unbounded();
        let agents = self.tour_work.tour_agents.clone();
        let pr = self.review.pr().clone();
        std::thread::spawn(move || {
            agents.follow(&pr, &mut |state| match changes.unbounded_send(state) {
                Ok(()) => ControlFlow::Continue(()),
                Err(_) => ControlFlow::Break(()),
            });
        });
        self.tour_agent_follow = Some(cx.spawn(async move |this, cx| {
            while let Some(state) = futures::StreamExt::next(&mut arrived).await {
                let shown = this.update(cx, |this, cx| {
                    if let TourAgentState::Agent { status, .. } = &state {
                        this.agent_status_is(*status);
                        this.take_seen_marks_off(cx);
                    }
                    this.tour_agent = state;
                    this.show_problems(cx);
                    this.deliver_hand_offs(cx);
                });
                if shown.is_err() {
                    break;
                }
            }
        }));
    }

    /// Types the Ask into the reviewer's own tour agent off the UI thread, since herdr is asked first.
    /// Ends with what the Ask panel says of it.
    pub fn is_delivering_hand_offs(&self) -> bool {
        self.delivering.is_some()
    }

    pub fn hand_ask_to_agent(&self, ask: &Ask, place: &str, cx: &App) -> Task<String> {
        let agents = self.tour_work.tour_agents.clone();
        let pr = self.review.pr().clone();
        let text = ask.hand_off_text(&pr, place);
        cx.background_executor()
            .spawn(async move { agents.hand_to_own_agent(&pr, &text).line() })
    }

    /// Whether the comment box offers Ask agent.
    pub fn agent_link_is_on(&self) -> bool {
        !matches!(self.tour_agent, TourAgentState::LinkOff(_))
    }

    fn agent_status_is(&mut self, status: AgentStatus) {
        let recorded = self
            .review
            .record_hand_offs(|hand_offs| hand_offs.agent_status_is(status));
        if let Err(error) = recorded {
            eprintln!("could not save where a hand-off is: {error:#}");
        }
    }

    /// One batch at a time, so a burst of status changes does not take a mark off twice.
    fn take_seen_marks_off(&mut self, cx: &mut Context<Self>) {
        if self.taking_seen_marks_off.is_some() {
            return;
        }
        let requests = self.review.hand_offs().seen_marks_to_take_off();
        if requests.is_empty() {
            return;
        }
        let github = Arc::clone(&self.github);
        self.taking_seen_marks_off = Some(self.run_off_thread(
            cx,
            move || {
                requests
                    .into_iter()
                    .map(|request| request.run(github.as_ref()))
                    .collect::<Vec<_>>()
            },
            |this, outcomes, cx| {
                this.taking_seen_marks_off = None;
                for outcome in outcomes {
                    if let Err(error) = this.change(cx, |review| {
                        review.record_hand_offs(|hand_offs| hand_offs.apply_seen_mark_off(outcome))
                    }) {
                        eprintln!("could not save where a hand-off is: {error:#}");
                    }
                }
            },
        ));
    }

    /// Types the oldest queued hand-off into the tour agent when it is idle. One at a time; the next goes
    /// when the agent next turns idle.
    fn deliver_hand_offs(&mut self, cx: &mut Context<Self>) {
        if self.delivering.is_some() {
            return;
        }
        let Some(request) = self.review.hand_offs().delivery_request(self.review.pr()) else {
            return;
        };
        let agents = self.tour_work.tour_agents.clone();
        let github = Arc::clone(&self.github);
        self.delivering = Some(self.run_off_thread(
            cx,
            move || request.run(&agents, github.as_ref()),
            |this, outcome, cx| {
                this.delivering = None;
                cx.notify();
                let started_an_agent = outcome.started_an_agent();
                let try_again_soon = outcome.wants_another_try_soon();
                if let Err(error) = this.change(cx, |review| {
                    review.record_hand_offs(|hand_offs| hand_offs.apply_delivery(outcome))
                }) {
                    eprintln!("could not save where a hand-off is: {error:#}");
                }
                if started_an_agent {
                    this.follow_tour_agent(cx);
                }
                if try_again_soon {
                    this.delivery_retry = Some(cx.spawn(async move |this, cx| {
                        cx.background_executor().timer(AGENT_START_RETRY).await;
                        this.update(cx, Self::deliver_hand_offs).ok();
                    }));
                }
            },
        ));
    }

    pub fn try_hand_off_again(&mut self, hand_off: HandOffId, cx: &mut Context<Self>) {
        let queued = self.change(cx, |review| {
            review.record_hand_offs(|hand_offs| hand_offs.try_again(hand_off))
        });
        if let Err(error) = queued.and_then(|tried| tried) {
            eprintln!("could not queue hand-off {hand_off} again: {error:#}");
        }
        self.deliver_hand_offs(cx);
    }

    pub fn show_tour_agent(&mut self, cx: &mut Context<Self>) {
        let agents = self.tour_work.tour_agents.clone();
        let pr = self.review.pr().clone();
        self.show_agent = Some(self.run_off_thread(
            cx,
            move || agents.show(&pr),
            |this, shown, cx| {
                this.review.show_agent_result(shown.as_ref().err());
                this.show_problems(cx);
            },
        ));
    }

    fn sidebar_rows(&self) -> SidebarRows {
        SidebarRows::of(&self.review, &self.tour_agent)
    }

    fn pull_from_github(&mut self, cx: &mut Context<Self>) {
        self.check_pr_head(cx);
        self.check_working_copy(cx);
        self.pull_pr_description(cx);
        self.sync(cx);
        self.pull_threads(cx);
        self.pull_pending_review(cx);
        self.pull_last_review(cx);
        self.retry_queued_comments(cx);
    }

    fn check_pr_head(&mut self, cx: &mut Context<Self>) {
        let (github, pr) = (Arc::clone(&self.github), self.review.pr().clone());
        self.pr_head_check = Some(self.run_off_thread(
            cx,
            move || github.pull_request_tips(&pr),
            |this, tips, cx| match tips {
                Ok(tips) => this.change(cx, |review| review.pr_head_is(tips.head)),
                Err(error) => this.change(cx, |review| review.could_not_read(Read::PrHead, &error)),
            },
        ));
    }

    /// Pushes what is queued and pulls what changed on GitHub, off the UI thread.
    fn sync(&mut self, cx: &mut Context<Self>) {
        if !self.sync_state.start_or_queue() {
            return;
        }
        let request = self.review.sync_request();
        let github = Arc::clone(&self.github);
        self.run_off_thread(
            cx,
            move || request.run(github.as_ref()),
            |this, outcome, cx| {
                if let Err(error) = this.change(cx, |review| review.apply_sync(outcome)) {
                    eprintln!("could not sync with GitHub: {error:#}");
                }
                if this.sync_state.finish_and_wants_again() {
                    this.sync(cx);
                }
            },
        )
        .detach();
    }

    fn sync_after_debounce(&mut self, cx: &mut Context<Self>) {
        self.push_debounce = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(PUSH_DEBOUNCE).await;
            this.update(cx, Self::sync).ok();
        }));
    }

    /// The reviewer may check out another branch while the app is open; focus is when to notice.
    fn check_working_copy(&mut self, cx: &mut Context<Self>) {
        let (source, head) = (Arc::clone(&self.source), self.review.head().clone());
        self.working_copy_check = Some(self.run_off_thread(
            cx,
            move || source.working_tree_is_at(&head),
            |this, is_at, cx| {
                this.change(cx, |review| review.working_copy_is(WorkingCopy::of(is_at)));
            },
        ));
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
                if let Err(error) = this.change(cx, |review| review.apply_to_notes(outcome)) {
                    eprintln!("warning: could not keep the PR's description: {error:#}");
                }
            },
        )
        .detach();
    }

    fn pull_pr_description(&mut self, cx: &mut Context<Self>) {
        let (github, pr) = (Arc::clone(&self.github), self.review.pr().clone());
        self.pr_description_pull = Some(self.run_off_thread(
            cx,
            move || github.pull_request_description(&pr),
            |this, body, cx| match body {
                Ok(body) => {
                    if let Err(error) = this.change(cx, |review| review.set_pr_description(&body)) {
                        eprintln!("warning: could not keep the PR's description: {error:#}");
                    }
                }
                Err(error) => {
                    this.change(cx, |review| {
                        review.could_not_read(Read::PrDescription, &error);
                    });
                }
            },
        ));
    }

    /// Outdated threads are followed to their lines in the same trip, so they never jump once shown.
    fn pull_threads(&mut self, cx: &mut Context<Self>) {
        let (github, pr) = (Arc::clone(&self.github), self.review.pr().clone());
        let (source, head) = (Arc::clone(&self.source), self.review.head().clone());
        self.threads_pull = Some(self.run_off_thread(
            cx,
            move || {
                github.review_threads(&pr).map(|threads| {
                    let followed = follow_outdated(&threads, source.as_ref(), &head);
                    (threads, followed)
                })
            },
            |this, threads, cx| match threads {
                Ok((threads, followed)) => this.change(cx, |review| {
                    review.set_threads(&threads);
                    review.set_followed_lines(followed);
                }),
                Err(error) => this.change(cx, |review| review.threads_not_read(&error)),
            },
        ));
    }

    /// The review the viewer sent last, which the finish button shows and updates.
    fn pull_last_review(&mut self, cx: &mut Context<Self>) {
        let (github, pr) = (Arc::clone(&self.github), self.review.pr().clone());
        self.last_review_pull = Some(self.run_off_thread(
            cx,
            move || github.viewer_latest_review(&pr),
            |this, last, cx| {
                this.change(cx, |review| match last {
                    Ok(last) => review.last_review_is(last),
                    Err(error) => review.could_not_read(Read::LastReview, &error),
                });
            },
        ));
    }

    /// The tour agent's reply is on GitHub: the hand-off is answered, and the threads and the pending
    /// review are read again so the reply shows under the asked comment.
    pub fn hand_off_replied(
        &mut self,
        id: HandOffId,
        replied: &AgentReplied,
        cx: &mut Context<Self>,
    ) -> anyhow::Result<()> {
        self.change(cx, |review| {
            review.record_hand_offs(|hand_offs| hand_offs.replied(id, replied))
        })??;
        self.pull_threads(cx);
        self.pull_pending_review(cx);
        Ok(())
    }

    /// What the finish dialog needs from GitHub: whether a pending review exists and who wrote the PR.
    fn pull_pending_review(&mut self, cx: &mut Context<Self>) {
        let (github, pr) = (Arc::clone(&self.github), self.review.pr().clone());
        self.pending_review_check = Some(self.run_off_thread(
            cx,
            move || (github.has_pending_review(&pr), github.viewer_authored(&pr)),
            |this, (exists, authored), cx| {
                this.change(cx, |review| {
                    match exists {
                        Ok(exists) => review.pending_review_is(exists),
                        Err(error) => review.could_not_read(Read::PendingReview, &error),
                    }
                    match authored {
                        Ok(authored) => review.viewer_authorship_is(authored),
                        Err(error) => review.could_not_read(Read::Authorship, &error),
                    }
                });
            },
        ));
    }

    /// Sends the queue oldest first, one comment at a time, until one does not go.
    fn retry_queued_comments(&mut self, cx: &mut Context<Self>) {
        if self.is_retrying_comments {
            return;
        }
        let Some(request) = self.review.queued_comment_request() else {
            return;
        };
        self.is_retrying_comments = true;
        let github = Arc::clone(&self.github);
        self.run_off_thread(
            cx,
            move || request.run(github.as_ref()),
            |this, outcome, cx| {
                this.is_retrying_comments = false;
                match this.change(cx, |review| review.apply_comment(outcome)) {
                    Ok(CommentResult::Sent) => {
                        this.retry_queued_comments(cx);
                    }
                    Ok(_) => {}
                    Err(error) => eprintln!("could not send a queued comment: {error:#}"),
                }
            },
        )
        .detach();
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
    };
    match audience {
        Audience::Author => format!("{what} {done}"),
        Audience::Agent => format!("{what} {done} and handed to your agent"),
    }
}

/// What a toast says once a thread action went through; an edit or a reaction shows in place.
fn thread_action_note(action: &ThreadAction) -> Option<String> {
    match action {
        ThreadAction::Reply { mode, audience, .. } => Some(sent_note("Reply", *mode, *audience)),
        ThreadAction::Resolve { .. } => Some("Thread resolved".to_owned()),
        ThreadAction::Unresolve { .. } => Some("Thread unresolved".to_owned()),
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
