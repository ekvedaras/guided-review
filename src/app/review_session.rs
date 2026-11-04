//! The open review on the GPUI side: it owns the `Review`, the rows built from it, and every round trip
//! to GitHub, the code source and the tour tools. Layouts draw from it and forward the reader's input to it.

use crate::file_path::FilePath;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::{App, Context, EventEmitter, ListAlignment, ListState, Subscription, Task, Window, px};

use crate::app::components::image_row::ImagePictures;
use crate::app::components::markdown_view::OpenDetails;
use crate::app::components::new_commits_banner::RepinProgress;
use crate::app::components::refresh_banner::RefreshProgress;
use crate::app::frame_stats::FrameStatsHook;
use crate::app::page_rows::{PageRows, RowFacts, RowsChange, line_count};
use crate::app::sidebar_rows::SidebarRows;
use crate::code_source::CodeSource;
use crate::viewer::{
    CommentPlace, CommentProblem, CommentRefusal, CommentResult, FinishResult, GitHubPort,
    ImageCache, LineLocation, OldHead, Place, RefreshBrief, RepinnedTour, Review, ReviewVerdict,
    Screen, SendMode, StepId, SyncState, ThreadAction, ThreadActionResult, TourRefresher,
    TourRepinner, WorkingCopy,
};

const PUSH_DEBOUNCE: Duration = Duration::from_secs(1);
const TYPING_SAVE_DEBOUNCE: Duration = Duration::from_millis(500);
/// Rows laid out beyond the visible part, so a fast scroll does not show a blank edge.
const LIST_OVERDRAW: f32 = 300.;

/// The two buttons that move a tour on: Re-pin (no AI) and Refresh (the local `claude`).
pub struct TourWork {
    pub repinner: Arc<TourRepinner>,
    pub refresher: Arc<TourRefresher>,
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
    /// The comment box that takes the keys.
    composing: Option<CommentPlace>,
    sending_comments: HashSet<CommentPlace>,
    comment_problems: HashMap<CommentPlace, CommentProblem>,
    is_retrying_comments: bool,
    sync_state: SyncState,
    pr_head_check: Option<Task<()>>,
    threads_pull: Option<Task<()>>,
    pr_description_pull: Option<Task<()>>,
    working_copy_check: Option<Task<()>>,
    push_debounce: Option<Task<()>>,
    pending_review_check: Option<Task<()>>,
    typing_save: Option<Task<()>>,
    frame_stats: FrameStatsHook,
    _focus_subscription: Subscription,
    _details_subscription: Subscription,
}

/// Typing only changes a box's height when its line count changes.
#[derive(Clone, Copy, PartialEq, Eq)]
enum BoxHeight {
    Same,
    Changed,
}

impl BoxHeight {
    fn of(lines_before: Option<usize>, text: &str) -> Self {
        if lines_before == Some(line_count(text)) {
            Self::Same
        } else {
            Self::Changed
        }
    }
}

impl EventEmitter<SessionEvent> for ReviewSession {}

