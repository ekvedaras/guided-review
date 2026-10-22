use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, App, AppContext, Context, Div, Entity, FocusHandle, Focusable, InteractiveElement,
    IntoElement, KeyDownEvent, ListAlignment, ListOffset, ListState, MouseButton, ParentElement,
    Render, SharedString, Stateful, StatefulInteractiveElement, Styled, Subscription, Task, Window,
    actions, div, list, px,
};

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::app::components::ask_panel::{
    AskPanel, AskSetup, UseAsComment, ask_button, lines_label,
};
use crate::app::components::comment_composer::{CommentComposer, OnCompose, QueuedCommentNote};
use crate::app::components::diagram::{DiagramView, Diagrams};
use crate::app::components::diff_view::{
    DiffRow, OnImageComment, OnMoreContext, OnOpenInIde, OnRowGesture, PieceHeader, RowGesture,
    binary_file_row, hunk_gap, no_change_since_read_row, only_whitespace_row, piece_end, piece_row,
};
use crate::app::components::finish_review::{FinishReviewDialog, OnBodyEdit, OnFinish};
use crate::app::components::fold_toggle::FoldToggle;
use crate::app::components::image_row::{ImagePictures, ImageRow};
use crate::app::components::jump_palette::{JumpEvent, JumpPalette};
use crate::app::components::new_commits_banner::{NewCommitsBanner, TourUpdate};
use crate::app::components::palette::{Palette, TEXT_FONT};
use crate::app::components::refresh_banner::RefreshBanner;
use crate::app::components::safety_net_view::SafetyNetIntro;
use crate::app::components::text_input::TextDraft;
use crate::app::components::thread_view::{
    OnReplyDraft, OnThreadAction, ReplyDraftChange, ThreadElement,
};
use crate::app::frame_stats::{FrameStats, FrameStatsMode, ScriptMove};
use crate::app::page_rows::{AskAt, PageRow, PageRows, RowsChange, SelectedRows, StepHeading};
use crate::app::sidebar_rows::SidebarRows;
use crate::code_source::CodeSource;
use crate::ide::{Ide, IdeLaunch};
use crate::viewer::{
    Ask, CommentPlace, CommentResult, FinishResult, GitHubPort, ImageCache, LineLocation,
    MoreContext, OldHead, Place, QueuedComment, RefreshBrief, Review, Screen, SendMode, StepId,
    ThreadView, TourRefresher, TourUpdater, UpdatedTour,
};

const PUSH_DEBOUNCE: Duration = Duration::from_secs(1);
/// Rows laid out beyond the visible part, so a fast scroll does not show a blank edge.
const LIST_OVERDRAW: f32 = 300.;

actions!(one_step, [GoForward, GoBack, ToggleJump]);

/// The two buttons that move a tour on: Update tour (no AI) and Refresh (the local `claude`).
pub struct TourWork {
    pub updater: Arc<TourUpdater>,
    pub refresher: Arc<TourRefresher>,
}

/// What the layout talks to outside the window.
pub struct Collaborators {
    pub github: Arc<dyn GitHubPort>,
    /// `None` hides every Open in IDE button.
    pub ide: Option<Ide>,
    pub tour_work: TourWork,
}

/// The one layout: step tree on the left, one screen on the right (the summary, or one step).
///
/// Every change of `review` goes through `review_changed`, which rebuilds the cached rows; frames only
/// draw from those caches. Looks-only state (the drag selection, hover, ask counts) is read when drawn.
pub struct OneStepLayout {
    review: Review,
    rows: PageRows,
    sidebar: SidebarRows,
    old_head: Option<OldHead>,
    refresh_brief: RefreshBrief,
    screen_list: ListState,
    on_row_gesture: OnRowGesture,
    github: Arc<dyn GitHubPort>,
    source: Arc<dyn CodeSource>,
    image_cache: ImageCache,
    image_pictures: HashMap<String, Rc<ImagePictures>>,
    loading_images: HashSet<String>,
    diagrams: Entity<Diagrams>,
    updater: Arc<TourUpdater>,
    tour_update: TourUpdate,
    refresher: Arc<TourRefresher>,
    tour_refresh: TourUpdate,
    focus: FocusHandle,
    ask: Entity<AskPanel>,
    pr_head_check: Option<Task<()>>,
    threads_pull: Option<Task<()>>,
    push_debounce: Option<Task<()>>,
    pending_review_check: Option<Task<()>>,
    /// The comment box that takes the keys.
    composing: Option<CommentPlace>,
    sending_comments: HashSet<CommentPlace>,
    comment_problems: HashMap<CommentPlace, String>,
    /// Why the last drag opened no comment box.
    selection_notice: Option<String>,
    /// The rows of the last drag, to ask the AI about.
    lines_to_ask: Option<Place>,
    is_retrying_comments: bool,
    is_syncing: bool,
    sync_again: bool,
    frame_stats: Option<FrameStats>,
    _focus_subscription: Subscription,
    jump: Entity<JumpPalette>,
    /// `None` hides every Open in IDE button.
    ide: Option<Ide>,
    _use_as_comment_subscription: Subscription,
    _asked_lines_subscription: Subscription,
}

impl OneStepLayout {
    pub const KEY_CONTEXT: &str = "OneStepLayout";

