use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, App, AppContext, Context, Div, Entity, FocusHandle, Focusable, InteractiveElement,
    IntoElement, KeyDownEvent, ListOffset, ListState, MouseButton, ParentElement, Render,
    SharedString, Stateful, StatefulInteractiveElement, Styled, Subscription, Window, actions, div,
    list, px,
};

use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

use crate::app::components::ask_panel::{AskPanel, AskSetup, UseAsComment, ask_button};
use crate::app::components::comment_composer::{
    CommentComposer, ComposerKey, OnCompose, QueuedCommentNote,
};
use crate::app::components::diagram::{DiagramView, Diagrams};
use crate::app::components::diff_view::{
    DiffRow, OnImageComment, OnMoreContext, OnOpenInIde, OnRowGesture, PieceHeader, RowGesture,
    binary_file_row, hunk_gap, no_change_since_read_row, only_whitespace_row, piece_end, piece_row,
};
use crate::app::components::finish_review::{FinishReviewDialog, OnBodyEdit, OnFinish};
use crate::app::components::fold_toggle::FoldToggle;
use crate::app::components::image_row::ImageRow;
use crate::app::components::jump_palette::{JumpEvent, JumpPalette};
use crate::app::components::new_commits_banner::NewCommitsBanner;
use crate::app::components::palette::{Colors, TEXT_FONT};
use crate::app::components::refresh_banner::RefreshBanner;
use crate::app::components::safety_net_view::SafetyNetIntro;
use crate::app::components::thread_view::{
    OnReplyDraft, OnThreadAction, ReplyDraftChange, ThreadElement,
};
use crate::app::frame_stats::{FrameStatsHook, ScriptMove};
use crate::app::page_rows::{AskAt, PageRow, SelectedRows, StepHeading};
use crate::app::review_session::{AskedLinesOf, ReviewSession, SessionEvent, TourWork};
use crate::code_source::CodeSource;
use crate::ide::Ide;
use crate::viewer::{
    Ask, CommentPlace, GitHubPort, LineLocation, MoreContext, Place, QueuedComment, Review, Screen,
    SendMode, StepId, ThreadView,
};

actions!(one_step, [GoForward, GoBack, ToggleJump]);

/// What the layout talks to outside the window.
pub struct Collaborators {
    pub github: Arc<dyn GitHubPort>,
    /// `None` hides every Open in IDE button.
    pub ide: Option<Ide>,
    pub tour_work: TourWork,
}

