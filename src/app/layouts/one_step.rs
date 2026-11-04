use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, App, AppContext, ClipboardItem, Context, Div, Entity, FocusHandle, Focusable,
    FontWeight, InteractiveElement, IntoElement, ListOffset, ListState, MouseButton, ParentElement,
    Pixels, Render, SharedString, Stateful, StatefulInteractiveElement, Styled, Subscription,
    Window, actions, div, list, px, relative,
};

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use crate::app::code_links::{
    open_code, open_code_hint, open_code_label, open_link, use_code_opener,
};
use crate::app::code_selection::{CodeSelection, CodeSpot};
use crate::app::components::ask_panel::{AskPanel, AskSetup, UseAsComment, ask_button};
use crate::app::components::button::{ButtonKind, ButtonSize, button, toggle_button};
use crate::app::components::comment_composer::{
    CommentComposer, ComposerActions, OnCompose, QueuedCommentNote,
};
use crate::app::components::diagram::{DiagramView, Diagrams};
use crate::app::components::diff_view::{
    CodeSelectionHooks, DiffRow, OnImageComment, OnMoreContext, OnRowGesture, OnScrollX,
    PieceHeader, PieceMarker, RowGesture, binary_file_row, hunk_gap, no_change_since_read_row,
    only_whitespace_row, piece_end, piece_row,
};
use crate::app::components::finish_review::{FinishReviewDialog, OnBodyEdit, OnFinish};
use crate::app::components::fold_toggle::FoldToggle;
use crate::app::components::icon::{Icon, icon};
use crate::app::components::image_row::ImageRow;
use crate::app::components::jump_palette::{JumpEvent, JumpPalette};
use crate::app::components::markdown_view::{InlineMarkdown, MarkdownView};
use crate::app::components::new_commits_banner::NewCommitsBanner;
use crate::app::components::palette::{Colors, TEXT_FONT};
use crate::app::components::refresh_banner::RefreshBanner;
use crate::app::components::safety_net_view::SafetyNetIntro;
use crate::app::components::sidebar_row::{RowMark, count_badge, sidebar_row};
use crate::app::components::thread_view::{
    OnReplyDraft, OnThreadAction, ReplyDraftChange, ThreadElement,
};
use crate::app::components::title_bar::{TRAFFIC_LIGHTS_WIDTH, TitleBarArea};
use crate::app::components::tooltip::tooltip;
use crate::app::frame_stats::{FrameStatsHook, ScriptMove};
use crate::app::page_rows::{AskAt, PageRow, ScreenFooter, SelectedRows, StepHeading, Toolbar};
use crate::app::review_session::{AskedLinesOf, ReviewSession, SessionEvent, TourWork};
use crate::code_source::CodeSource;
use crate::ide::{Ide, NoIde};
use crate::viewer::{
    Ask, CodeLink, CommentPlace, GitHubPort, LineLocation, Markdown, MoreContext, NextScreen,
    Place, QueuedComment, ReadProgress, Review, Screen, SendMode, StepId, ThreadView,
};

actions!(
    one_step,
    [
        GoForward,
        GoBack,
        ToggleJump,
        CopySelection,
        ToggleWrapLines
    ]
);

type OnCodeSpot = Rc<dyn Fn(RowGesture, CodeSpot, &mut Window, &mut App)>;