    pub fn new(
        review: Review,
        source: Arc<dyn CodeSource>,
        ask: AskSetup,
        collaborators: Collaborators,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus_subscription = cx.observe_window_activation(window, |this, window, cx| {
            if window.is_window_active() {
                this.check_pr_head(cx);
                this.sync(cx);
                this.pull_threads(cx);
                this.pull_pending_review(cx);
                this.retry_queued_comments(cx);
            }
        });
        let diagrams = Self::new_diagrams(cx);
        let focus = cx.focus_handle();
        let ask = cx.new(|cx| AskPanel::new(ask, focus.clone(), cx));
        let jump = cx.new(|cx| JumpPalette::new(focus.clone(), cx));
        cx.subscribe(&jump, |this, palette, event: &JumpEvent, cx| match event {
            JumpEvent::QueryChanged(query) => {
                let targets = this.review.jump_targets(query);
                palette.update(cx, |palette, _| palette.set_targets(targets));
            }
            JumpEvent::Picked(target) => {
                this.review.jump_to(target);
                this.review_changed(cx);
                let row = this.rows.row_of_jump(target);
                this.screen_list.scroll_to(ListOffset {
                    item_ix: row,
                    offset_in_item: px(0.),
                });
            }
        })
        .detach();
        let Collaborators {
            github,
            ide,
            tour_work,
        } = collaborators;
        let use_as_comment_subscription =
            cx.subscribe_in(&ask, window, |this, _, UseAsComment(ask), window, cx| {
                this.use_as_comment(ask, window, cx)
            });
        let asked_lines_subscription = cx.observe(&ask, |this, _, cx| {
            if this.asked_lines(cx) != this.rows.asked_lines() {
                this.review_changed(cx);
            }
        });
        let layout_handle = cx.weak_entity();
        let on_row_gesture: OnRowGesture = Rc::new(move |gesture, at, _window, cx| {
            layout_handle
                .update(cx, |this, cx| this.on_row_gesture(gesture, at, cx))
                .ok();
        });
        let rows = PageRows::of(&review, Vec::new());
        let screen_list = ListState::new(rows.len(), ListAlignment::Top, px(LIST_OVERDRAW));
        let mut layout = Self {
            sidebar: SidebarRows::of(&review),
            old_head: review.old_head(),
            refresh_brief: review.refresh_brief(),
            rows,
            screen_list,
            on_row_gesture,
            review,
            github,
            source: source.clone(),
            image_cache: ImageCache::under(&std::env::home_dir().unwrap_or_default()),
            image_pictures: HashMap::new(),
            loading_images: HashSet::new(),
            diagrams,
            updater: tour_work.updater,
            tour_update: TourUpdate::Idle,
            refresher: tour_work.refresher,
            tour_refresh: TourUpdate::Idle,
            focus,
            ask,
            jump,
            ide,
            pr_head_check: None,
            threads_pull: None,
            push_debounce: None,
            pending_review_check: None,
            composing: None,
            sending_comments: HashSet::new(),
            comment_problems: HashMap::new(),
            selection_notice: None,
            lines_to_ask: None,
            is_retrying_comments: false,
            is_syncing: false,
            sync_again: false,
            frame_stats: FrameStatsMode::from_env().map(FrameStats::new),
            _focus_subscription: focus_subscription,
            _use_as_comment_subscription: use_as_comment_subscription,
            _asked_lines_subscription: asked_lines_subscription,
        };
        if layout.frame_stats.is_some() {
            cx.on_app_quit(|this, _| {
                if let Some(stats) = &this.frame_stats {
                    stats.report_on_quit();
                }
                async {}
            })
            .detach();
        }
        layout.start_background_work(source, cx);
        layout
    }

    fn new_diagrams(cx: &mut Context<Self>) -> Entity<Diagrams> {
        let diagrams = cx.new(|_| Diagrams::default());
        cx.observe(&diagrams, |this, _, cx| {
            if let Some(row) = this.rows.diagram_row() {
                this.screen_list.remeasure_items(row..row + 1);
            }
            cx.notify();
        })
        .detach();
        diagrams
    }

    /// Call after every change of `review`: rebuilds the rows and the sidebar, and tells the list which rows
    /// changed, so the reader keeps their place unless the screen changed.
    fn review_changed(&mut self, cx: &mut Context<Self>) {
        let started = Instant::now();
        let rows = PageRows::of(&self.review, self.asked_lines(cx));
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
        if self.frame_stats.is_some() {
            eprintln!(
                "review changed: {} rows rebuilt in {:.1}ms",
                self.rows.len(),
                started.elapsed().as_secs_f64() * 1000.
            );
        }
        cx.notify();
    }

    /// The selected lines with Asks on the screen shown, which get a marker under their last row.
    fn asked_lines(&self, cx: &App) -> Vec<Place> {
        let step = match self.review.screen() {
            Screen::Step(id) => Some(id),
            Screen::SafetyNet => None,
            Screen::Summary => return Vec::new(),
        };
        self.ask
            .read(cx)
            .line_places(step)
            .into_iter()
            .map(|(place, _)| place)
            .collect()
    }

    /// What a freshly opened review needs: the step shows plain code at once, the colours and
    /// the context lines follow from the background, then the PR head, viewed marks and threads.
    fn start_background_work(&mut self, source: Arc<dyn CodeSource>, cx: &mut Context<Self>) {
        let request = self.review.highlight_request();
        let files_at_head_request = self.review.files_at_head_request();
        cx.spawn(async move |this, cx| {
            let (files_at_head, highlights) = cx
                .background_executor()
                .spawn(async move {
                    (
                        files_at_head_request.read(source.as_ref()),
                        request.compute(source.as_ref()),
                    )
                })
                .await;
            this.update(cx, |this, cx| {
                this.review.set_files_at_head(files_at_head);
                this.review.set_highlights(highlights);
                if let Some(stats) = this.frame_stats.as_mut() {
                    stats.code_arrived();
                }
                this.review_changed(cx);
            })
            .ok();
        })
        .detach();
        self.check_pr_head(cx);
        self.sync(cx);
        self.pull_threads(cx);
        self.pull_pending_review(cx);
        self.retry_queued_comments(cx);
    }