/// The one layout: step tree on the left, one screen on the right (the summary, or one step).
///
/// It draws from the `ReviewSession` and forwards the reader's input to it. Looks-only state (the drag
/// selection, hover, ask counts) is read when drawn.
pub struct OneStepLayout {
    session: Entity<ReviewSession>,
    /// The session's list of the screen's rows; the layout scrolls it.
    screen_list: ListState,
    on_row_gesture: OnRowGesture,
    diagrams: Entity<Diagrams>,
    focus: FocusHandle,
    ask: Entity<AskPanel>,
    /// Why the last drag opened no comment box.
    selection_notice: Option<String>,
    /// The rows of the last drag, to ask the AI about.
    lines_to_ask: Option<Place>,
    frame_stats: FrameStatsHook,
    jump: Entity<JumpPalette>,
    /// `None` hides every Open in IDE button.
    ide: Option<Ide>,
    _session_subscriptions: [Subscription; 2],
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
        let Collaborators {
            github,
            ide,
            tour_work,
        } = collaborators;
        let focus = cx.focus_handle();
        let ask = cx.new(|cx| AskPanel::new(ask, focus.clone(), cx));
        let frame_stats = FrameStatsHook::from_env();
        let asks = ask.clone();
        let asked_lines_of: AskedLinesOf =
            Box::new(move |screen, cx| asks.read(cx).asked_lines_on(screen));
        let session = cx.new(|cx| {
            ReviewSession::new(
                review,
                source,
                github,
                tour_work,
                asked_lines_of,
                frame_stats.clone(),
                window,
                cx,
            )
        });
        let session_subscriptions = [
            cx.observe(&session, |_, _, cx| cx.notify()),
            cx.subscribe(&session, |this, _, SessionEvent::Reopened, cx| {
                this.reopened(cx)
            }),
        ];
        let diagrams = Self::new_diagrams(cx);
        let jump = cx.new(|cx| JumpPalette::new(focus.clone(), cx));
        cx.subscribe(&jump, |this, palette, event: &JumpEvent, cx| match event {
            JumpEvent::QueryChanged(query) => {
                let targets = this.session.read(cx).review().jump_targets(query);
                palette.update(cx, |palette, _| palette.set_targets(targets));
            }
            JumpEvent::Picked(target) => {
                let row = this.session.update(cx, |session, cx| {
                    session.change(cx, |review| review.jump_to(target));
                    session.rows().row_of_jump(target)
                });
                this.screen_list.scroll_to(ListOffset {
                    item_ix: row,
                    offset_in_item: px(0.),
                });
            }
        })
        .detach();
        let use_as_comment_subscription =
            cx.subscribe_in(&ask, window, |this, _, UseAsComment(ask), window, cx| {
                this.use_as_comment(ask, window, cx)
            });
        let asked_lines_subscription = cx.observe(&ask, |this, _, cx| {
            this.session
                .update(cx, |session, cx| session.asks_changed(cx))
        });
        let layout_handle = cx.weak_entity();
        let on_row_gesture: OnRowGesture = Rc::new(move |gesture, at, _window, cx| {
            layout_handle
                .update(cx, |this, cx| this.on_row_gesture(gesture, at, cx))
                .ok();
        });
        cx.on_app_quit(|this, _| {
            this.frame_stats.report_on_quit();
            async {}
        })
        .detach();
        Self {
            screen_list: session.read(cx).screen_list().clone(),
            session,
            on_row_gesture,
            diagrams,
            focus,
            ask,
            selection_notice: None,
            lines_to_ask: None,
            frame_stats,
            jump,
            ide,
            _session_subscriptions: session_subscriptions,
            _use_as_comment_subscription: use_as_comment_subscription,
            _asked_lines_subscription: asked_lines_subscription,
        }
    }

    fn new_diagrams(cx: &mut Context<Self>) -> Entity<Diagrams> {
        let diagrams = cx.new(|_| Diagrams::default());
        cx.observe(&diagrams, |this, _, cx| {
            if let Some(row) = this.session.read(cx).rows().diagram_row() {
                this.screen_list.remeasure_items(row..row + 1);
            }
            cx.notify();
        })
        .detach();
        diagrams
    }

    fn change_review<T>(&self, cx: &mut App, change: impl FnOnce(&mut Review) -> T) -> T {
        self.session
            .update(cx, |session, cx| session.change(cx, change))
    }

    /// The re-pin or Refresh opened a new review: diagrams and notes on old answers follow it.
    fn reopened(&mut self, cx: &mut Context<Self>) {
        let ask = self.ask.clone();
        self.session.update(cx, |session, cx| {
            ask.update(cx, |panel, cx| {
                panel.recheck_code_changes(session.review(), cx)
            })
        });
        self.diagrams = Self::new_diagrams(cx);
        cx.notify();
    }

    /// Folded steps are opened, so the script scrolls through their pieces too.
    fn run_scroll_script(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(next) = self
            .frame_stats
            .scroll_script_frame(&self.screen_list, window)
        else {
            return;
        };
        if let ScriptMove::ScreenDone(report) = next {
            let active = if window.is_window_active() {
                "active"
            } else {
                "INACTIVE window"
            };
            let screen = self.session.read(cx).review().screen().clone();
            eprintln!("{screen:?} ({active}): {report}");
            let has_next = self.change_review(cx, |review| {
                if !review.go_forward() {
                    return false;
                }
                if let Screen::Step(id) = review.screen().clone()
                    && review.is_folded(&id)
                {
                    review.toggle_fold(&id);
                }
                true
            });
            if !has_next {
                cx.quit();
                return;
            }
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
        ask_button(Colors::active(cx), SharedString::from(key), count).on_click(cx.listener(
            move |this, _, window, cx| {
                let ask = this.ask.clone();
                this.session.update(cx, |session, cx| {
                    ask.update(cx, |panel, cx| {
                        panel.open(session.review(), place.clone(), label.clone(), window, cx)
                    })
                });
            },
        ))
    }

    fn on_row_gesture(&mut self, gesture: RowGesture, at: &LineLocation, cx: &mut Context<Self>) {
        if gesture == RowGesture::Press {
            self.selection_notice = None;
            self.lines_to_ask = None;
        }
        self.session.update(cx, |session, cx| match gesture {
            RowGesture::Press => session.select_from(at.clone(), cx),
            RowGesture::Over => session.extend_selection(at.clone(), cx),
        });
    }

    fn end_row_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let ended = self.session.update(cx, |session, cx| {
            let review = session.review();
            if !review.is_selecting() {
                return None;
            }
            let lines_to_ask = review
                .selected_lines()
                .map(|selected| review.place_of_lines(&selected));
            let finished = session.change(cx, Review::finish_selection);
            if let Ok(Some(place)) = &finished {
                session.compose(place.clone(), cx);
            }
            Some((lines_to_ask, finished))
        });
        let Some((lines_to_ask, finished)) = ended else {
            return;
        };
        self.lines_to_ask = lines_to_ask;
        match finished {
            Ok(Some(_)) => window.focus(&self.focus, cx),
            Ok(None) => {}
            Err(refusal) => self.selection_notice = Some(refusal.to_string()),
        }
        cx.notify();
    }

    fn begin_file_comment(&mut self, path: &str, window: &mut Window, cx: &mut Context<Self>) {
        let begun = self.session.update(cx, |session, cx| {
            let begun = session.change(cx, |review| review.begin_file_comment(path));
            if let Ok(place) = &begun {
                session.compose(place.clone(), cx);
            }
            begun
        });
        match begun {
            Ok(_) => window.focus(&self.focus, cx),
            Err(refusal) => self.selection_notice = Some(refusal.to_string()),
        }
        cx.notify();
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
        let session = self.session.read(cx);
        ImageRow::new(
            path.to_string(),
            session.review().image_preview(path).cloned(),
            session.image_pictures(path),
            on_comment,
        )
    }

    fn on_comment_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.focus.is_focused(window) {
            return;
        }
        let session = self.session.read(cx);
        let Some(place) = session.composing().cloned() else {
            return;
        };
        let Some(draft) = session
            .review()
            .comment_drafts()
            .iter()
            .find(|d| d.place == place)
        else {
            return;
        };
        match CommentComposer::on_key(&draft.body, &event.keystroke) {
            ComposerKey::Stop => self
                .session
                .update(cx, |session, cx| session.stop_composing(cx)),
            ComposerKey::Send => self.session.update(cx, |session, cx| {
                session.send_comment(place, SendMode::AddToReview, cx)
            }),
            ComposerKey::Typed(body) => self
                .session
                .update(cx, |session, cx| session.type_in_comment(&place, &body, cx)),
            ComposerKey::Ignored => {}
        }
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
        let used = self.session.update(cx, |session, cx| {
            let used = session.change(cx, |review| review.use_as_comment(ask));
            if let Ok(place) = &used {
                session.compose(place.clone(), cx);
            }
            used
        });
        match used {
            Ok(_) => {
                self.selection_notice = None;
                window.focus(&self.focus, cx);
            }
            Err(refusal) => self.selection_notice = Some(refusal.to_string()),
        }
        cx.notify();
    }

    fn queued_comment(&self, queued: &QueuedComment, cx: &mut Context<Self>) -> QueuedCommentNote {
        let on_discard = self.on_compose(&queued.draft.place, cx, |this, place, _, cx| {
            this.session
                .update(cx, |session, cx| session.discard_comment(&place, cx))
        });
        QueuedCommentNote::new(queued.clone(), on_discard)
    }

    /// The draft is read here, so typing redraws the box without new rows.
    fn composer(&self, place: &CommentPlace, cx: &mut Context<Self>) -> Option<CommentComposer> {
        let session = self.session.read(cx);
        let review = session.review();
        let draft = review
            .comment_drafts()
            .iter()
            .find(|draft| &draft.place == place)?
            .clone();
        let is_composing = session.composing() == Some(place);
        let is_sending = session.is_sending(place);
        let can_comment_now = review.can_comment_now();
        let can_suggest_change = review.can_suggest_change(place);
        let problem = session.comment_problem(place);
        Some(CommentComposer::new(
            draft,
            is_composing,
            is_sending,
            can_comment_now,
            can_suggest_change,
            problem,
            self.on_compose(place, cx, |this, place, window, cx| {
                this.session
                    .update(cx, |session, cx| session.compose(place, cx));
                window.focus(&this.focus, cx);
            }),
            self.on_compose(place, cx, |this, place, _, cx| {
                this.session.update(cx, |session, cx| {
                    session.send_comment(place, SendMode::AddToReview, cx)
                })
            }),
            self.on_compose(place, cx, |this, place, _, cx| {
                this.session.update(cx, |session, cx| {
                    session.send_comment(place, SendMode::CommentNow, cx)
                })
            }),
            self.on_compose(place, cx, |this, place, _, cx| {
                if let Err(error) = this.change_review(cx, |review| review.suggest_change(&place)) {
                    eprintln!("could not save the suggestion: {error:#}");
                }
            }),
            self.on_compose(place, cx, |this, place, _, cx| {
                this.session
                    .update(cx, |session, cx| session.discard_comment(&place, cx))
            }),
        ))
    }

    fn on_thread_action(&self) -> OnThreadAction {
        let session = self.session.clone();
        Rc::new(move |action, on_done, cx| {
            session.update(cx, |session, cx| session.act_on_thread(action, on_done, cx))
        })
    }

    /// Reply text goes to the core, which saves it, so it survives scrolling and restarts.
    fn on_reply_draft(&self, thread_id: &str) -> OnReplyDraft {
        let (session, thread_id) = (self.session.clone(), thread_id.to_string());
        Rc::new(move |change, cx| {
            session.update(cx, |session, cx| {
                let saved = match change {
                    ReplyDraftChange::Edit(text) => {
                        return session.type_in_reply(&thread_id, &text, cx);
                    }
                    ReplyDraftChange::Open => {
                        session.change(cx, |review| review.open_reply(&thread_id))
                    }
                    ReplyDraftChange::Discard => {
                        session.change(cx, |review| review.discard_reply(&thread_id))
                    }
                };
                if let Err(error) = saved {
                    eprintln!("could not save the reply text: {error:#}");
                }
            });
        })
    }

    fn on_finish_body_edit(&self) -> OnBodyEdit {
        let session = self.session.clone();
        Rc::new(move |text, cx| {
            session.update(cx, |session, cx| session.type_in_finish_body(&text, cx));
        })
    }

    fn on_finish_review(&self) -> OnFinish {
        let session = self.session.clone();
        Rc::new(move |verdict, body, on_done, cx| {
            session.update(cx, |session, cx| {
                session.finish_review(verdict, &body, on_done, cx)
            })
        })
    }

    fn thread_element(&self, view: &ThreadView, cx: &mut Context<Self>) -> ThreadElement {
        let id = view.thread.id.clone();
        let review = self.session.read(cx).review();
        let can_comment_now = review.can_comment_now();
        let reply = review.reply_draft(&view.thread.id).map(str::to_string);
        ThreadElement::new(
            view.clone(),
            can_comment_now,
            reply,
            self.on_reply_draft(&view.thread.id),
            cx.listener(move |this, _, _, cx| {
                this.change_review(cx, |review| review.toggle_thread_fold(&id));
            }),
            self.on_thread_action(),
        )
    }

    fn toggle_jump(&mut self, _: &ToggleJump, window: &mut Window, cx: &mut Context<Self>) {
        let targets = self.session.read(cx).review().jump_targets("");
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
            tool_button(Colors::active(cx), key, "Open in IDE")
                .on_click(cx.listener(move |_, _, _, cx| launch.run(|url| cx.open_url(url)))),
        )
    }

    fn open_line_in_ide(&self) -> Option<OnOpenInIde> {
        let ide = self.ide.clone()?;
        Some(Rc::new(move |at: &LineLocation, cx: &mut App| {
            ide.launch(Path::new(&at.path), at.line)
                .run(|url| cx.open_url(url))
        }))
    }

    fn go_forward(&mut self, _: &GoForward, _: &mut Window, cx: &mut Context<Self>) {
        self.change_review(cx, Review::go_forward);
    }

    fn go_back(&mut self, _: &GoBack, _: &mut Window, cx: &mut Context<Self>) {
        self.change_review(cx, Review::go_back);
    }

    fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = Colors::active(cx);
        let session = self.session.read(cx);
        let sidebar = session.sidebar();
        let summary_row = div()
            .id("summary")
            .px_3()
            .py_1()
            .cursor_pointer()
            .when_current(sidebar.is_summary_current, colors)
            .on_click(cx.listener(|this, _, _, cx| {
                this.change_review(cx, Review::go_to_summary);
            }))
            .child("Summary");
        let progress_row = div()
            .px_3()
            .pb_2()
            .text_color(colors.muted_text)
            .child(sidebar.progress.clone());
        let sync_row = sidebar.sync_banner.clone().map(|banner| {
            div()
                .px_3()
                .pb_2()
                .text_color(colors.muted_text)
                .child(banner)
        });
        let not_synced_row = sidebar.not_synced.then(|| {
            div()
                .px_3()
                .pb_2()
                .text_color(colors.muted_text)
                .child("● not synced with GitHub yet")
        });
        let finish_dialog = FinishReviewDialog::new(
            sidebar.can_judge,
            sidebar.approve_warning,
            session.review().finish_body().to_string(),
            self.on_finish_body_edit(),
            self.on_finish_review(),
        );
        let step_rows = sidebar.steps.iter().map(|step| {
            let id: StepId = step.id.clone();
            div()
                .id(SharedString::from(format!("step-{id}")))
                .pl(px(12. + 16. * step.depth as f32))
                .pr_3()
                .py_1()
                .cursor_pointer()
                .when_current(step.is_current, colors)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.change_review(cx, |review| review.go_to(&id));
                }))
                .when_ticked(step.is_ticked || step.is_gone, colors)
                .child(step.label.clone())
        });
        let safety_net_row = sidebar.safety_net.as_ref().map(|safety_net| {
            div()
                .id("safety-net")
                .px_3()
                .py_1()
                .cursor_pointer()
                .when_current(safety_net.is_current, colors)
                .on_click(cx.listener(|this, _, _, cx| {
                    this.change_review(cx, Review::go_to_safety_net);
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
            .bg(colors.sidebar)
            .border_r_1()
            .border_color(colors.border)
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
        let screen_list = &self.screen_list;
        let diagrams = &self.diagrams;
        self.session.update(cx, |session, cx| {
            let rows = session.rows();
            let is_off_view = rows.diagram_row().is_some_and(|row| {
                screen_list.item_is_above_viewport(row) == Some(true)
                    || screen_list.item_is_below_viewport(row) == Some(true)
            });
            diagrams.update(cx, |diagrams, cx| {
                diagrams.show(rows.diagram(), window, cx);
                if is_off_view {
                    diagrams.leave_screen(window);
                }
            });
        });
    }

    fn render_row(
        &mut self,
        index: usize,
        selected: Option<&SelectedRows>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = Colors::active(cx);
        let Some(row) = self.session.read(cx).rows().row(index).cloned() else {
            return div().into_any_element();
        };
        match row {
            PageRow::SummaryTitle(title) => div().pb_3().text_2xl().child(title).into_any_element(),
            PageRow::SummaryBullet(bullet) => div().pb_3().child(bullet).into_any_element(),
            PageRow::StepHeading(heading) => step_heading(&heading, cx).into_any_element(),
            PageRow::StepWhy(why) => div()
                .pb_3()
                .text_color(colors.muted_text)
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
            } => repin_notes(colors, gone_pieces, text_may_be_old).into_any_element(),
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
                        this.change_review(cx, |review| review.toggle_fold(&step));
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
            PageRow::NoChangeSinceRead => no_change_since_read_row(colors).into_any_element(),
            PageRow::BinaryFile(link) => binary_file_row(colors, link).into_any_element(),
            PageRow::Image { path, .. } => piece_row(colors)
                .child(self.image_row(&path, cx))
                .into_any_element(),
            PageRow::OnlyWhitespace => only_whitespace_row(colors).into_any_element(),
            PageRow::HunkGap { id, more } => {
                hunk_gap(colors, id, more, self.on_more_context(cx)).into_any_element()
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
                ..
            } => {
                let thread = self.thread_element(&view, cx);
                if is_under_a_line {
                    piece_row(colors).child(thread).into_any_element()
                } else {
                    div().pb_3().child(thread).into_any_element()
                }
            }
            PageRow::QueuedComment(queued) => piece_row(colors)
                .child(self.queued_comment(&queued, cx))
                .into_any_element(),
            PageRow::Composer { place, .. } => piece_row(colors)
                .children(self.composer(&place, cx))
                .into_any_element(),
            PageRow::AskMarker(ask) => piece_row(colors)
                .flex()
                .px_2()
                .py_1()
                .child(self.ask_at(&ask, cx))
                .into_any_element(),
            PageRow::PieceEnd => piece_end(colors).into_any_element(),
        }
    }

    fn ask_at(&self, ask: &AskAt, cx: &mut Context<Self>) -> Stateful<Div> {
        self.ask_button(ask.id.to_string(), ask.place.clone(), ask.label.clone(), cx)
    }

    fn on_more_context(&self, cx: &mut Context<Self>) -> OnMoreContext {
        Rc::new(cx.listener(|this, more: &MoreContext, _, cx| {
            this.change_review(cx, |review| review.show_more_context(more));
        }))
    }

    fn toggle_tick(&mut self, id: &StepId, cx: &mut Context<Self>) {
        self.session
            .update(cx, |session, cx| session.toggle_tick(id, cx));
    }
}

trait TickedRow {
    fn when_ticked(self, is_ticked: bool, colors: Colors) -> Self;
}

impl<E: Styled> TickedRow for E {
    fn when_ticked(self, is_ticked: bool, colors: Colors) -> Self {
        if is_ticked {
            self.text_color(colors.muted_text)
        } else {
            self
        }
    }
}

trait CurrentRow {
    fn when_current(self, is_current: bool, colors: Colors) -> Self;
}

impl<E: Styled> CurrentRow for E {
    fn when_current(self, is_current: bool, colors: Colors) -> Self {
        if is_current {
            self.bg(colors.current)
        } else {
            self
        }
    }
}

fn step_heading(heading: &StepHeading, cx: &mut Context<OneStepLayout>) -> impl IntoElement {
    let colors = Colors::active(cx);
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
                this.change_review(cx, |review| review.toggle_fold(&id));
            }))
    } else {
        title
    };
    div()
        .pb_3()
        .flex()
        .items_center()
        .gap_3()
        .when(heading.is_gone, |row| row.text_color(colors.muted_text))
        .child(title)
        .child(tick_button(heading.is_ticked, heading.id.clone(), cx))
}