/// What the layout talks to outside the window.
pub struct Collaborators {
    pub github: Arc<dyn GitHubPort>,
    /// `None` opens all code on GitHub.
    pub ide: Result<Ide, NoIde>,
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
    /// The comment being written takes keys with this one.
    composer_focus: FocusHandle,
    ask: Entity<AskPanel>,
    code_selection: Option<CodeSelection>,
    on_code_gesture: OnCodeSpot,
    /// The screen whose end the reader has had on screen; the arrow key ticks a step only then.
    seen_end_of: Option<Screen>,
    /// How far each file box on the screen is scrolled sideways, by path.
    code_scroll_x: CodeScrollX,
    /// Why the last drag opened no comment box.
    selection_notice: Option<String>,
    /// The rows of the last drag, to ask the AI about.
    lines_to_ask: Option<Place>,
    frame_stats: FrameStatsHook,
    #[cfg(feature = "snapshots")]
    snapshots: Option<crate::app::snapshots::Snapshots>,
    jump: Entity<JumpPalette>,
    /// `None` opens all code on GitHub.
    ide: Result<Ide, NoIde>,
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
            cx.observe(&session, |this, _, cx| {
                this.use_code_opener_of_review(cx);
                cx.notify();
            }),
            cx.subscribe(&session, |this, _, SessionEvent::Reopened, cx| {
                this.reopened(cx);
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
                this.use_as_comment(ask, window, cx);
            });
        let asked_lines_subscription = cx.observe(&ask, |this, _, cx| {
            this.session.update(cx, ReviewSession::asks_changed);
        });
        let layout_handle = cx.weak_entity();
        let on_row_gesture: OnRowGesture = Rc::new(move |gesture, at, _window, cx| {
            layout_handle
                .update(cx, |this, cx| this.on_row_gesture(gesture, at, cx))
                .ok();
        });
        let layout_handle = cx.weak_entity();
        let on_code_gesture: OnCodeSpot = Rc::new(move |gesture, at, window, cx| {
            layout_handle
                .update(cx, |this, cx| {
                    if gesture == RowGesture::Press {
                        window.focus(&this.focus, cx);
                    }
                    this.on_code_gesture(gesture, at, cx);
                })
                .ok();
        });
        cx.on_app_quit(|this, _| {
            this.frame_stats.report_on_quit();
            async {}
        })
        .detach();
        let opener = session.read(cx).review().code_opener(ide.clone());
        use_code_opener(opener, cx);
        Self {
            screen_list: session.read(cx).screen_list().clone(),
            seen_end_of: None,
            code_scroll_x: CodeScrollX::default(),
            session,
            on_row_gesture,
            diagrams,
            focus,
            composer_focus: cx.focus_handle(),
            ask,
            code_selection: None,
            on_code_gesture,
            selection_notice: None,
            lines_to_ask: None,
            frame_stats,
            #[cfg(feature = "snapshots")]
            snapshots: crate::app::snapshots::Snapshots::from_env(),
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
                panel.recheck_code_changes(session.review(), cx);
            });
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

    #[cfg(feature = "snapshots")]
    fn run_snapshots(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        use crate::app::snapshots::SnapshotMove;
        let Some(snapshots) = self.snapshots.as_mut() else {
            return;
        };
        match snapshots.frame(window) {
            SnapshotMove::Wait => {}
            SnapshotMove::ScrollToEnd => self.screen_list.scroll_to(ListOffset {
                item_ix: self.screen_list.item_count(),
                offset_in_item: px(0.),
            }),
            SnapshotMove::NextScreen => {
                let has_next = self.change_review(cx, Review::go_forward);
                if !has_next {
                    cx.quit();
                }
            }
            SnapshotMove::Done => cx.quit(),
        }
    }

    fn ask_button(
        &self,
        key: String,
        place: Place,
        label: String,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let count = self.ask.read(cx).count_at(&place);
        ask_button(&Colors::active(cx), SharedString::from(key), count).on_click(cx.listener(
            move |this, _, window, cx| {
                let ask = this.ask.clone();
                this.session.update(cx, |session, cx| {
                    ask.update(cx, |panel, cx| {
                        panel.open(session.review(), place.clone(), label.clone(), window, cx);
                    });
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

    fn on_code_gesture(&mut self, gesture: RowGesture, at: CodeSpot, cx: &mut Context<Self>) {
        match gesture {
            RowGesture::Press => {
                let screen = self.session.read(cx).review().screen().clone();
                self.code_selection = Some(CodeSelection::start(screen, at));
            }
            RowGesture::Over => match &mut self.code_selection {
                Some(selection) if selection.is_dragging() => selection.drag_to(at),
                _ => return,
            },
        }
        cx.notify();
    }

    fn copy_selection(&mut self, _: &CopySelection, _: &mut Window, cx: &mut Context<Self>) {
        let session = self.session.read(cx);
        let Some(selection) = self
            .code_selection
            .as_ref()
            .filter(|selection| selection.is_on(session.review().screen()))
        else {
            return;
        };
        let rows = session.rows();
        let text = selection.text(|row| match rows.row(row)? {
            PageRow::Line(line) => Some(line.code()),
            _ => None,
        });
        cx.write_to_clipboard(ClipboardItem::new_string(text));
    }

    fn end_row_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(selection) = &mut self.code_selection
            && selection.is_dragging()
            && !selection.end_drag()
        {
            self.code_selection = None;
            cx.notify();
        }
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
            Ok(Some(_)) => window.focus(&self.composer_focus, cx),
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
            Ok(_) => window.focus(&self.composer_focus, cx),
            Err(refusal) => self.selection_notice = Some(refusal.to_string()),
        }
        cx.notify();
    }

    /// The comments on the whole file are rows under it, not part of it.
    fn image_row(&self, path: &str, cx: &mut Context<Self>) -> ImageRow {
        let layout = cx.entity();
        let comment_path = path.to_owned();
        let on_comment: OnImageComment = Rc::new(move |window, cx| {
            layout.update(cx, |this, cx| {
                this.begin_file_comment(&comment_path, window, cx);
            });
        });
        let session = self.session.read(cx);
        ImageRow::new(
            path.to_owned(),
            session.review().image_preview(path).cloned(),
            session.image_pictures(path),
            on_comment,
        )
    }

    fn on_compose(
        &self,
        place: &CommentPlace,
        cx: &mut Context<Self>,
        act: impl Fn(&mut Self, CommentPlace, &mut Window, &mut Context<Self>) + 'static,
    ) -> OnCompose {
        let (layout, place) = (cx.entity(), place.clone());
        Rc::new(move |window, cx| {
            layout.update(cx, |this, cx| act(this, place.clone(), window, cx));
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
                window.focus(&self.composer_focus, cx);
            }
            Err(refusal) => self.selection_notice = Some(refusal.to_string()),
        }
        cx.notify();
    }

    fn queued_comment(&self, queued: &QueuedComment, cx: &mut Context<Self>) -> QueuedCommentNote {
        let on_discard = self.on_compose(&queued.draft.place, cx, |this, place, _, cx| {
            this.session
                .update(cx, |session, cx| session.discard_comment(&place, cx));
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
        let on_type = {
            let (layout, place) = (cx.entity(), place.clone());
            Rc::new(move |body: String, _: &mut Window, cx: &mut App| {
                layout.update(cx, |this, cx| {
                    this.session
                        .update(cx, |session, cx| session.type_in_comment(&place, &body, cx));
                });
            })
        };
        let actions = ComposerActions {
            on_focus: self.on_compose(place, cx, |this, place, window, cx| {
                this.session
                    .update(cx, |session, cx| session.compose(place, cx));
                window.focus(&this.composer_focus, cx);
            }),
            on_type,
            on_stop: self.on_compose(place, cx, |this, _, window, cx| {
                this.session.update(cx, ReviewSession::stop_composing);
                window.focus(&this.focus, cx);
            }),
            on_add_to_review: self.on_compose(place, cx, |this, place, window, cx| {
                this.session.update(cx, |session, cx| {
                    session.send_comment(place, SendMode::AddToReview, cx);
                });
                window.focus(&this.focus, cx);
            }),
            on_comment_now: self.on_compose(place, cx, |this, place, window, cx| {
                this.session.update(cx, |session, cx| {
                    session.send_comment(place, SendMode::CommentNow, cx);
                });
                window.focus(&this.focus, cx);
            }),
            on_suggest_change: self.on_compose(place, cx, |this, place, window, cx| {
                if let Err(error) = this.change_review(cx, |review| review.suggest_change(&place)) {
                    eprintln!("could not save the suggestion: {error:#}");
                }
                window.focus(&this.composer_focus, cx);
            }),
            on_discard: self.on_compose(place, cx, |this, place, window, cx| {
                this.session
                    .update(cx, |session, cx| session.discard_comment(&place, cx));
                window.focus(&this.focus, cx);
            }),
        };
        Some(CommentComposer::new(
            draft,
            self.composer_focus.clone(),
            is_composing,
            is_sending,
            can_comment_now,
            can_suggest_change,
            problem,
            actions,
        ))
    }

    fn on_thread_action(&self) -> OnThreadAction {
        let session = self.session.clone();
        Rc::new(move |action, on_done, cx| {
            session.update(cx, |session, cx| session.act_on_thread(action, on_done, cx));
        })
    }

    /// Reply text goes to the core, which saves it, so it survives scrolling and restarts.
    fn on_reply_draft(&self, thread_id: &str) -> OnReplyDraft {
        let (session, thread_id) = (self.session.clone(), thread_id.to_owned());
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
                session.finish_review(verdict, &body, on_done, cx);
            });
        })
    }

    fn thread_element(&self, view: &ThreadView, cx: &mut Context<Self>) -> ThreadElement {
        let id = view.thread.id.clone();
        let review = self.session.read(cx).review();
        let can_comment_now = review.can_comment_now();
        let reply = review.reply_draft(&view.thread.id).map(str::to_owned);
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

    fn open_code_button(key: String, link: &CodeLink, cx: &mut Context<Self>) -> Stateful<Div> {
        let link = link.clone();
        let hint = link.clone();
        tool_button(&Colors::active(cx), key, open_code_label(&link, cx))
            .tooltip(tooltip(move |cx| open_code_hint(&hint, cx)))
            .on_click(move |_, _, cx| open_code(&link, cx))
    }

    /// The opener follows the review: a re-pin moves its head, a focus re-checks the clone.
    fn use_code_opener_of_review(&self, cx: &mut Context<Self>) {
        let opener = self.session.read(cx).review().code_opener(self.ide.clone());
        use_code_opener(opener, cx);
    }

    /// Like the footer's button, the arrow key ticks the step it leaves, but only once the reader
    /// has seen the step's end, so skipping ahead does not count as reading.
    fn go_forward(&mut self, _: &GoForward, _: &mut Window, cx: &mut Context<Self>) {
        self.note_seen_end(cx);
        let review = self.session.read(cx).review();
        if let Screen::Step(id) = review.screen().clone()
            && !review.is_ticked(&id)
            && self.seen_end_of.as_ref() == Some(review.screen())
        {
            self.toggle_tick(&id, cx);
        }
        self.change_review(cx, Review::go_forward);
    }

    fn scroll_code_sideways(
        &mut self,
        path: &str,
        delta: Pixels,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let session = self.session.read(cx);
        let font_size = window.rem_size() * 0.875;
        let font_id = window
            .text_system()
            .resolve_font(&gpui::font(crate::app::components::palette::CODE_FONT));
        let char_width = window
            .text_system()
            .advance(font_id, font_size, 'm')
            .map_or(px(8.), |size| size.width);
        let gutter = char_width * 14. + px(16.);
        let shown = self.screen_list.viewport_bounds().size.width - gutter;
        let widest = char_width * session.rows().widest_line(path) as f32;
        let max = (widest - shown + char_width * 2.).max(Pixels::ZERO);
        let screen = session.review().screen().clone();
        self.code_scroll_x.scroll(&screen, path, delta, max);
        cx.notify();
    }

    /// The screen's last row is its footer; once it is on screen, or above it, the end was seen.
    fn note_seen_end(&mut self, cx: &App) {
        let Some(last) = self.screen_list.item_count().checked_sub(1) else {
            return;
        };
        if self.screen_list.item_is_below_viewport(last) == Some(false) {
            self.seen_end_of = Some(self.session.read(cx).review().screen().clone());
        }
    }

    fn go_back(&mut self, _: &GoBack, _: &mut Window, cx: &mut Context<Self>) {
        self.change_review(cx, Review::go_back);
    }

    fn toggle_wrap_lines(&mut self, _: &ToggleWrapLines, _: &mut Window, cx: &mut Context<Self>) {
        self.change_review(cx, Review::toggle_wrap_lines);
    }

    fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = Colors::active(cx);
        let session = self.session.read(cx);
        let sidebar = session.sidebar();
        let progress = sidebar.progress;
        let summary_row = sidebar_row(
            &colors,
            "summary",
            0,
            sidebar.is_summary_current,
            RowMark::Page,
        )
        .on_click(cx.listener(|this, _, _, cx| {
            this.change_review(cx, Review::go_to_summary);
        }))
        .child("Summary");
        let sync_notices = sidebar
            .sync_banner
            .clone()
            .into_iter()
            .chain(
                sidebar
                    .not_synced
                    .then(|| SharedString::from("Not synced with GitHub yet")),
            )
            .map(|notice| div().text_xs().text_color(colors.warning).child(notice));
        let finish_dialog = FinishReviewDialog::new(
            progress.total > 0 && progress.read == progress.total,
            sidebar.can_judge,
            sidebar.approve_warning,
            session.review().finish_body().to_owned(),
            self.on_finish_body_edit(),
            self.on_finish_review(),
        );
        let step_rows = sidebar.steps.iter().map(|step| {
            let id: StepId = step.id.clone();
            let mark = if step.is_gone {
                RowMark::Gone
            } else if step.is_ticked {
                RowMark::Read
            } else {
                RowMark::Unread
            };
            sidebar_row(
                &colors,
                SharedString::from(format!("step-{id}")),
                step.depth,
                step.is_current,
                mark,
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.change_review(cx, |review| review.go_to(&id));
            }))
            .child(div().flex_1().min_w_0().child(InlineMarkdown::new(
                format!("sidebar-{}", step.id),
                &step.label,
            )))
            .children(count_badge(&colors, step.thread_count))
        });
        let safety_net_row = sidebar.safety_net.as_ref().map(|safety_net| {
            sidebar_row(
                &colors,
                "safety-net",
                0,
                safety_net.is_current,
                RowMark::Page,
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.change_review(cx, Review::go_to_safety_net);
            }))
            .child(div().flex_1().child(SafetyNetIntro::TITLE))
            .child(
                div()
                    .flex_none()
                    .text_xs()
                    .text_color(colors.muted_text)
                    .child(match safety_net.pieces {
                        1 => "1 piece".to_owned(),
                        pieces => format!("{pieces} pieces"),
                    }),
            )
            .children(count_badge(&colors, safety_net.thread_count))
        });
        let header = div()
            .flex_none()
            .px_4()
            .pt_4()
            .pb_3()
            .flex()
            .flex_col()
            .gap_3()
            .border_b_1()
            .border_color(colors.subtle_border)
            .child(progress_meter(&colors, progress))
            .child(finish_dialog)
            .children(sync_notices);
        div()
            .w(px(300.))
            .flex_none()
            .h_full()
            .flex()
            .flex_col()
            .bg(colors.sidebar)
            .border_r_1()
            .border_color(colors.border)
            .text_sm()
            .child(
                TitleBarArea::new("sidebar-title-bar", colors.sidebar).child(
                    div()
                        .pl(px(TRAFFIC_LIGHTS_WIDTH))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(colors.muted_text)
                        .child("Guided review"),
                ),
            )
            .child(header)
            .child(
                div()
                    .id("sidebar-steps")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .py_2()
                    .flex()
                    .flex_col()
                    .gap_px()
                    .child(summary_row)
                    .children(step_rows)
                    .children(safety_net_row),
            )
            .child(key_hints(&colors))
    }

    /// The PR this review is of, and the window-wide actions.
    fn title_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = Colors::active(cx);
        let review = self.session.read(cx).review();
        let pr = review.pr().clone();
        let title = review.title().to_owned();
        let jump = button(
            &colors,
            "title-bar-jump",
            ButtonKind::Ghost,
            ButtonSize::Small,
        )
        .child(icon(Icon::Search, colors.muted_text))
        .child("Jump")
        .child(div().pl_1().text_color(colors.muted_text).child("⌘K"))
        .on_click(cx.listener(|this, _, window, cx| this.toggle_jump(&ToggleJump, window, cx)));
        let pr_link = pr.to_string();
        let github = button(
            &colors,
            "title-bar-github",
            ButtonKind::Ghost,
            ButtonSize::Small,
        )
        .child(icon(Icon::ExternalLink, colors.muted_text))
        .child("GitHub")
        .on_click(move |_, _, cx| open_link(&pr_link, cx));
        TitleBarArea::new("title-bar", colors.background).child(
            div()
                .flex_1()
                .min_w_0()
                .px_4()
                .flex()
                .items_center()
                .gap_2()
                .text_sm()
                .child(icon(Icon::PullRequest, colors.success))
                .child(
                    div()
                        .flex_none()
                        .text_color(colors.muted_text)
                        .child(pr.short()),
                )
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .font_weight(FontWeight::MEDIUM)
                        .child(InlineMarkdown::new("title-bar-title", &title)),
                )
                .child(div().flex_1())
                .child(jump)
                .child(github),
        )
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
            PageRow::SummaryTitle(title) => div()
                .pb_4()
                .flex()
                .flex_col()
                .gap_1()
                .child(eyebrow(&colors, "Summary"))
                .child(
                    div()
                        .text_2xl()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(InlineMarkdown::new("summary-title", &title)),
                )
                .into_any_element(),
            PageRow::SummaryBullet(bullet) => div()
                .pb_2()
                .flex()
                .items_start()
                .gap_3()
                .max_w(READING_WIDTH)
                .child(
                    div()
                        .flex_none()
                        .h(px(24.))
                        .flex()
                        .items_center()
                        .child(div().size(px(6.)).rounded_full().bg(colors.accent)),
                )
                .child(div().flex_1().child(InlineMarkdown::new(
                    format!("summary-bullet-{index}"),
                    &bullet,
                )))
                .into_any_element(),
            PageRow::PrDescription(description) => {
                self.pr_description(&description, cx).into_any_element()
            }
            PageRow::StepHeading(heading) => step_heading(&heading, cx).into_any_element(),
            PageRow::StepWhy(why) => div()
                .pb_4()
                .max_w(READING_WIDTH)
                .text_color(colors.muted_text)
                .child(MarkdownView::new(
                    format!("step-why-{index}"),
                    Markdown::parse(&why),
                ))
                .into_any_element(),
            PageRow::Diagram => div()
                .pb_4()
                .child(DiagramView::new(self.diagrams.clone()))
                .into_any_element(),
            PageRow::StepNotes(notes) => div().pb_4().child(notes).into_any_element(),
            PageRow::RepinNotes {
                gone_pieces,
                text_may_be_old,
            } => repin_notes(&colors, gone_pieces, text_may_be_old).into_any_element(),
            PageRow::Toolbar(toolbar) => self.toolbar(&toolbar, cx).into_any_element(),
            PageRow::ScreenFooter(footer) => screen_footer(&footer, cx).into_any_element(),
            PageRow::SafetyNetIntro => SafetyNetIntro.into_any_element(),
            PageRow::PieceHeader(heading) => {
                let open_code = heading.code_link.as_ref().map(|link| {
                    Self::open_code_button(format!("open-piece-{}", heading.piece), link, cx)
                });
                let ask = heading.ask.as_ref().map(|ask| self.ask_at(ask, cx));
                let header = PieceHeader::new(
                    heading.piece.clone(),
                    heading.repin_label,
                    open_code.into_iter().chain(ask).collect(),
                );
                match heading.fold.clone() {
                    Some(fold) => header
                        .with_fold_toggle(
                            div()
                                .id(SharedString::from(format!("fold-box-{}", fold.path)))
                                .flex_none()
                                .w(px(14.))
                                .cursor_pointer()
                                .text_color(colors.muted_text)
                                .hover(|chevron| chevron.text_color(colors.accent))
                                .child(if fold.is_folded { "▸" } else { "▾" })
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.change_review(cx, |review| {
                                        review.toggle_box_fold(&fold.step, &fold.path);
                                    });
                                })),
                        )
                        .into_any_element(),
                    None => header.into_any_element(),
                }
            }
            PageRow::PieceMarker(heading) => {
                let open_code = heading.code_link.as_ref().map(|link| {
                    Self::open_code_button(format!("open-marker-{}", heading.piece), link, cx)
                });
                let ask = heading.ask.as_ref().map(|ask| self.ask_at(ask, cx));
                PieceMarker::new(
                    heading.piece.clone(),
                    heading.repin_label,
                    open_code.into_iter().chain(ask).collect(),
                )
                .into_any_element()
            }
            PageRow::NoChangeSinceRead => no_change_since_read_row(&colors).into_any_element(),
            PageRow::BinaryFile { piece, link } => {
                binary_file_row(&colors, format!("binary-{piece}").into(), link, cx)
                    .into_any_element()
            }
            PageRow::Image { path, .. } => piece_row(&colors)
                .child(self.image_row(&path, cx))
                .into_any_element(),
            PageRow::OnlyWhitespace => only_whitespace_row(&colors).into_any_element(),
            PageRow::HunkGap { id, more } => {
                hunk_gap(&colors, id, more, self.on_more_context(cx)).into_any_element()
            }
            PageRow::Line(line) => {
                let is_selected = selected.is_some_and(|rows| rows.contains(index, &line.location));
                let code_selection = self
                    .code_selection
                    .as_ref()
                    .filter(|selection| selection.is_on(self.session.read(cx).review().screen()))
                    .and_then(|selection| selection.range_on(index, line.code().len()));
                let on_code_gesture = Rc::clone(&self.on_code_gesture);
                let path = line.location.path.clone();
                let scroll_x = self
                    .code_scroll_x
                    .of(self.session.read(cx).review().screen(), &path);
                let layout_handle = cx.weak_entity();
                let on_scroll_x: OnScrollX = Rc::new(move |delta, window, cx| {
                    layout_handle
                        .update(cx, |this, cx| {
                            this.scroll_code_sideways(&path, delta, window, cx);
                        })
                        .ok();
                });
                DiffRow::new(line)
                    .scrolled_sideways(scroll_x, on_scroll_x)
                    .selected(is_selected)
                    .with_code_selection(CodeSelectionHooks {
                        selected: code_selection,
                        on_gesture: Rc::new(move |gesture, byte, window, cx| {
                            on_code_gesture(gesture, CodeSpot { row: index, byte }, window, cx);
                        }),
                    })
                    .with_gestures(Rc::clone(&self.on_row_gesture))
                    .into_any_element()
            }
            PageRow::Thread {
                view,
                is_under_a_line,
                ..
            } => {
                let thread = self.thread_element(&view, cx);
                if is_under_a_line {
                    piece_row(&colors).child(thread).into_any_element()
                } else {
                    div().pb_3().child(thread).into_any_element()
                }
            }
            PageRow::QueuedComment(queued) => piece_row(&colors)
                .child(self.queued_comment(&queued, cx))
                .into_any_element(),
            PageRow::Composer { place, .. } => piece_row(&colors)
                .children(self.composer(&place, cx))
                .into_any_element(),
            PageRow::AskMarker(ask) => piece_row(&colors)
                .flex()
                .px_2()
                .py_1()
                .child(self.ask_at(&ask, cx))
                .into_any_element(),
            PageRow::PieceEnd => piece_end(&colors).into_any_element(),
        }
    }

    fn pr_description(&self, description: &Markdown, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = Colors::active(cx);
        div()
            .pt_2()
            .pb_4()
            .w_full()
            .max_w(READING_WIDTH)
            .flex()
            .flex_col()
            .gap_2()
            .child(eyebrow(&colors, "PR description"))
            .child(
                div()
                    .px_4()
                    .py_3()
                    .rounded_md()
                    .border_1()
                    .border_color(colors.subtle_border)
                    .bg(colors.surface)
                    .text_sm()
                    .child(MarkdownView::new("pr-description", description.clone())),
            )
    }

    fn toolbar(&self, toolbar: &Toolbar, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = Colors::active(cx);
        let fold = toolbar.fold.as_ref().map(|fold| {
            let step = fold.step.clone();
            FoldToggle::new(
                fold.is_folded,
                fold.piece_count,
                cx.listener(move |this, _, _, cx| {
                    this.change_review(cx, |review| review.toggle_fold(&step));
                }),
            )
        });
        let full_diff = toolbar.full_diff.as_ref().map(|control| {
            let step = control.step.clone();
            toggle_button(
                &colors,
                "full-diff-toggle",
                control.is_showing_full_diff,
                ButtonSize::Small,
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.change_review(cx, |review| review.toggle_full_diff(&step));
            }))
            .child("Full diff, not only the change since your read")
        });
        let whitespace = toolbar.ignores_whitespace.map(|is_on| {
            toggle_button(&colors, "ignore-whitespace", is_on, ButtonSize::Small)
                .on_click(cx.listener(|this, _, _, cx| {
                    this.change_review(cx, Review::toggle_ignore_whitespace);
                }))
                .child("Hide whitespace")
        });
        let wrap = toolbar.wraps_lines.map(|is_on| {
            toggle_button(&colors, "wrap-lines", is_on, ButtonSize::Small)
                .tooltip(tooltip(|_| "Wrap long lines (W)".into()))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.change_review(cx, Review::toggle_wrap_lines);
                }))
                .child("Wrap lines")
        });
        let ask = toolbar.ask.as_ref().map(|ask| self.ask_at(ask, cx));
        div()
            .w_full()
            .pb_3()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_2()
            .children(fold)
            .children(full_diff)
            .child(div().flex_1())
            .children(whitespace)
            .children(wrap)
            .children(ask)
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