    /// Runs only on the button. The old `Review` takes no changes while it runs and is replaced after.
    fn update_tour(&mut self, cx: &mut Context<Self>) {
        if self.tour_update == TourUpdate::Running {
            return;
        }
        self.tour_update = TourUpdate::Running;
        self.review.start_repin();
        let (updater, github) = (self.updater.clone(), self.github.clone());
        cx.spawn(async move |this, cx| {
            let updated = cx
                .background_executor()
                .spawn(async move { updater.run(github.as_ref()) })
                .await;
            this.update(cx, |this, cx| {
                match updated {
                    Ok(updated) => {
                        this.tour_update = TourUpdate::Idle;
                        this.reopen(updated, cx);
                    }
                    Err(error) => {
                        this.review.repin_failed();
                        this.tour_update = TourUpdate::Failed(format!("{error:#}"));
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Runs only on the button, in the background: the user's `claude` rewrites the marked steps.
    fn refresh_tour(&mut self, cx: &mut Context<Self>) {
        if self.tour_refresh == TourUpdate::Running {
            return;
        }
        self.tour_refresh = TourUpdate::Running;
        let refresher = self.refresher.clone();
        cx.spawn(async move |this, cx| {
            let refreshed = cx
                .background_executor()
                .spawn(async move { refresher.run() })
                .await;
            this.update(cx, |this, cx| {
                match refreshed {
                    Ok(refreshed) => {
                        this.tour_refresh = TourUpdate::Idle;
                        this.reopen(refreshed, cx);
                    }
                    Err(error) => this.tour_refresh = TourUpdate::Failed(format!("{error:#}")),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn reopen(&mut self, updated: UpdatedTour, cx: &mut Context<Self>) {
        let screen = self.review.screen().clone();
        self.review = updated.review;
        self.source = updated.source.clone();
        self.image_pictures.clear();
        self.loading_images.clear();
        if let Screen::Step(id) = screen {
            self.review.go_to(&id);
        }
        let review = &self.review;
        self.ask
            .update(cx, |panel, cx| panel.review_changed(review, cx));
        self.diagrams = Self::new_diagrams(cx);
        self.rows = PageRows::none();
        self.review_changed(cx);
        self.start_background_work(updated.source, cx);
    }

    /// Folded steps are opened, so the script scrolls through their pieces too.
    fn run_scroll_script(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(stats) = self
            .frame_stats
            .as_mut()
            .filter(|stats| stats.is_scroll_script())
        else {
            return;
        };
        if let ScriptMove::ScreenDone(report) = stats.scroll_script_frame(&self.screen_list, window)
        {
            let active = if window.is_window_active() {
                "active"
            } else {
                "INACTIVE window"
            };
            eprintln!("{:?} ({active}): {report}", self.review.screen());
            if !self.review.go_forward() {
                cx.quit();
                return;
            }
            if let Screen::Step(id) = self.review.screen().clone()
                && self.review.is_folded(&id)
            {
                self.review.toggle_fold(&id);
            }
            self.review_changed(cx);
        }
        window.request_animation_frame();
    }

    fn ask_button(
        &self,
        key: String,
        place: Place,
        label: String,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let count = self.ask.read(cx).count_at(&place);
        ask_button(SharedString::from(key), count).on_click(cx.listener(
            move |this, _, window, cx| {
                this.ask.update(cx, |panel, cx| {
                    panel.open(&this.review, place.clone(), label.clone(), window, cx)
                });
            },
        ))
    }

    fn check_pr_head(&mut self, cx: &mut Context<Self>) {
        let pr = self.review.pr().clone();
        let github = self.github.clone();
        self.pr_head_check = Some(cx.spawn(async move |this, cx| {
            let tips = cx
                .background_executor()
                .spawn(async move { github.pull_request_tips(&pr) })
                .await;
            match tips {
                Ok(tips) => {
                    let _ = this.update(cx, |this, cx| {
                        this.review.pr_head_is(tips.head);
                        this.review_changed(cx);
                    });
                }
                Err(error) => eprintln!("warning: could not check the PR's head: {error:#}"),
            }
        }));
    }

    /// Pushes what is queued and pulls what changed on GitHub, off the UI thread.
    fn sync(&mut self, cx: &mut Context<Self>) {
        if self.is_syncing {
            self.sync_again = true;
            return;
        }
        self.is_syncing = true;
        let request = self.review.sync_request();
        let (github, pr) = (self.github.clone(), self.review.pr().clone());
        cx.spawn(async move |this, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move { request.run(github.as_ref(), &pr) })
                .await;
            this.update(cx, |this, cx| {
                if let Err(error) = this.review.apply_sync(outcome) {
                    eprintln!("could not sync with GitHub: {error:#}");
                }
                this.is_syncing = false;
                if std::mem::take(&mut this.sync_again) {
                    this.sync(cx);
                }
                this.review_changed(cx);
            })
            .ok();
        })
        .detach();
    }

    fn sync_after_debounce(&mut self, cx: &mut Context<Self>) {
        self.push_debounce = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(PUSH_DEBOUNCE).await;
            this.update(cx, |this, cx| this.sync(cx)).ok();
        }));
    }

    fn pull_threads(&mut self, cx: &mut Context<Self>) {
        let pr = self.review.pr().clone();
        let github = self.github.clone();
        self.threads_pull = Some(cx.spawn(async move |this, cx| {
            let threads = cx
                .background_executor()
                .spawn(async move { github.review_threads(&pr) })
                .await;
            match threads {
                Ok(threads) => {
                    let _ = this.update(cx, |this, cx| {
                        this.review.set_threads(&threads);
                        this.review_changed(cx);
                    });
                }
                Err(error) => eprintln!("warning: could not read the review threads: {error:#}"),
            }
        }));
    }

    /// What the finish dialog needs from GitHub: whether a pending review exists and who wrote the PR.
    fn pull_pending_review(&mut self, cx: &mut Context<Self>) {
        let pr = self.review.pr().clone();
        let github = self.github.clone();
        self.pending_review_check = Some(cx.spawn(async move |this, cx| {
            let (exists, authored) = cx
                .background_executor()
                .spawn(async move { (github.has_pending_review(&pr), github.viewer_authored(&pr)) })
                .await;
            let _ = this.update(cx, |this, cx| {
                match exists {
                    Ok(exists) => this.review.pending_review_is(exists),
                    Err(error) => {
                        eprintln!("warning: could not look for a pending review: {error:#}")
                    }
                }
                match authored {
                    Ok(authored) => this.review.viewer_authorship_is(authored),
                    Err(error) => eprintln!("warning: could not tell who wrote the PR: {error:#}"),
                }
                this.review_changed(cx);
            });
        }));
    }

    fn on_row_gesture(&mut self, gesture: RowGesture, at: &LineLocation, cx: &mut Context<Self>) {
        match gesture {
            RowGesture::Press => {
                self.selection_notice = None;
                self.lines_to_ask = None;
                self.review.begin_selection(at.clone());
            }
            RowGesture::Over if self.review.is_selecting() => {
                self.review.extend_selection(at.clone());
            }
            RowGesture::Over => return,
        }
        // The selection only changes how rows look, which is read when they are drawn.
        cx.notify();
    }

    fn end_row_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.review.is_selecting() {
            return;
        }
        self.lines_to_ask = self
            .review
            .selected_lines()
            .map(|selected| self.review.place_of_lines(&selected));
        match self.review.finish_selection() {
            Ok(Some(place)) => {
                self.composing = Some(place);
                window.focus(&self.focus, cx);
            }
            Ok(None) => {}
            Err(refusal) => self.selection_notice = Some(refusal.to_string()),
        }
        self.review_changed(cx);
    }

    fn begin_file_comment(&mut self, path: &str, window: &mut Window, cx: &mut Context<Self>) {
        match self.review.begin_file_comment(path) {
            Ok(place) => {
                self.composing = Some(place);
                window.focus(&self.focus, cx);
            }
            Err(refusal) => self.selection_notice = Some(refusal.to_string()),
        }
        self.review_changed(cx);
    }

    /// Loads the pictures of the screen once each, off the main thread. A load that ends after `reopen`
    /// is dropped: `loading_images` was cleared, and its picture may not match the new review.
    fn load_images(&mut self, cx: &mut Context<Self>) {
        for request in self.review.image_preview_requests() {
            if !self.loading_images.insert(request.path().to_string()) {
                continue;
            }
            let (source, cache) = (self.source.clone(), self.image_cache.clone());
            cx.spawn(async move |this, cx| {
                let (preview, pictures) = cx
                    .background_executor()
                    .spawn(async move {
                        let preview = request.load(source.as_ref(), &cache);
                        let pictures = ImagePictures::of(&preview);
                        (preview, pictures)
                    })
                    .await;
                this.update(cx, |this, cx| {
                    let path = preview.path.clone();
                    if !this.review.set_image_preview(preview) {
                        return;
                    }
                    this.loading_images.remove(&path);
                    this.image_pictures.insert(path, Rc::new(pictures));
                    this.review_changed(cx);
                })
                .ok();
            })
            .detach();
        }
    }

    /// The comments on the whole file are rows under it, not part of it.
    fn image_row(&self, path: &str, cx: &mut Context<Self>) -> ImageRow {
        let layout = cx.entity();
        let comment_path = path.to_string();
        let on_comment: OnImageComment = Rc::new(move |window, cx| {
            layout.update(cx, |this, cx| {
                this.begin_file_comment(&comment_path, window, cx)
            })
        });
        ImageRow::new(
            path.to_string(),
            self.review.image_preview(path).cloned(),
            self.image_pictures.get(path).cloned(),
            on_comment,
            Vec::new(),
        )
    }

    fn on_comment_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(place) = self.composing.clone() else {
            return;
        };
        if !self.focus.is_focused(window) {
            return;
        }
        let Some(draft) = self
            .review
            .comment_drafts()
            .iter()
            .find(|d| d.place == place)
        else {
            return;
        };
        let mut body = TextDraft::with_text(&draft.body);
        let keystroke = &event.keystroke;
        match keystroke.key.as_str() {
            "escape" => self.composing = None,
            "enter" if keystroke.modifiers.platform => {
                return self.send_comment(place, SendMode::AddToReview, cx);
            }
            "enter" => body.newline(),
            _ => {
                if !body.edit(keystroke) {
                    return;
                }
            }
        }
        if let Err(error) = self.review.edit_draft(&place, body.text()) {
            eprintln!("could not save the comment text: {error:#}");
        }
        self.review_changed(cx);
    }

    fn send_comment(&mut self, place: CommentPlace, mode: SendMode, cx: &mut Context<Self>) {
        let Some(request) = self.review.comment_request(&place, mode) else {
            return;
        };
        self.sending_comments.insert(place.clone());
        self.comment_problems.remove(&place);
        let (github, pr) = (self.github.clone(), self.review.pr().clone());
        cx.spawn(async move |this, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move { request.run(github.as_ref(), &pr) })
                .await;
            this.update(cx, |this, cx| {
                this.sending_comments.remove(&place);
                match this.review.apply_comment(outcome) {
                    Ok(CommentResult::Sent) => {
                        this.composing = None;
                        this.pull_threads(cx);
                    }
                    Ok(CommentResult::Queued(_)) => this.composing = None,
                    Ok(CommentResult::Refused(refusal)) => {
                        this.comment_problems.insert(place, refusal.to_string());
                    }
                    Ok(CommentResult::Rejected(reason)) => {
                        this.comment_problems.insert(place, reason);
                    }
                    Err(error) => {
                        this.comment_problems.insert(place, format!("{error:#}"));
                    }
                }
                this.review_changed(cx);
            })
            .ok();
        })
        .detach();
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
        let (github, pr) = (self.github.clone(), self.review.pr().clone());
        cx.spawn(async move |this, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move { request.run(github.as_ref(), &pr) })
                .await;
            this.update(cx, |this, cx| {
                this.is_retrying_comments = false;
                match this.review.apply_comment(outcome) {
                    Ok(CommentResult::Sent) => {
                        this.pull_threads(cx);
                        this.retry_queued_comments(cx);
                    }
                    Ok(_) => {}
                    Err(error) => eprintln!("could not send a queued comment: {error:#}"),
                }
                this.review_changed(cx);
            })
            .ok();
        })
        .detach();
    }

    fn discard_comment(&mut self, place: &CommentPlace, cx: &mut Context<Self>) {
        if let Err(error) = self.review.discard_draft(place) {
            eprintln!("could not save after discarding a comment: {error:#}");
        }
        if self.composing.as_ref() == Some(place) {
            self.composing = None;
        }
        self.comment_problems.remove(place);
        self.review_changed(cx);
    }

    fn on_compose(
        &self,
        place: &CommentPlace,
        cx: &mut Context<Self>,
        act: impl Fn(&mut Self, CommentPlace, &mut Window, &mut Context<Self>) + 'static,
    ) -> OnCompose {
        let (layout, place) = (cx.entity(), place.clone());
        Rc::new(move |window, cx| {
            layout.update(cx, |this, cx| act(this, place.clone(), window, cx))
        })
    }

    fn use_as_comment(&mut self, ask: &Ask, window: &mut Window, cx: &mut Context<Self>) {
        match self.review.use_as_comment(ask) {
            Ok(place) => {
                self.selection_notice = None;
                self.composing = Some(place);
                window.focus(&self.focus, cx);
            }
            Err(refusal) => self.selection_notice = Some(refusal.to_string()),
        }
        self.review_changed(cx);
    }

    fn queued_comment(&self, queued: &QueuedComment, cx: &mut Context<Self>) -> QueuedCommentNote {
        let on_discard = self.on_compose(&queued.draft.place, cx, |this, place, _, cx| {
            this.discard_comment(&place, cx)
        });
        QueuedCommentNote::new(queued.clone(), on_discard)
    }

    /// The draft is read here, so typing redraws the box without new rows.
    fn composer(&self, place: &CommentPlace, cx: &mut Context<Self>) -> Option<CommentComposer> {
        let draft = self
            .review
            .comment_drafts()
            .iter()
            .find(|draft| &draft.place == place)?;
        let problem = self.comment_problems.get(place).cloned().or_else(|| {
            self.old_head
                .as_ref()
                .map(|_| crate::viewer::CommentRefusal::TourIsOld.to_string())
        });
        Some(CommentComposer::new(
            draft.clone(),
            self.composing.as_ref() == Some(place),
            self.sending_comments.contains(place),
            self.review.can_comment_now(),
            self.review.can_suggest_change(place),
            problem,
            self.on_compose(place, cx, |this, place, window, cx| {
                this.composing = Some(place);
                window.focus(&this.focus, cx);
                cx.notify();
            }),
            self.on_compose(place, cx, |this, place, _, cx| {
                this.send_comment(place, SendMode::AddToReview, cx)
            }),
            self.on_compose(place, cx, |this, place, _, cx| {
                this.send_comment(place, SendMode::CommentNow, cx)
            }),
            self.on_compose(place, cx, |this, place, _, cx| {
                if let Err(error) = this.review.suggest_change(&place) {
                    eprintln!("could not save the suggestion: {error:#}");
                }
                this.review_changed(cx);
            }),
            self.on_compose(place, cx, |this, place, _, cx| {
                this.discard_comment(&place, cx)
            }),
        ))
    }

    /// Runs a thread action in the background, then shows the threads as GitHub has them.
    fn on_thread_action(&self, cx: &mut Context<Self>) -> OnThreadAction {
        let layout = cx.entity();
        Rc::new(move |action, on_done, cx| {
            let (request, github) = layout.update(cx, |this, _| {
                (
                    this.review.thread_action_request(action),
                    this.github.clone(),
                )
            });
            let layout = layout.clone();
            cx.spawn(async move |cx| {
                let outcome = cx
                    .background_executor()
                    .spawn(async move { request.run(github.as_ref()) })
                    .await;
                cx.update(|cx| {
                    let result = layout.update(cx, |this, cx| {
                        let result = this.review.apply_thread_action(outcome);
                        this.review_changed(cx);
                        result
                    });
                    on_done(&result, cx);
                });
            })
            .detach();
        })
    }

    /// Reply text goes to the core, which saves it, so it survives scrolling and restarts.
    fn on_reply_draft(&self, thread_id: &str, cx: &mut Context<Self>) -> OnReplyDraft {
        let (layout, thread_id) = (cx.entity(), thread_id.to_string());
        Rc::new(move |change, cx| {
            layout.update(cx, |this, cx| {
                let saved = match change {
                    ReplyDraftChange::Open => this.review.open_reply(&thread_id),
                    ReplyDraftChange::Edit(text) => this.review.edit_reply(&thread_id, &text),
                    ReplyDraftChange::Discard => this.review.discard_reply(&thread_id),
                };
                if let Err(error) = saved {
                    eprintln!("could not save the reply text: {error:#}");
                }
                this.review_changed(cx);
            });
        })
    }

    fn on_finish_body_edit(&self, cx: &mut Context<Self>) -> OnBodyEdit {
        let layout = cx.entity();
        Rc::new(move |text, cx| {
            layout.update(cx, |this, cx| {
                if let Err(error) = this.review.edit_finish_body(&text) {
                    eprintln!("could not save the review text: {error:#}");
                }
                cx.notify();
            });
        })
    }

    /// Sends the review in the background and shows the answer in the dialog.
    fn on_finish_review(&self, cx: &mut Context<Self>) -> OnFinish {
        let layout = cx.entity();
        Rc::new(move |verdict, body, on_done, cx| {
            let started = layout.update(cx, |this, _| {
                this.review
                    .finish_review_request(verdict, &body)
                    .map(|request| (request, this.github.clone()))
            });
            let (request, github) = match started {
                Ok(started) => started,
                Err(refusal) => return on_done(&FinishResult::Refused(refusal), cx),
            };
            let layout = layout.clone();
            cx.spawn(async move |cx| {
                let outcome = cx
                    .background_executor()
                    .spawn(async move { request.run(github.as_ref()) })
                    .await;
                cx.update(|cx| {
                    let result = layout.update(cx, |this, cx| {
                        let result = this.review.apply_finish(outcome);
                        this.review_changed(cx);
                        result
                    });
                    on_done(&result, cx);
                });
            })
            .detach();
        })
    }

    fn thread_element(&self, view: &ThreadView, cx: &mut Context<Self>) -> ThreadElement {
        let id = view.thread.id.clone();
        ThreadElement::new(
            view.clone(),
            self.review.can_comment_now(),
            view.reply_draft.clone(),
            self.on_reply_draft(&view.thread.id, cx),
            cx.listener(move |this, _, _, cx| {
                this.review.toggle_thread_fold(&id);
                this.review_changed(cx);
            }),
            self.on_thread_action(cx),
        )
    }

    fn toggle_jump(&mut self, _: &ToggleJump, window: &mut Window, cx: &mut Context<Self>) {
        let targets = self.review.jump_targets("");
        self.jump.update(cx, |palette, cx| {
            if palette.is_open() {
                palette.close(window, cx);
            } else {
                palette.open(targets, window, cx);
            }
        });
    }

    fn ide_button(
        &self,
        key: String,
        at: Option<&LineLocation>,
        cx: &mut Context<Self>,
    ) -> Option<Stateful<Div>> {
        let ide = self.ide.as_ref()?;
        let at = at?;
        let launch = ide.launch(Path::new(&at.path), at.line);
        Some(
            tool_button(key, "Open in IDE")
                .on_click(cx.listener(move |_, _, _, cx| run_launch(&launch, cx))),
        )
    }

    fn open_line_in_ide(&self) -> Option<OnOpenInIde> {
        let ide = self.ide.clone()?;
        Some(Rc::new(move |at: &LineLocation, cx: &mut App| {
            run_launch(&ide.launch(Path::new(&at.path), at.line), cx)
        }))
    }

    fn go_forward(&mut self, _: &GoForward, _: &mut Window, cx: &mut Context<Self>) {
        if self.review.go_forward() {
            self.review_changed(cx);
        }
    }

    fn go_back(&mut self, _: &GoBack, _: &mut Window, cx: &mut Context<Self>) {
        if self.review.go_back() {
            self.review_changed(cx);
        }
    }

    fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let sidebar = &self.sidebar;
        let summary_row = div()
            .id("summary")
            .px_3()
            .py_1()
            .cursor_pointer()
            .when_current(sidebar.is_summary_current)
            .on_click(cx.listener(|this, _, _, cx| {
                this.review.go_to_summary();
                this.review_changed(cx);
            }))
            .child("Summary");
        let progress_row = div()
            .px_3()
            .pb_2()
            .text_color(Palette::muted_text())
            .child(sidebar.progress.clone());
        let sync_row = sidebar.sync_banner.clone().map(|banner| {
            div()
                .px_3()
                .pb_2()
                .text_color(Palette::muted_text())
                .child(banner)
        });
        let not_synced_row = sidebar.not_synced.then(|| {
            div()
                .px_3()
                .pb_2()
                .text_color(Palette::muted_text())
                .child("● not synced with GitHub yet")
        });
        let finish_dialog = FinishReviewDialog::new(
            sidebar.can_judge,
            sidebar.approve_warning,
            self.review.finish_body().to_string(),
            self.on_finish_body_edit(cx),
            self.on_finish_review(cx),
        );
        let step_rows = sidebar.steps.iter().map(|step| {
            let id: StepId = step.id.clone();
            div()
                .id(SharedString::from(format!("step-{id}")))
                .pl(px(12. + 16. * step.depth as f32))
                .pr_3()
                .py_1()
                .cursor_pointer()
                .when_current(step.is_current)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.review.go_to(&id);
                    this.review_changed(cx);
                }))
                .when_ticked(step.is_ticked || step.is_gone)
                .child(step.label.clone())
        });
        let safety_net_row = sidebar.safety_net.as_ref().map(|safety_net| {
            div()
                .id("safety-net")
                .px_3()
                .py_1()
                .cursor_pointer()
                .when_current(safety_net.is_current)
                .on_click(cx.listener(|this, _, _, cx| {
                    this.review.go_to_safety_net();
                    this.review_changed(cx);
                }))
                .child(safety_net.label.clone())
        });
        div()
            .id("sidebar")
            .w(px(280.))
            .flex_none()
            .h_full()
            .overflow_y_scroll()
            .py_2()
            .bg(Palette::sidebar())
            .border_r_1()
            .border_color(Palette::border())
            .text_sm()
            .child(progress_row)
            .child(finish_dialog)
            .children(sync_row)
            .children(not_synced_row)
            .child(summary_row)
            .children(step_rows)
            .children(safety_net_row)
    }

    /// Strips of a diagram scrolled out of the list get no layout, so they are let go here.
    fn show_diagram(&self, window: &mut Window, cx: &mut Context<Self>) {
        let is_off_view = self.rows.diagram_row().is_some_and(|row| {
            self.screen_list.item_is_above_viewport(row) == Some(true)
                || self.screen_list.item_is_below_viewport(row) == Some(true)
        });
        self.diagrams.update(cx, |diagrams, cx| {
            diagrams.show(self.rows.diagram(), window, cx);
            if is_off_view {
                diagrams.leave_screen(window);
            }
        });
    }

    fn render_row(
        &mut self,
        index: usize,
        selected: Option<&SelectedRows>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(row) = self.rows.row(index).cloned() else {
            return div().into_any_element();
        };
        match row {
            PageRow::SummaryTitle(title) => div().pb_3().text_2xl().child(title).into_any_element(),
            PageRow::SummaryBullet(bullet) => div().pb_3().child(bullet).into_any_element(),
            PageRow::StepHeading(heading) => step_heading(&heading, cx).into_any_element(),
            PageRow::StepWhy(why) => div()
                .pb_3()
                .text_color(Palette::muted_text())
                .child(why)
                .into_any_element(),
            PageRow::Diagram => div()
                .pb_3()
                .child(DiagramView::new(self.diagrams.clone()))
                .into_any_element(),
            PageRow::Ask(ask) => div()
                .pb_3()
                .flex()
                .child(self.ask_at(&ask, cx))
                .into_any_element(),
            PageRow::StepNotes(notes) => div().pb_3().child(notes).into_any_element(),
            PageRow::WhitespaceToggle { is_on } => div()
                .pb_3()
                .flex()
                .child(whitespace_toggle(is_on, cx))
                .into_any_element(),
            PageRow::RepinNotes {
                gone_pieces,
                text_may_be_old,
            } => repin_notes(gone_pieces, text_may_be_old).into_any_element(),
            PageRow::FullDiffToggle {
                step,
                is_showing_full_diff,
            } => full_diff_toggle(step, is_showing_full_diff, cx).into_any_element(),
            PageRow::FoldToggle {
                step,
                is_folded,
                piece_count,
            } => div()
                .pb_3()
                .child(FoldToggle::new(
                    is_folded,
                    piece_count,
                    cx.listener(move |this, _, _, cx| {
                        this.review.toggle_fold(&step);
                        this.review_changed(cx);
                    }),
                ))
                .into_any_element(),
            PageRow::SafetyNetIntro => SafetyNetIntro.into_any_element(),
            PageRow::PieceHeader(heading) => {
                let ide = self.ide_button(
                    format!("ide-piece-{}", heading.piece),
                    heading.open_at.as_ref(),
                    cx,
                );
                let ask = heading.ask.as_ref().map(|ask| self.ask_at(ask, cx));
                PieceHeader::new(
                    heading.piece.clone(),
                    heading.repin_label,
                    ide.into_iter().chain(ask).collect(),
                )
                .into_any_element()
            }
            PageRow::NoChangeSinceRead => no_change_since_read_row().into_any_element(),
            PageRow::BinaryFile(link) => binary_file_row(link).into_any_element(),
            PageRow::Image { path, .. } => piece_row()
                .child(self.image_row(&path, cx))
                .into_any_element(),
            PageRow::OnlyWhitespace => only_whitespace_row().into_any_element(),
            PageRow::HunkGap { id, more } => {
                hunk_gap(id, more, self.on_more_context(cx)).into_any_element()
            }
            PageRow::Line(line) => {
                let is_selected = selected.is_some_and(|rows| rows.contains(index, &line.location));
                DiffRow::new(line)
                    .selected(is_selected)
                    .with_gestures(self.on_row_gesture.clone())
                    .with_open_in_ide(self.open_line_in_ide())
                    .into_any_element()
            }
            PageRow::Thread {
                view,
                is_under_a_line,
            } => {
                let thread = self.thread_element(&view, cx);
                if is_under_a_line {
                    piece_row().child(thread).into_any_element()
                } else {
                    div().pb_3().child(thread).into_any_element()
                }
            }
            PageRow::QueuedComment(queued) => piece_row()
                .child(self.queued_comment(&queued, cx))
                .into_any_element(),
            PageRow::Composer(place) => piece_row()
                .children(self.composer(&place, cx))
                .into_any_element(),
            PageRow::AskMarker(ask) => piece_row()
                .flex()
                .px_2()
                .py_1()
                .child(self.ask_at(&ask, cx))
                .into_any_element(),
            PageRow::PieceEnd => piece_end().into_any_element(),
        }
    }

    fn ask_at(&self, ask: &AskAt, cx: &mut Context<Self>) -> Stateful<Div> {
        self.ask_button(ask.id.to_string(), ask.place.clone(), ask.label.clone(), cx)
    }

    fn on_more_context(&self, cx: &mut Context<Self>) -> OnMoreContext {
        Rc::new(cx.listener(|this, more: &MoreContext, _, cx| {
            this.review.show_more_context(more);
            this.review_changed(cx);
        }))
    }

    fn toggle_tick(&mut self, id: &StepId, cx: &mut Context<Self>) {
        let saved = if self.review.is_ticked(id) {
            self.review.untick(id)
        } else {
            self.review.tick(id)
        };
        if let Err(error) = saved {
            eprintln!("could not save the read tick: {error:#}");
        }
        self.sync_after_debounce(cx);
        self.review_changed(cx);
    }
}