fn whitespace_toggle(is_on: bool, cx: &mut Context<OneStepLayout>) -> impl IntoElement {
    let colors = Colors::active(cx);
    div()
        .id("ignore-whitespace")
        .px_3()
        .py_1()
        .rounded_md()
        .border_1()
        .border_color(colors.border)
        .cursor_pointer()
        .text_sm()
        .when_current(is_on, colors)
        .on_click(cx.listener(|this, _, _, cx| {
            this.change_review(cx, Review::toggle_ignore_whitespace);
        }))
        .child("Hide whitespace")
}

fn repin_notes(colors: Colors, gone_pieces: usize, text_may_be_old: bool) -> impl IntoElement {
    div()
        .pb_3()
        .flex()
        .gap_3()
        .text_sm()
        .text_color(colors.muted_text)
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
    let colors = Colors::active(cx);
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
            .text_color(colors.muted_text)
            .on_click(cx.listener(move |this, _, _, cx| {
                this.change_review(cx, |review| review.toggle_full_diff(&step));
            }))
            .child(label),
    )
}

fn tool_button(
    colors: Colors,
    id: impl Into<gpui::ElementId>,
    label: &'static str,
) -> Stateful<Div> {
    div()
        .id(id)
        .px_2()
        .py_1()
        .cursor_pointer()
        .text_sm()
        .text_color(colors.muted_text)
        .border_1()
        .border_color(colors.border)
        .rounded_md()
        .child(label)
}