impl ReviewSession {
    #[expect(
        clippy::too_many_arguments,
        reason = "the view is built from its collaborators, which the caller owns"
    )]
    pub fn new(
        review: Review,
        source: Arc<dyn CodeSource>,
        github: Arc<dyn GitHubPort>,
        tour_work: TourWork,
        asked_lines_of: AskedLinesOf,
        frame_stats: FrameStatsHook,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus_subscription = cx.observe_window_activation(window, |this, window, cx| {
            if window.is_window_active() {
                this.pull_from_github(cx);
            }
        });
        if !cx.has_global::<OpenDetails>() {
            cx.set_global(OpenDetails::default());
        }
        let details_subscription = cx.observe_global::<OpenDetails>(Self::rebuild_rows);
        cx.on_app_quit(|this, _| {
            this.save_typing_now();
            async {}
        })
        .detach();
        let rows = PageRows::of(
            &review,
            Vec::new(),
            &RowFacts {
                comment_problems: &HashMap::new(),
                sending_comments: &HashSet::new(),
                open_details: cx.global::<OpenDetails>(),
            },
        );
        let mut session = Self {
            sidebar: SidebarRows::of(&review),
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
            composing: None,
            sending_comments: HashSet::new(),
            comment_problems: HashMap::new(),
            is_retrying_comments: false,
            sync_state: SyncState::default(),
            pr_head_check: None,
            threads_pull: None,
            pr_description_pull: None,
            working_copy_check: None,
            push_debounce: None,
            pending_review_check: None,
            typing_save: None,
            frame_stats,
            _focus_subscription: focus_subscription,
            _details_subscription: details_subscription,
        };
        session.start_background_work(cx);
        session
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

    pub fn image_pictures(&self, path: &str) -> Option<Rc<ImagePictures>> {
        self.image_pictures.get(path).cloned()
    }

    pub fn composing(&self) -> Option<&CommentPlace> {
        self.composing.as_ref()
    }

    pub fn is_sending(&self, place: &CommentPlace) -> bool {
        self.sending_comments.contains(place)
    }

    /// Why the comment cannot go: its last refusal, or else the tour being behind the PR.
    pub fn comment_problem(&self, place: &CommentPlace) -> Option<CommentProblem> {
        self.comment_problems.get(place).cloned().or_else(|| {
            self.old_head
                .as_ref()
                .map(|_| CommentProblem::Refused(CommentRefusal::TourIsOld))
        })
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

    pub fn compose(&mut self, place: CommentPlace, cx: &mut Context<Self>) {
        self.composing = Some(place);
        cx.notify();
    }

    pub fn stop_composing(&mut self, cx: &mut Context<Self>) {
        self.composing = None;
        cx.notify();
    }

    pub fn type_in_comment(&mut self, place: &CommentPlace, text: &str, cx: &mut Context<Self>) {
        let lines_before = self
            .review
            .comment_drafts()
            .iter()
            .find(|draft| &draft.place == place)
            .map(|draft| line_count(&draft.body));
        let kept = self.review.type_in_draft(place, text);
        self.typed(&kept, BoxHeight::of(lines_before, text), cx);
    }

    pub fn type_in_reply(&mut self, thread_id: &str, text: &str, cx: &mut Context<Self>) {
        let lines_before = self.review.reply_draft(thread_id).map(line_count);
        let kept = self.review.type_in_reply(thread_id, text);
        self.typed(&kept, BoxHeight::of(lines_before, text), cx);
    }

    pub fn type_in_finish_body(&mut self, text: &str, cx: &mut Context<Self>) {
        let kept = self.review.type_in_finish_body(text);
        self.typed(&kept, BoxHeight::Same, cx);
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

    pub fn discard_comment(&mut self, place: &CommentPlace, cx: &mut Context<Self>) {
        // A refused or failed save is one of the review's problems, which the rebuilt sidebar shows.
        self.change_session(cx, |session| {
            if session.composing.as_ref() == Some(place) {
                session.composing = None;
            }
            session.comment_problems.remove(place);
            session.review.discard_draft(place)
        })
        .ok();
    }

    pub fn send_comment(&mut self, place: CommentPlace, mode: SendMode, cx: &mut Context<Self>) {
        let Some(request) = self.review.comment_request(&place, mode) else {
            return;
        };
        self.sending_comments.insert(place.clone());
        self.comment_problems.remove(&place);
        let github = Arc::clone(&self.github);
        self.run_off_thread(
            cx,
            move || request.run(github.as_ref()),
            move |this, outcome, cx| {
                let is_sent = this.change_session(cx, |session| {
                    session.sending_comments.remove(&place);
                    let problem = match session.review.apply_comment(outcome) {
                        Ok(CommentResult::Sent) => {
                            session.composing = None;
                            return true;
                        }
                        Ok(CommentResult::Queued(_)) => {
                            session.composing = None;
                            return false;
                        }
                        Ok(CommentResult::Refused(refusal)) => CommentProblem::Refused(refusal),
                        Ok(CommentResult::Rejected(reason)) => {
                            CommentProblem::Rejected(reason.into())
                        }
                        Err(error) => CommentProblem::Failed(format!("{error:#}").into()),
                    };
                    session.comment_problems.insert(place, problem);
                    false
                });
                if is_sent {
                    this.pull_threads(cx);
                }
            },
        )
        .detach();
    }

    /// Runs a thread action in the background, then shows the threads as GitHub has them.
    pub fn act_on_thread(
        &mut self,
        action: ThreadAction,
        on_done: impl FnOnce(&ThreadActionResult, &mut App) + 'static,
        cx: &mut Context<Self>,
    ) {
        let request = self.review.thread_action_request(action);
        let github = Arc::clone(&self.github);
        self.run_off_thread(
            cx,
            move || request.run(github.as_ref()),
            move |this, outcome, cx| {
                let result = this.change(cx, |review| review.apply_thread_action(outcome));
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
                    this.refresh_progress = RefreshProgress::Failed(format!("{error:#}"));
                    cx.notify();
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

    fn rebuild_rows(&mut self, cx: &mut Context<Self>) {
        let started = Instant::now();
        let rows = PageRows::of(
            &self.review,
            (self.asked_lines_of)(self.review.screen(), cx),
            &RowFacts {
                comment_problems: &self.comment_problems,
                sending_comments: &self.sending_comments,
                open_details: cx.global::<OpenDetails>(),
            },
        );
        match rows.change_from(&self.rows) {
            RowsChange::OtherScreen => self.screen_list.reset(rows.len()),
            RowsChange::Spliced { old, new_count } => self.screen_list.splice(old, new_count),
            RowsChange::Unchanged => {}
        }
        self.rows = rows;
        self.sidebar = SidebarRows::of(&self.review);
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

    fn show_problems(&mut self, cx: &mut Context<Self>) {
        self.sidebar = SidebarRows::of(&self.review);
        cx.notify();
    }

    fn reopen(&mut self, repinned: RepinnedTour, cx: &mut Context<Self>) {
        let screen = self.review.screen().clone();
        self.save_typing_now();
        self.review = repinned.review;
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
                    review.set_highlights(highlights);
                });
            },
        )
        .detach();
        self.pull_from_github(cx);
    }

    fn pull_from_github(&mut self, cx: &mut Context<Self>) {
        self.check_pr_head(cx);
        self.check_working_copy(cx);
        self.pull_pr_description(cx);
        self.sync(cx);
        self.pull_threads(cx);
        self.pull_pending_review(cx);
        self.retry_queued_comments(cx);
    }

    fn check_pr_head(&mut self, cx: &mut Context<Self>) {
        let (github, pr) = (Arc::clone(&self.github), self.review.pr().clone());
        self.pr_head_check = Some(self.run_off_thread(
            cx,
            move || github.pull_request_tips(&pr),
            |this, tips, cx| match tips {
                Ok(tips) => this.change(cx, |review| review.pr_head_is(tips.head)),
                Err(error) => eprintln!("warning: could not check the PR's head: {error:#}"),
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

    fn pull_pr_description(&mut self, cx: &mut Context<Self>) {
        let (github, pr) = (Arc::clone(&self.github), self.review.pr().clone());
        self.pr_description_pull = Some(self.run_off_thread(
            cx,
            move || github.pull_request_description(&pr),
            |this, body, cx| {
                let set = body
                    .and_then(|body| this.change(cx, |review| review.set_pr_description(&body)));
                if let Err(error) = set {
                    eprintln!("warning: could not read the PR's description: {error:#}");
                }
            },
        ));
    }

    fn pull_threads(&mut self, cx: &mut Context<Self>) {
        let (github, pr) = (Arc::clone(&self.github), self.review.pr().clone());
        self.threads_pull = Some(self.run_off_thread(
            cx,
            move || github.review_threads(&pr),
            |this, threads, cx| match threads {
                Ok(threads) => this.change(cx, |review| review.set_threads(&threads)),
                Err(error) => this.change(cx, |review| review.threads_not_read(&error)),
            },
        ));
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
                        Err(error) => {
                            eprintln!("warning: could not look for a pending review: {error:#}");
                        }
                    }
                    match authored {
                        Ok(authored) => review.viewer_authorship_is(authored),
                        Err(error) => {
                            eprintln!("warning: could not tell who wrote the PR: {error:#}");
                        }
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
                        this.pull_threads(cx);
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
                        if session.review.set_image_preview(preview) {
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
    fn save_typing_now(&mut self) {
        self.typing_save = None;
        if let Some(save) = self.review.typing_save()
            && let Err(error) = save.run()
        {
            eprintln!("could not save the text: {error:#}");
        }
    }
}