trait TickedRow {
    fn when_ticked(self, is_ticked: bool) -> Self;
}

impl<E: Styled> TickedRow for E {
    fn when_ticked(self, is_ticked: bool) -> Self {
        if is_ticked {
            self.text_color(Palette::muted_text())
        } else {
            self
        }
    }
}

trait CurrentRow {
    fn when_current(self, is_current: bool) -> Self;
}

impl<E: Styled> CurrentRow for E {
    fn when_current(self, is_current: bool) -> Self {
        if is_current {
            self.bg(Palette::current())
        } else {
            self
        }
    }
}

fn step_heading(heading: &StepHeading, cx: &mut Context<OneStepLayout>) -> impl IntoElement {
    let title = div()
        .id("step-title")
        .flex_1()
        .text_xl()
        .child(heading.title.clone());
    let title = if heading.folds_on_title_click {
        let id = heading.id.clone();
        title
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, _, cx| {
                this.review.toggle_fold(&id);
                this.review_changed(cx);
            }))
    } else {
        title
    };
    div()
        .pb_3()
        .flex()
        .items_center()
        .gap_3()
        .when(heading.is_gone, |row| row.text_color(Palette::muted_text()))
        .child(title)
        .child(tick_button(heading.is_ticked, heading.id.clone(), cx))
}