fn tick_button(is_ticked: bool, id: StepId, cx: &mut Context<OneStepLayout>) -> impl IntoElement {
    let colors = Colors::active(cx);
    div()
        .id("tick")
        .px_3()
        .py_1()
        .rounded_md()
        .border_1()
        .border_color(colors.border)
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
        let colors = Colors::active(cx);
        let is_composing = self.session.read(cx).composing().is_some();
        let key_context = if is_composing {
            format!("{} {}", Self::KEY_CONTEXT, CommentComposer::KEY_CONTEXT)
        } else {
            Self::KEY_CONTEXT.to_string()
        };
        let frame_painted_marker = self.frame_stats.frame_started();
        self.run_scroll_script(window, cx);
        self.show_diagram(window, cx);
        let session = self.session.read(cx);
        let selected = Rc::new(
            session
                .review()
                .row_selection()
                .and_then(|selection| session.rows().selected_rows(selection)),
        );
        let old_head = session.old_head().cloned();
        let repin_progress = session.repin_progress().clone();
        let refresh_brief = session.refresh_brief().clone();
        let refresh_progress = session.refresh_progress().clone();
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
            .bg(colors.background)
            .font_family(TEXT_FONT)
            .text_color(colors.text)
            .child(self.sidebar(cx))
            .child(
                div()
                    .flex_1()
                    .h_full()
                    .flex()
                    .flex_col()
                    .children(old_head.clone().map(|old_head| {
                        NewCommitsBanner::new(
                            old_head,
                            repin_progress,
                            cx.listener(|this, _, _, cx| {
                                this.session.update(cx, |session, cx| session.repin(cx))
                            }),
                        )
                    }))
                    .children(self.lines_to_ask.clone().map(|place| {
                        let label = place.lines_label().unwrap_or_default();
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
                            .bg(colors.current)
                            .child(notice)
                    }))
                    .children(
                        Some(refresh_brief)
                            .filter(|brief| !brief.is_empty() && old_head.is_none())
                            .map(|brief| {
                                RefreshBanner::new(
                                    brief,
                                    refresh_progress,
                                    cx.listener(|this, _, _, cx| {
                                        this.session
                                            .update(cx, |session, cx| session.refresh_tour(cx))
                                    }),
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
