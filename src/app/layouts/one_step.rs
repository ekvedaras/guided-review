use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, App, AppContext, Context, Div, Entity, FocusHandle, Focusable, InteractiveElement,
    IntoElement, KeyDownEvent, MouseButton, ParentElement, Render, SharedString, Stateful,
    StatefulInteractiveElement, Styled, Subscription, Task, Window, actions, div, px,
};

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use crate::app::components::ask_panel::{AskPanel, AskSetup, ask_button};
use crate::app::components::comment_composer::{CommentComposer, OnCompose, QueuedCommentNote};
use crate::app::components::diagram::{DiagramView, Diagrams};
use crate::app::components::diff_view::{
    DiffView, OnMoreContext, OnOpenInIde, RowGesture, RowSelecting, threads_by_piece,
};
use crate::app::components::finish_review::{FinishReviewDialog, OnBodyEdit, OnFinish};
use crate::app::components::fold_toggle::FoldToggle;
use crate::app::components::jump_palette::{JumpEvent, JumpPalette};
use crate::app::components::new_commits_banner::{NewCommitsBanner, TourUpdate};
use crate::app::components::palette::{Palette, TEXT_FONT};
use crate::app::components::refresh_banner::RefreshBanner;
use crate::app::components::safety_net_view::SafetyNetView;
use crate::app::components::step_notes::StepNotes;
use crate::app::components::text_input::TextDraft;
use crate::app::components::thread_view::{
    OnReplyDraft, OnThreadAction, ReplyDraftChange, ThreadElement,
};
use crate::code_source::CodeSource;
use crate::ide::{Ide, IdeLaunch};
use crate::viewer::{
    CommentPlace, CommentResult, FinishResult, GitHubPort, LineLocation, MoreContext, Place,
    Review, Screen, SendMode, StepId, StepPage, ThreadView, TourRefresher, TourUpdater,
    UpdatedTour,
};