fn whitespace_toggle(is_on: bool, cx: &mut Context<OneStepLayout>) -> impl IntoElement {
    div()
        .id("ignore-whitespace")
        .px_3()
        .py_1()
        .rounded_md()
        .border_1()
        .border_color(Palette::border())
        .cursor_pointer()
        .text_sm()
        .when_current(is_on)
        .on_click(cx.listener(|this, _, _, cx| {
            this.review.toggle_ignore_whitespace();
            this.review_changed(cx);
        }))
        .child("Hide whitespace")
}

fn repin_notes(gone_pieces: usize, text_may_be_old: bool) -> impl IntoElement {
    div()
        .pb_3()
        .flex()
        .gap_3()
        .text_sm()
        .text_color(Palette::muted_text())
        .when(gone_pieces > 0, |row| {
            row.child(match gone_pieces {
                1 => "1 piece gone since last version".to_string(),
                count => format!("{count} pieces gone since last version"),
            })
        })
        .when(text_may_be_old, |row| row.child("Text may be old"))
}

fn full_diff_toggle(
    step: StepId,
    is_showing_full_diff: bool,
    cx: &mut Context<OneStepLayout>,
) -> impl IntoElement {
    let label = if is_showing_full_diff {
        "Show only the change since your read"
    } else {
        "Show the full diff"
    };
    div().pb_3().flex().child(
        div()
            .id("full-diff-toggle")
            .text_sm()
            .cursor_pointer()
            .text_color(Palette::muted_text())
            .on_click(cx.listener(move |this, _, _, cx| {
                this.review.toggle_full_diff(&step);
                this.review_changed(cx);
            }))
            .child(label),
    )
}