/// Keeps prose at a readable line length on a wide window; code still takes the full width.
const READING_WIDTH: Pixels = px(760.);

fn eyebrow(colors: &Colors, text: impl Into<SharedString>) -> impl IntoElement {
    div()
        .text_xs()
        .font_weight(FontWeight::MEDIUM)
        .text_color(colors.muted_text)
        .child(text.into().to_uppercase())
}

fn step_heading(heading: &StepHeading, cx: &mut Context<OneStepLayout>) -> impl IntoElement {
    let colors = Colors::active(cx);
    let title = div()
        .id("step-title")
        .text_2xl()
        .font_weight(FontWeight::SEMIBOLD)
        .child(InlineMarkdown::new("step-title", &heading.title));
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
    let eyebrow_text = match (heading.number, heading.is_gone) {
        (_, true) => "Gone since the last version".to_owned(),
        (Some((number, count)), false) => format!("Step {number} of {count}"),
        (None, false) => "Step".to_owned(),
    };
    div()
        .w_full()
        .pb_2()
        .flex()
        .items_end()
        .gap_4()
        .when(heading.is_gone, |row| row.text_color(colors.muted_text))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap_1()
                .child(eyebrow(&colors, eyebrow_text))
                .child(title),
        )
        .child(tick_button(heading.is_ticked, heading.id.clone(), cx))
}