const PUSH_DEBOUNCE: Duration = Duration::from_secs(1);

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
pub struct OneStepLayout {
    review: Review,
    github: Arc<dyn GitHubPort>,
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
    is_retrying_comments: bool,
    is_syncing: bool,
    sync_again: bool,
    _focus_subscription: Subscription,
    jump: Entity<JumpPalette>,
    /// `None` hides every Open in IDE button.
    ide: Option<Ide>,
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
                cx.notify();
            }
        })
        .detach();
        let Collaborators {
            github,
            ide,
            tour_work,
        } = collaborators;
        let mut layout = Self {
            review,
            github,
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
            is_retrying_comments: false,
            is_syncing: false,
            sync_again: false,
            _focus_subscription: focus_subscription,
        };
        layout.start_background_work(source, cx);
        layout
    }

    fn new_diagrams(cx: &mut Context<Self>) -> Entity<Diagrams> {
        let diagrams = cx.new(|_| Diagrams::default());
        cx.observe(&diagrams, |_, _, cx| cx.notify()).detach();
        diagrams
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
                cx.notify();
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
        if let Screen::Step(id) = screen {
            self.review.go_to(&id);
        }
        self.diagrams = Self::new_diagrams(cx);
        self.start_background_work(updated.source, cx);
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
                        cx.notify();
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
                cx.notify();
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
                        cx.notify();
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
                cx.notify();
            });
        }));
    }

    fn row_selecting(&self, cx: &mut Context<Self>) -> RowSelecting {
        let layout = cx.entity();
        RowSelecting {
            selected: self.review.selected_lines(),
            on_gesture: Rc::new(move |gesture, at, _window, cx| {
                layout.update(cx, |this, cx| this.on_row_gesture(gesture, at, cx))
            }),
        }
    }

    fn on_row_gesture(&mut self, gesture: RowGesture, at: &LineLocation, cx: &mut Context<Self>) {
        match gesture {
            RowGesture::Press => {
                self.selection_notice = None;
                self.review.begin_selection(at.clone());
            }
            RowGesture::Over if self.review.is_selecting() => {
                self.review.extend_selection(at.clone());
            }
            RowGesture::Over => return,
        }
        cx.notify();
    }

    fn end_row_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.review.is_selecting() {
            return;
        }
        match self.review.finish_selection() {
            Ok(Some(place)) => {
                self.composing = Some(place);
                window.focus(&self.focus, cx);
            }
            Ok(None) => {}
            Err(refusal) => self.selection_notice = Some(refusal.to_string()),
        }
        cx.notify();
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
        cx.notify();
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
                cx.notify();
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
                cx.notify();
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
        cx.notify();
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

    /// Open comment boxes and comments waiting for GitHub, each with the row it hangs under.
    fn comment_elements(&self, cx: &mut Context<Self>) -> Vec<(LineLocation, AnyElement)> {
        let queued = self
            .review
            .queued_comments()
            .iter()
            .map(|queued| {
                let place = queued.draft.place.clone();
                let on_discard = self.on_compose(&place, cx, |this, place, _, cx| {
                    this.discard_comment(&place, cx)
                });
                (
                    place.end,
                    QueuedCommentNote::new(queued.clone(), on_discard).into_any_element(),
                )
            })
            .collect::<Vec<_>>();
        let old_head_problem = self
            .review
            .old_head()
            .map(|_| crate::viewer::CommentRefusal::TourIsOld.to_string());
        let can_comment_now = self.review.can_comment_now();
        let composers = self.review.comment_drafts().iter().map(|draft| {
            let place = &draft.place;
            let is_focused = self.composing.as_ref() == Some(place);
            let problem = self
                .comment_problems
                .get(place)
                .cloned()
                .or(old_head_problem.clone());
            let composer = CommentComposer::new(
                draft.clone(),
                is_focused,
                self.sending_comments.contains(place),
                can_comment_now,
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
                    cx.notify();
                }),
                self.on_compose(place, cx, |this, place, _, cx| {
                    this.discard_comment(&place, cx)
                }),
            );
            (place.end.clone(), composer.into_any_element())
        });
        queued.into_iter().chain(composers).collect()
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
                        cx.notify();
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
                cx.notify();
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
                        cx.notify();
                        result
                    });
                    on_done(&result, cx);
                });
            })
            .detach();
        })
    }

    /// Threads under a line come with their line, the rest go after the code.
    fn thread_elements(
        &self,
        views: &[ThreadView],
        cx: &mut Context<Self>,
    ) -> (Vec<(LineLocation, AnyElement)>, Vec<AnyElement>) {
        let mut under_lines = Vec::new();
        let mut after_code = Vec::new();
        for view in views {
            let id = view.thread.id.clone();
            let element = ThreadElement::new(
                view.clone(),
                self.review.can_comment_now(),
                self.review.reply_draft(&view.thread.id).map(str::to_string),
                self.on_reply_draft(&view.thread.id, cx),
                cx.listener(move |this, _, _, cx| {
                    this.review.toggle_thread_fold(&id);
                    cx.notify();
                }),
                self.on_thread_action(cx),
            )
            .into_any_element();
            match &view.anchor {
                Some(location) => under_lines.push((location.clone(), element)),
                None => after_code.push(element),
            }
        }
        (under_lines, after_code)
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
            cx.notify();
        }
    }

    fn go_back(&mut self, _: &GoBack, _: &mut Window, cx: &mut Context<Self>) {
        if self.review.go_back() {
            cx.notify();
        }
    }

    fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let summary_row = div()
            .id("summary")
            .px_3()
            .py_1()
            .cursor_pointer()
            .when_current(self.review.screen() == &Screen::Summary)
            .on_click(cx.listener(|this, _, _, cx| {
                this.review.go_to_summary();
                cx.notify();
            }))
            .child("Summary");
        let progress = self.review.progress();
        let progress_row = div()
            .px_3()
            .pb_2()
            .text_color(Palette::muted_text())
            .child(format!("Read {} of {}", progress.read, progress.total));
        let sync_row = self.review.sync_banner().map(|reason| {
            div()
                .px_3()
                .pb_2()
                .text_color(Palette::muted_text())
                .child(format!("Not syncing with GitHub: {reason}"))
        });
        let not_synced_row = self.review.not_synced().then(|| {
            div()
                .px_3()
                .pb_2()
                .text_color(Palette::muted_text())
                .child("● not synced with GitHub yet")
        });
        let finish_dialog = FinishReviewDialog::new(
            self.review.can_approve_or_request_changes(),
            self.review.approve_warning(),
            self.review.finish_body().to_string(),
            self.on_finish_body_edit(cx),
            self.on_finish_review(cx),
        );
        let step_rows = self.review.sidebar().into_iter().map(|entry| {
            let id: StepId = entry.id.clone();
            div()
                .id(SharedString::from(format!("step-{id}")))
                .pl(px(12. + 16. * entry.depth as f32))
                .pr_3()
                .py_1()
                .cursor_pointer()
                .when_current(entry.is_current)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.review.go_to(&id);
                    cx.notify();
                }))
                .when_ticked(entry.is_ticked || entry.is_gone)
                .child(match entry.thread_count {
                    0 => entry.title.to_string(),
                    count => format!("{} ({count} 💬)", entry.title),
                })
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
            .children(self.safety_net_row(cx))
    }

    fn safety_net_row(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let page = self.review.safety_net()?;
        Some(
            div()
                .id("safety-net")
                .px_3()
                .py_1()
                .cursor_pointer()
                .when_current(self.review.screen() == &Screen::SafetyNet)
                .on_click(cx.listener(|this, _, _, cx| {
                    this.review.go_to_safety_net();
                    cx.notify();
                }))
                .child(match page.threads.len() {
                    0 => format!("{} ({})", SafetyNetView::TITLE, page.pieces.len()),
                    threads => format!(
                        "{} ({}, {threads} 💬)",
                        SafetyNetView::TITLE,
                        page.pieces.len()
                    ),
                }),
        )
    }

    fn screen(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        self.show_diagram(window, cx);
        match self.review.screen() {
            Screen::Summary => {
                let button =
                    self.ask_button("ask-tour".into(), Place::Tour, "the whole tour".into(), cx);
                summary_screen(self.review.title(), self.review.summary(), button)
                    .into_any_element()
            }
            Screen::Step(_) => match self.review.current_step() {
                Some(page) => self.step_screen(page, cx).into_any_element(),
                None => div().into_any_element(),
            },
            Screen::SafetyNet => match self.review.safety_net() {
                Some(page) => {
                    let (mut under_lines, after_code) = self.thread_elements(&page.threads, cx);
                    under_lines.extend(self.comment_elements(cx));
                    SafetyNetView::new(page, self.on_more_context(cx))
                        .with_row_selecting(self.row_selecting(cx))
                        .with_threads(under_lines, after_code)
                        .with_open_in_ide(self.open_line_in_ide())
                        .into_any_element()
                }
                None => div().into_any_element(),
            },
        }
    }

    fn show_diagram(&self, window: &mut Window, cx: &mut Context<Self>) {
        let page = match self.review.screen() {
            Screen::Step(_) => self.review.current_step(),
            _ => None,
        };
        let diagram = page
            .as_ref()
            .filter(|page| !page.is_folded)
            .and_then(|page| page.step.diagram.as_deref());
        self.diagrams
            .update(cx, |diagrams, cx| diagrams.show(diagram, window, cx));
    }

    fn step_screen(&self, page: StepPage<'_>, cx: &mut Context<Self>) -> impl IntoElement {
        let step_id = StepId::from(page.step.id.as_str());
        let step_button = self.ask_button(
            format!("ask-step-{step_id}"),
            Place::Step {
                step: step_id.clone(),
            },
            page.step.title.clone(),
            cx,
        );
        let on_more_context = self.on_more_context(cx);
        let (mut under_lines, after_code) = self.thread_elements(&page.threads, cx);
        under_lines.extend(self.comment_elements(cx));
        let selecting = self.row_selecting(cx);
        let mut threads_of_piece = threads_by_piece(&page.pieces, under_lines).into_iter();
        let shows_change_since_read = page
            .since_read
            .as_ref()
            .is_some_and(|since| !since.showing_full_diff);
        let pieces: Vec<_> = page
            .pieces
            .iter()
            .map(|piece| {
                let threads = threads_of_piece.next().unwrap_or_default();
                let place = Place::Piece {
                    step: step_id.clone(),
                    piece: piece.id.to_string(),
                };
                let button = self.ask_button(
                    format!("ask-piece-{step_id}-{}", piece.id),
                    place,
                    piece.id.to_string(),
                    cx,
                );
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .gap_2()
                            .children(self.ide_button(
                                format!("ide-piece-{step_id}-{}", piece.id),
                                piece.open_at(),
                                cx,
                            ))
                            .child(button),
                    )
                    .child(
                        DiffView::new(piece.clone(), on_more_context.clone())
                            .with_row_selecting(selecting.clone())
                            .with_threads(threads)
                            .with_open_in_ide(self.open_line_in_ide())
                            .in_change_since_read(shows_change_since_read),
                    )
            })
            .collect();
        let diagram = (!page.is_folded && page.step.diagram.is_some())
            .then(|| DiagramView::new(self.diagrams.clone()));
        step_screen(
            page,
            step_button,
            diagram,
            pieces,
            self.review.ignores_whitespace(),
            after_code,
            cx,
        )
    }

    fn on_more_context(&self, cx: &mut Context<Self>) -> OnMoreContext {
        Rc::new(cx.listener(|this, more: &MoreContext, _, cx| {
            this.review.show_more_context(more);
            cx.notify();
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
        cx.notify();
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

fn summary_screen(title: &str, summary: &[String], ask: impl IntoElement) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap_3()
        .child(div().text_2xl().child(title.to_string()))
        .child(div().flex().child(ask))
        .children(
            summary
                .iter()
                .map(|bullet| div().child(format!("• {bullet}"))),
        )
}

fn step_screen(
    page: StepPage<'_>,
    ask: impl IntoElement,
    diagram: Option<DiagramView>,
    pieces: Vec<impl IntoElement>,
    ignores_whitespace: bool,
    after_code: Vec<AnyElement>,
    cx: &mut Context<OneStepLayout>,
) -> impl IntoElement {
    let step = page.step;
    let id = StepId::from(step.id.as_str());
    let fold_toggle = (page.is_folded || step.skim || page.is_ticked).then(|| {
        FoldToggle::new(
            page.is_folded,
            page.pieces.len(),
            cx.listener(move |this, _, _, cx| {
                this.review.toggle_fold(&id);
                cx.notify();
            }),
        )
    });
    let (pieces, after_code) = if page.is_folded {
        (Vec::new(), Vec::new())
    } else {
        (pieces, after_code)
    };
    let full_diff_toggle = page.since_read.as_ref().map(|since| {
        let id = StepId::from(step.id.as_str());
        let label = if since.showing_full_diff {
            "Show only the change since your read"
        } else {
            "Show the full diff"
        };
        div()
            .id("full-diff-toggle")
            .text_sm()
            .cursor_pointer()
            .text_color(Palette::muted_text())
            .on_click(cx.listener(move |this, _, _, cx| {
                this.review.toggle_full_diff(&id);
                cx.notify();
            }))
            .child(label)
    });
    let repin_notes = (page.gone_pieces > 0 || page.text_may_be_old).then(|| {
        div()
            .flex()
            .gap_3()
            .text_sm()
            .text_color(Palette::muted_text())
            .when(page.gone_pieces > 0, |row| {
                row.child(match page.gone_pieces {
                    1 => "1 piece gone since last version".to_string(),
                    count => format!("{count} pieces gone since last version"),
                })
            })
            .when(page.text_may_be_old, |row| row.child("Text may be old"))
    });
    div()
        .flex()
        .flex_col()
        .gap_3()
        .when(page.is_gone, |column| {
            column.text_color(Palette::muted_text())
        })
        .child(step_header(&page, cx))
        .child(
            div()
                .text_color(Palette::muted_text())
                .child(step.why.clone()),
        )
        .children(diagram)
        .child(div().flex().child(ask))
        .child(StepNotes::of(step))
        .child(whitespace_toggle(ignores_whitespace, cx))
        .children(repin_notes)
        .children(full_diff_toggle)
        .children(fold_toggle)
        .children(pieces)
        .children(after_code)
}

fn step_header(page: &StepPage<'_>, cx: &mut Context<OneStepLayout>) -> impl IntoElement {
    let id = StepId::from(page.step.id.as_str());
    let title = div()
        .id("step-title")
        .flex_1()
        .text_xl()
        .child(page.step.title.clone());
    let title = if page.is_ticked || page.step.skim {
        let id = id.clone();
        title
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, _, cx| {
                this.review.toggle_fold(&id);
                cx.notify();
            }))
    } else {
        title
    };
    div()
        .flex()
        .items_center()
        .gap_3()
        .child(title)
        .child(tick_button(page.is_ticked, id, cx))
}

fn whitespace_toggle(is_on: bool, cx: &mut Context<OneStepLayout>) -> impl IntoElement {
    div()
        .id("ignore-whitespace")
        .self_start()
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
            cx.notify();
        }))
        .child("Hide whitespace")
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
                    .children(self.review.old_head().map(|old_head| {
                        NewCommitsBanner::new(
                            old_head,
                            self.tour_update.clone(),
                            cx.listener(|this, _, _, cx| this.update_tour(cx)),
                        )
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
                        Some(self.review.refresh_brief())
                            .filter(|brief| !brief.is_empty() && self.review.old_head().is_none())
                            .map(|brief| {
                                RefreshBanner::new(
                                    brief,
                                    self.tour_refresh.clone(),
                                    cx.listener(|this, _, _, cx| this.refresh_tour(cx)),
                                )
                            }),
                    )
                    .child(
                        div()
                            .id("screen")
                            .flex_1()
                            .overflow_y_scroll()
                            .p_6()
                            .child(self.screen(window, cx)),
                    ),
            )
            .when(self.ask.read(cx).is_open(), |row| {
                row.child(self.ask.clone())
            })
            .child(self.jump.clone())
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