fn tool_button(id: impl Into<gpui::ElementId>, label: &'static str) -> Stateful<Div> {
    div()
        .id(id)
        .px_2()
        .py_1()
        .cursor_pointer()
        .text_sm()
        .text_color(Palette::muted_text())
        .border_1()
        .border_color(Palette::border())
        .rounded_md()
        .child(label)
}

fn tick_button(is_ticked: bool, id: StepId, cx: &mut Context<OneStepLayout>) -> impl IntoElement {
    div()
        .id("tick")
        .px_3()
        .py_1()
        .rounded_md()
        .border_1()
        .border_color(Palette::border())
        .cursor_pointer()
        .text_sm()
        .on_click(cx.listener(move |this, _, _, cx| this.toggle_tick(&id, cx)))
        .child(if is_ticked {
            "✓ Read"
        } else {
            "Mark as read"
        })
}

impl Focusable for OneStepLayout {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for OneStepLayout {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let key_context = if self.composing.is_some() {
            format!("{} {}", Self::KEY_CONTEXT, CommentComposer::KEY_CONTEXT)
        } else {
            Self::KEY_CONTEXT.to_string()
        };
        let frame_painted_marker = self.frame_stats.as_mut().map(FrameStats::frame_started);
        self.run_scroll_script(window, cx);
        self.show_diagram(window, cx);
        let selected = Rc::new(
            self.review
                .row_selection()
                .and_then(|selection| self.rows.selected_rows(selection)),
        );
        let screen = div().flex_1().px_6().pt_6().child(
            list(
                self.screen_list.clone(),
                cx.processor(move |this, index, _window, cx| {
                    this.render_row(index, selected.as_ref().as_ref(), cx)
                }),
            )
            .size_full(),
        );
        div()
            .key_context(key_context.as_str())
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_comment_key))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| this.end_row_selection(window, cx)),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| this.end_row_selection(window, cx)),
            )
            .on_action(cx.listener(Self::go_forward))
            .on_action(cx.listener(Self::go_back))
            .on_action(cx.listener(Self::toggle_jump))
            .size_full()
            .flex()
            .bg(Palette::background())
            .font_family(TEXT_FONT)
            .text_color(Palette::text())
            .child(self.sidebar(cx))
            .child(
                div()
                    .flex_1()
                    .h_full()
                    .flex()
                    .flex_col()
                    .children(self.old_head.clone().map(|old_head| {
                        NewCommitsBanner::new(
                            old_head,
                            self.tour_update.clone(),
                            cx.listener(|this, _, _, cx| this.update_tour(cx)),
                        )
                    }))
                    .children(self.lines_to_ask.clone().map(|place| {
                        let label = match &place {
                            Place::Lines { start, end, .. } => lines_label(start, end),
                            _ => String::new(),
                        };
                        div().px_6().py_1().flex().child(self.ask_button(
                            "ask-selected-lines".into(),
                            place,
                            label,
                            cx,
                        ))
                    }))
                    .children(self.selection_notice.clone().map(|notice| {
                        div()
                            .px_6()
                            .py_1()
                            .text_sm()
                            .bg(Palette::current())
                            .child(notice)
                    }))
                    .children(
                        Some(self.refresh_brief.clone())
                            .filter(|brief| !brief.is_empty() && self.old_head.is_none())
                            .map(|brief| {
                                RefreshBanner::new(
                                    brief,
                                    self.tour_refresh.clone(),
                                    cx.listener(|this, _, _, cx| this.refresh_tour(cx)),
                                )
                            }),
                    )
                    .child(screen),
            )
            .when(self.ask.read(cx).is_open(), |row| {
                row.child(self.ask.clone())
            })
            .child(self.jump.clone())
            .children(frame_painted_marker)
    }
}

fn run_launch(launch: &IdeLaunch, cx: &mut App) {
    match launch {
        IdeLaunch::Url(url) => cx.open_url(url),
        IdeLaunch::Shell(command) => {
            if let Err(error) = std::process::Command::new("sh")
                .args(["-c", command])
                .spawn()
            {
                eprintln!("could not run the IDE command: {error}");
            }
        }
    }
}