fn repin_notes(colors: &Colors, gone_pieces: usize, text_may_be_old: bool) -> impl IntoElement {
    let note = |text: String| {
        div()
            .px_2()
            .py_0p5()
            .rounded_md()
            .bg(colors.warning_background)
            .text_color(colors.warning)
            .child(text)
    };
    div()
        .pb_3()
        .flex()
        .gap_2()
        .text_xs()
        .when(gone_pieces > 0, |row| {
            row.child(note(match gone_pieces {
                1 => "1 piece gone since last version".to_owned(),
                count => format!("{count} pieces gone since last version"),
            }))
        })
        .when(text_may_be_old, |row| {
            row.child(note("The text may describe an older version".to_owned()))
        })
}

fn screen_footer(footer: &ScreenFooter, cx: &mut Context<OneStepLayout>) -> impl IntoElement {
    let colors = Colors::active(cx);
    let previous = footer.has_previous.then(|| {
        button(
            &colors,
            "footer-previous",
            ButtonKind::Ghost,
            ButtonSize::Regular,
        )
        .on_click(cx.listener(|this, _, _, cx| {
            this.change_review(cx, Review::go_back);
        }))
        .child("← Previous")
    });
    let next_label = footer.next.as_ref().map(|next| match next {
        NextScreen::Step { title } => title.clone(),
        NextScreen::SafetyNet => SafetyNetIntro::TITLE.to_owned(),
    });
    let action = match (&footer.ticks, footer.next.is_some(), footer.is_summary) {
        (_, true, true) => Some("Start the tour →"),
        (Some(_), true, false) => Some("Mark as read and go on →"),
        (Some(_), false, _) => Some("Mark as read"),
        (None, true, false) => Some("Next →"),
        (None, false, _) => None,
    };
    let primary = action.map(|label| {
        let ticks = footer.ticks.clone();
        button(
            &colors,
            "footer-next",
            ButtonKind::Primary,
            ButtonSize::Regular,
        )
        .on_click(cx.listener(move |this, _, _, cx| {
            if let Some(id) = &ticks {
                this.toggle_tick(id, cx);
            }
            this.change_review(cx, Review::go_forward);
        }))
        .child(label)
    });
    let up_next = next_label.map(|title| {
        div()
            .flex()
            .gap_1()
            .text_sm()
            .text_color(colors.muted_text)
            .child("Up next:")
            .child(
                div()
                    .text_color(colors.text)
                    .child(InlineMarkdown::new("footer-up-next", &title)),
            )
    });
    let the_end = (footer.next.is_none()).then(|| {
        div()
            .text_sm()
            .text_color(colors.muted_text)
            .child(if footer.ticks.is_some() {
                "This is the last step."
            } else {
                "That was the last step. Finish the review in the sidebar."
            })
    });
    div().pt_4().pb_10().child(
        div()
            .pt_4()
            .border_t_1()
            .border_color(colors.subtle_border)
            .flex()
            .items_center()
            .gap_3()
            .children(previous)
            .child(div().flex_1())
            .children(up_next)
            .children(the_end)
            .children(primary),
    )
}

fn tool_button(
    colors: &Colors,
    id: impl Into<gpui::ElementId>,
    label: &'static str,
) -> Stateful<Div> {
    button(colors, id, ButtonKind::Ghost, ButtonSize::Small).child(label)
}

fn tick_button(is_ticked: bool, id: StepId, cx: &mut Context<OneStepLayout>) -> impl IntoElement {
    let colors = Colors::active(cx);
    let kind = if is_ticked {
        ButtonKind::Done
    } else {
        ButtonKind::Secondary
    };
    button(&colors, "tick", kind, ButtonSize::Regular)
        .on_click(cx.listener(move |this, _, _, cx| this.toggle_tick(&id, cx)))
        .child(if is_ticked {
            "✓ Read"
        } else {
            "Mark as read"
        })
}

fn progress_meter(colors: &Colors, progress: ReadProgress) -> impl IntoElement {
    let share = if progress.total == 0 {
        0.
    } else {
        progress.read as f32 / progress.total as f32
    };
    let is_done = progress.total > 0 && progress.read == progress.total;
    div()
        .flex()
        .flex_col()
        .gap_1p5()
        .child(
            div()
                .flex()
                .justify_between()
                .child(div().font_weight(FontWeight::MEDIUM).child(format!(
                    "{} of {} steps read",
                    progress.read, progress.total
                )))
                .child(
                    div()
                        .text_color(colors.muted_text)
                        .child(format!("{}%", (share * 100.).round())),
                ),
        )
        .child(
            div()
                .h(px(4.))
                .w_full()
                .rounded_full()
                .bg(colors.subtle_border)
                .child(
                    div()
                        .h_full()
                        .w(relative(share))
                        .rounded_full()
                        .bg(if is_done {
                            colors.success
                        } else {
                            colors.accent
                        }),
                ),
        )
}

fn key_hints(colors: &Colors) -> impl IntoElement {
    let hint = |keys: &'static [&'static str], what: &'static str| {
        div()
            .flex()
            .items_center()
            .gap_1()
            .children(keys.iter().map(|key| key_cap(colors, key)))
            .child(div().pl_1().child(what))
    };
    div()
        .flex_none()
        .px_4()
        .py_2()
        .flex()
        .flex_col()
        .gap_1()
        .border_t_1()
        .border_color(colors.subtle_border)
        .text_xs()
        .text_color(colors.muted_text)
        .child(hint(&["←", "→"], "previous / next screen"))
        .child(hint(&["⌘", "K"], "jump to a step, file or symbol"))
        .child(hint(&["W"], "wrap long lines"))
}

fn key_cap(colors: &Colors, key: &'static str) -> impl IntoElement {
    div()
        .min_w(px(18.))
        .h(px(18.))
        .px_1()
        .flex()
        .items_center()
        .justify_center()
        .rounded_sm()
        .border_1()
        .border_color(colors.border)
        .bg(colors.surface)
        .text_color(colors.text)
        .child(key)
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
            Self::KEY_CONTEXT.to_owned()
        };
        let frame_painted_marker = self.frame_stats.frame_started();
        self.run_scroll_script(window, cx);
        self.note_seen_end(cx);
        #[cfg(feature = "snapshots")]
        self.run_snapshots(window, cx);
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
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    if this
                        .code_selection
                        .as_ref()
                        .is_some_and(|selection| !selection.is_dragging())
                    {
                        this.code_selection = None;
                        cx.notify();
                    }
                }),
            )
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
            .on_action(cx.listener(Self::toggle_wrap_lines))
            .on_action(cx.listener(Self::toggle_jump))
            .on_action(cx.listener(Self::copy_selection))
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
                    .child(self.title_bar(cx))
                    .children(old_head.clone().map(|old_head| {
                        NewCommitsBanner::new(
                            old_head,
                            repin_progress,
                            cx.listener(|this, _, _, cx| {
                                this.session.update(cx, ReviewSession::repin);
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
                            .bg(colors.warning_background)
                            .text_color(colors.warning)
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
                                        this.session.update(cx, ReviewSession::refresh_tour);
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

/// Sideways scroll of the file boxes of one screen; another screen starts unscrolled.
#[derive(Default)]
struct CodeScrollX {
    screen: Option<Screen>,
    by_path: HashMap<String, Pixels>,
}

impl CodeScrollX {
    fn of(&self, screen: &Screen, path: &str) -> Pixels {
        match &self.screen {
            Some(scrolled) if scrolled == screen => {
                self.by_path.get(path).copied().unwrap_or(Pixels::ZERO)
            }
            _ => Pixels::ZERO,
        }
    }

    fn scroll(&mut self, screen: &Screen, path: &str, delta: Pixels, max: Pixels) {
        if self.screen.as_ref() != Some(screen) {
            self.screen = Some(screen.clone());
            self.by_path.clear();
        }
        let offset = self.by_path.entry(path.to_owned()).or_insert(Pixels::ZERO);
        *offset = (*offset + delta).clamp(Pixels::ZERO, max);
    }
}
