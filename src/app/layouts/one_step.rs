use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, App, AppContext, ClipboardItem, Context, Div, ElementId, Entity, FocusHandle,
    Focusable, FontWeight, InteractiveElement, IntoElement, ListOffset, ListState, MouseButton,
    ParentElement, Pixels, Render, SharedString, Stateful, StatefulInteractiveElement, Styled,
    Subscription, Window, actions, div, list, px,
};
use std::os::unix::process::CommandExt;

use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

use crate::app::active_session::use_active_session;
use crate::app::code_links::{
    open_code, open_code_hint, open_code_icon, open_code_label, open_link, use_code_opener,
};
use crate::app::code_selection::{CodeSelection, TextSpot};
use crate::app::components::ask_panel::{
    AskPanel, AskSetup, HandToAgent, UseAsComment, ask_button,
};
use crate::app::components::button::{
    ButtonKind, ButtonSize, button, button_icon, button_keys, icon_label_button, joined,
    toggle_button,
};
use crate::app::components::card::card;
use crate::app::components::chip::{build_chip, callout, chip, dropping_in, notice_band};
use crate::app::components::comment_composer::{
    CommentCapabilities, CommentComposer, ComposerActions, OnCompose, QueuedCommentNote,
};
use crate::app::components::diagram::{DiagramSet, DiagramView, Diagrams};
use crate::app::components::diff_view::{
    CodeGesture, CodeSelectionHooks, DiffRow, GapAsk, OnGapAsk, OnImageComment, OnRowGesture,
    OnScrollX, PieceHeader, PieceMarker, RowGesture, binary_file_row, box_row, hung_row, hunk_gap,
    no_change_since_read_row, only_whitespace_row, piece_end, rendered_block_row,
    rendered_file_waiting_row,
};
use crate::app::components::finish_review::{FinishReviewDialog, OnBodyEdit, OnFinish};
use crate::app::components::fold_toggle::FoldToggle;
use crate::app::components::icon::{Icon, icon, sized_icon};
use crate::app::components::image_row::ImageRow;
use crate::app::components::jump_palette::{JumpEvent, JumpPalette};
use crate::app::components::loading_screen::LoadingScreen;
use crate::app::components::markdown_view::{
    InlineMarkdown, MarkdownView, PreparedMarkdown, UnderPicture, bullet, parsed_markdown,
};
use crate::app::components::motion::{
    BANNER_DROP, Edge, SCREEN, SCREEN_SLIDE, STATE, glide_to, glide_to_share, grow_when_shown,
    pop_in, slide_in, turn_to,
};
use crate::app::components::new_commits_banner::NewCommitsBanner;
use crate::app::components::palette::{
    CARD_RADIUS, CONTROL_RADIUS, Colors, INNER_RADIUS, SIDEBAR_WIDTH, TEXT_FONT, Tone, TrailStage,
};
use crate::app::components::picture_zoom::{PictureZoom, use_picture_zoom};
use crate::app::components::pr_switcher::{PrSwitcherView, PrSwitching, SwitcherPlace};
use crate::app::components::refresh_banner::RefreshBanner;
use crate::app::components::safety_net_view::SafetyNetIntro;
use crate::app::components::segmented::{Segment, SegmentLayout, Segmented};
use crate::app::components::settings_panel::{Placement, SettingsEvent, SettingsPanel};
use crate::app::components::shortcuts_sheet::ShortcutsSheet;
use crate::app::components::sidebar_row::{
    RowMark, RowPlace, check_flag, count_badge, row_note, row_places, sidebar_row, skim_chip,
};
use crate::app::components::step_notes::{StepNotesCard, note_label};
use crate::app::components::switching_veil::SwitchingVeil;
use crate::app::components::thread_view::{
    DeleteQuestion, OnCommentEdit, OnDeleteQuestion, OnReplyDraft, OnThreadAction,
    OnTryHandOffAgain, ReplyDraftChange, ThreadChanges, ThreadDrafts, ThreadElement,
};
use crate::app::components::title_bar::{TRAFFIC_LIGHTS_WIDTH, TitleBarArea, TitleBarEdge};
use crate::app::components::toast_stack::{Toaster, post_toast};
use crate::app::components::tooltip::tooltip;
use crate::app::components::version_link::version_link;
use crate::app::frame_stats::{FrameStatsHook, ScriptMove};
use crate::app::page_rows::DESCRIPTION_KEY;
use crate::app::prose_selection::ProseSelections;
use crate::app::review_session::{ReviewSession, SessionEvent, SessionWiring, TourWork};
use crate::app::sidebar_rows::SidebarTourAgent;
use crate::app::updater::{UpdateInstalled, Updater, Updates, running_build};
use crate::code_source::CodeSource;
use crate::file_path::FilePath;
use crate::ide::{Ide, NoIde};
use crate::viewer::{
    AgentReplied, AskAt, Audience, BoxEdges, CodeLink, Command, CommentBox, CommentGesture,
    CommentPlace, Departure, Effect, FileView, GitHubPort, HandOffId, InFlight, Intent,
    LineLocation, NextScreen, NoteSubject, NotesButton, Overlay, PageRow, Place, QueuedComment,
    ReadProgress, Reopen, ReopenAfterSave, Reopening, RestartArgs, RestartGuard, RestartStep,
    Review, ReviewChange, ReviewerSummaryRow, RouteStopRow, Screen, ScreenFooter, ScreenTravel,
    SelectedRows, SendMode, SettingsSheet, StepHeading, StepId, StepIntro, SummaryColumns,
    SummaryStatsRow, TextBox, ThreadId, ThreadRowPlace, ThreadView, ToastTone, Toolbar, TourLead,
    Travel, ViewSwitch, WindowMoment,
};

#[cfg(feature = "snapshots")]
mod gallery;
#[cfg(feature = "snapshots")]
mod scenes;

actions!(
    one_step,
    [
        GoForward,
        GoBack,
        ToggleJump,
        CopySelection,
        ToggleWrapLines,
        ToggleSettings,
        GoToNextUnread,
        ToggleRead,
        ToggleShortcuts,
        OpenPrOnGitHub,
        CloseWindow
    ]
);

use crate::app::Quit;

type OnTextSpot = Rc<dyn Fn(CodeGesture, TextSpot, &mut Window, &mut App)>;

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
    description_diagrams: DiagramSet,
    focus: FocusHandle,
    /// The comment being written takes keys with this one.
    composer_focus: FocusHandle,
    ask: Entity<AskPanel>,
    code_selection: Option<CodeSelection>,
    /// The sidebar row touching the current one that the mouse is on, so the current row squares the corners they share.
    hovered_touching_row: Option<(usize, RowPlace)>,
    on_code_gesture: OnTextSpot,
    /// Counts the asks to open the finish-review form; the form opens when it sees a new one.
    finish_form_request: u32,
    /// Which way the reader went to the screen on show, so its content comes in from that side.
    screen_travel: ScreenTravel,
    /// How far each file box on the screen is scrolled sideways, by path.
    code_scroll_x: CodeScrollX,
    /// Why the last drag opened no comment box.
    selection_notice: Option<String>,
    /// The rows of the last drag, to ask the AI about.
    frame_stats: FrameStatsHook,
    #[cfg(feature = "snapshots")]
    _snapshots: Option<gpui::Task<()>>,
    jump: Entity<JumpPalette>,
    pr_switcher: Entity<PrSwitcherView>,
    shortcuts: Entity<ShortcutsSheet>,
    picture_zoom: Entity<PictureZoom>,
    settings: Entity<SettingsPanel>,
    loading: Entity<LoadingScreen>,
    /// Lives and dies with this window's session: whatever ends the session (the reopen itself, or a
    /// switch to another PR) ends a wait for it, so no state has to be carried across the root swap.
    reopen_after_save: ReopenAfterSave,
    restart_guard: RestartGuard,
    /// `None` opens all code on GitHub.
    ide: Result<Ide, NoIde>,
    _session_subscriptions: [Subscription; 2],
    _use_as_comment_subscription: Subscription,
}

impl OneStepLayout {
    pub const KEY_CONTEXT: &str = "OneStepLayout";

    pub fn new(
        review: Review,
        source: Arc<dyn CodeSource>,
        ask: AskSetup,
        collaborators: Collaborators,
        carried_settings: Option<Entity<SettingsPanel>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let Collaborators {
            github,
            ide,
            tour_work,
        } = collaborators;
        let focus = cx.focus_handle();
        #[cfg(feature = "snapshots")]
        let ask = crate::app::snapshots::quiet_ask(ask);
        let frame_stats = FrameStatsHook::from_env();
        let session = cx.new(|cx| {
            ReviewSession::new(
                review,
                SessionWiring {
                    source,
                    github,
                    tour_work,
                    frame_stats: frame_stats.clone(),
                },
                window,
                cx,
            )
        });
        let ask = cx.new(|cx| AskPanel::new(ask, session.clone(), focus.clone(), cx));
        let session_subscriptions = [
            cx.observe(&session, |this, session, cx| {
                this.screen_travel.arrive(session.read(cx).review());
                this.use_code_opener_of_review(cx);
                this.reopen_if_work_ended(cx);
                cx.notify();
            }),
            cx.subscribe(&session, |this, _, SessionEvent::Reopened, cx| {
                this.reopened(cx);
            }),
        ];
        let diagrams = Self::new_diagrams(cx);
        let jump = cx.new(|cx| JumpPalette::new(focus.clone(), cx));
        let switching = cx.global::<PrSwitching>().clone();
        let loading = cx.new(|cx| LoadingScreen::new(&switching.active, cx));
        cx.observe(&switching.active, |_, _, cx| cx.notify())
            .detach();
        let settings = if let Some(settings) = carried_settings {
            settings.update(cx, |settings, cx| {
                settings.move_to(focus.clone(), Placement::SheetOverReview, cx);
            });
            settings
        } else {
            let home = switching.home.clone();
            cx.new(|cx| SettingsPanel::new(home, focus.clone(), Placement::SheetOverReview, cx))
        };
        cx.subscribe_in(
            &settings,
            window,
            |this, _, event, window, cx| match event {
                SettingsEvent::Saved => this.settings_saved(window, cx),
                SettingsEvent::Closed => {}
            },
        )
        .detach();
        let pr_switcher = cx
            .new(|cx| PrSwitcherView::new(switching, SwitcherPlace::OverReview, focus.clone(), cx));
        let shortcuts = cx.new(|cx| ShortcutsSheet::new(focus.clone(), cx));
        let picture_zoom = cx.new(|cx| PictureZoom::new(focus.clone(), cx));
        cx.subscribe_in(
            &jump,
            window,
            |this, palette, event: &JumpEvent, window, cx| match event {
                JumpEvent::QueryChanged(query) => {
                    let groups = this
                        .session
                        .read(cx)
                        .review()
                        .palette(query, running_build());
                    palette.update(cx, |palette, _| palette.set_groups(groups));
                }
                JumpEvent::Act(action) => this.act((*action).into(), window, cx),
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
            },
        )
        .detach();
        if let Some(Updates(updater)) = cx.try_global::<Updates>().cloned() {
            cx.observe(&updater, |_, _, cx| cx.notify()).detach();
            cx.subscribe(&updater, |this, _, installed: &UpdateInstalled, cx| {
                let restart = this.session.read(cx).review().restart_args();
                this.restart_app_with(installed.program.clone(), &restart, cx);
            })
            .detach();
        }
        let use_as_comment_subscription =
            cx.subscribe_in(&ask, window, |this, _, UseAsComment(ask), window, cx| {
                this.open_comment_box(CommentGesture::AskAnswer(ask), window, cx);
            });
        cx.subscribe(&ask, |this, _, HandToAgent { ask, place }, cx| {
            let handing = this.session.read(cx).hand_ask_to_agent(ask, place, cx);
            let id = ask.id;
            this.reopen_after_save.ask_handed_to_agent();
            cx.spawn(async move |this, cx| {
                let line = handing.await;
                this.update(cx, |this, cx| {
                    this.session.update(cx, |session, cx| {
                        session.change_asks(cx, |review| {
                            review.change_asks(|desk| desk.hand_off_ended(id, &line));
                        });
                    });
                    let in_flight = this.in_flight(cx);
                    let reopen = this.reopen_after_save.ask_hand_off_ended(in_flight);
                    this.reopen_if_due(reopen, cx);
                })
                .ok();
            })
            .detach();
        })
        .detach();
        let layout_handle = cx.weak_entity();
        let on_row_gesture: OnRowGesture = Rc::new(move |gesture, at, _window, cx| {
            layout_handle
                .update(cx, |this, cx| this.on_row_gesture(gesture, at, cx))
                .ok();
        });
        let layout_handle = cx.weak_entity();
        let on_code_gesture: OnTextSpot = Rc::new(move |gesture, at, window, cx| {
            layout_handle
                .update(cx, |this, cx| {
                    if gesture != CodeGesture::Over {
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
        use_active_session(&session, cx);
        use_picture_zoom(&picture_zoom, cx);
        use_code_opener(opener, cx);
        let mut screen_travel = ScreenTravel::default();
        screen_travel.arrive(session.read(cx).review());
        Self {
            screen_list: session.read(cx).screen_list().clone(),
            finish_form_request: 0,
            screen_travel,
            code_scroll_x: CodeScrollX::default(),
            session,
            on_row_gesture,
            diagrams,
            description_diagrams: DiagramSet::default(),
            focus,
            composer_focus: cx.focus_handle(),
            ask,
            code_selection: None,
            hovered_touching_row: None,
            on_code_gesture,
            selection_notice: None,
            frame_stats,
            #[cfg(feature = "snapshots")]
            _snapshots: scenes::start(window, cx),
            jump,
            pr_switcher,
            shortcuts,
            picture_zoom,
            settings,
            loading,
            reopen_after_save: ReopenAfterSave::default(),
            restart_guard: RestartGuard::default(),
            ide,
            _session_subscriptions: session_subscriptions,
            _use_as_comment_subscription: use_as_comment_subscription,
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

    fn new_description_diagrams(sources: &[&str], cx: &mut Context<Self>) -> DiagramSet {
        let set = DiagramSet::new(sources, cx);
        for (_, diagrams) in set.iter() {
            cx.observe(diagrams, |this, _, cx| {
                if let Some(row) = this.session.read(cx).rows().description_row() {
                    this.screen_list.remeasure_items(row..row + 1);
                }
                cx.notify();
            })
            .detach();
        }
        set
    }

    fn change_review<T>(&self, cx: &mut App, change: impl FnOnce(&mut Review) -> T) -> T {
        self.session
            .update(cx, |session, cx| session.change(cx, change))
    }

    pub fn flush_typing(&mut self, cx: &mut Context<Self>) {
        self.session
            .update(cx, |session, _| session.save_typing_now());
    }

    pub fn switch_failed(&mut self, reason: String, cx: &mut Context<Self>) {
        self.session
            .update(cx, |session, cx| session.switch_failed(reason, cx));
    }

    pub fn hand_off_replied(
        &mut self,
        id: HandOffId,
        replied: &AgentReplied,
        cx: &mut Context<Self>,
    ) -> anyhow::Result<()> {
        self.session
            .update(cx, |session, cx| session.hand_off_replied(id, replied, cx))
    }

    pub fn reload_tour(&mut self, cx: &mut Context<Self>) {
        self.session.update(cx, ReviewSession::reload_tour);
    }

    /// The re-pin or Refresh opened a new review: diagrams follow it.
    fn reopened(&mut self, cx: &mut Context<Self>) {
        self.diagrams = Self::new_diagrams(cx);
        cx.notify();
    }

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
            let has_next = self.change_review(cx, |review| review.go_forward_unfolded());
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
        kind: ButtonKind,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let count = self.session.read(cx).review().asks().count_at(&place);
        ask_button(&Colors::active(cx), SharedString::from(key), count, kind).on_click(cx.listener(
            move |this, _, window, cx| {
                this.ask.update(cx, |panel, cx| {
                    panel.open(place.clone(), label.clone(), window, cx);
                });
            },
        ))
    }

    fn on_row_gesture(&mut self, gesture: RowGesture, at: &LineLocation, cx: &mut Context<Self>) {
        if gesture == RowGesture::Press {
            self.selection_notice = None;
        }
        self.session.update(cx, |session, cx| match gesture {
            RowGesture::Press => session.select_from(at.clone(), cx),
            RowGesture::Over => session.extend_selection(at.clone(), cx),
        });
    }

    fn on_code_gesture(&mut self, gesture: CodeGesture, at: TextSpot, cx: &mut Context<Self>) {
        match gesture {
            CodeGesture::Press => {
                let screen = self.session.read(cx).review().screen().clone();
                self.code_selection = Some(CodeSelection::start(screen, at));
            }
            CodeGesture::PressTwice => {
                let session = self.session.read(cx);
                let screen = session.review().screen().clone();
                self.code_selection = session
                    .rows()
                    .line(at.row)
                    .and_then(|line| CodeSelection::word(screen, at, &line.code));
            }
            CodeGesture::Over => match &mut self.code_selection {
                Some(selection) if selection.is_dragging() => selection.drag_to(at),
                _ => return,
            },
        }
        cx.notify();
    }

    fn copy_selection(&mut self, _: &CopySelection, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = ProseSelections::text(cx) {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
            post_toast(cx, ToastTone::Neutral, "Copied".to_owned());
            return;
        }
        let session = self.session.read(cx);
        let Some(selection) = self
            .code_selection
            .as_ref()
            .filter(|selection| selection.is_on(session.review().screen()))
        else {
            return;
        };
        let rows = session.rows();
        let text = selection.text(|row| rows.line(row).map(|line| line.code.as_ref()));
        let lines = text.lines().count();
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        post_toast(
            cx,
            ToastTone::Neutral,
            if lines == 1 {
                "Copied 1 line".to_owned()
            } else {
                format!("Copied {lines} lines")
            },
        );
    }

    fn end_row_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let started = Instant::now();
        if let Some(selection) = &mut self.code_selection
            && selection.is_dragging()
            && !selection.end_drag()
        {
            self.code_selection = None;
            cx.notify();
        }
        if !self.session.read(cx).review().is_selecting() {
            return;
        }
        self.open_comment_box(CommentGesture::DragEnded, window, cx);
        self.frame_stats.selection_ended(started.elapsed());
    }

    /// The box that opens takes the keys. A refusal is named above the page until the next press
    /// on the rows or the next answer used as a comment.
    fn open_comment_box(
        &mut self,
        gesture: CommentGesture,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let opened = self.session.update(cx, |session, cx| {
            session.change(cx, |review| review.open_comment_box(gesture))
        });
        match opened {
            Ok(Some(_)) => {
                if matches!(gesture, CommentGesture::AskAnswer(_)) {
                    self.selection_notice = None;
                }
                window.focus(&self.composer_focus, cx);
            }
            Ok(None) => {}
            Err(refusal) => self.selection_notice = Some(refusal.to_string()),
        }
        cx.notify();
    }

    /// The comments on the whole file are rows under it, not part of it.
    fn view_switch(&self, switch: &ViewSwitch, cx: &mut Context<Self>) -> Stateful<Div> {
        let segment = |view: FileView, label: &'static str, cx: &mut Context<Self>| {
            let path = switch.path.clone();
            Segment::new(
                format!("view-{label}-{path}"),
                label,
                cx.listener(move |this, _, _, cx| {
                    this.change_review(cx, |review| {
                        review.display_options_mut().show_file_as(&path, view);
                    });
                }),
            )
        };
        let segments = vec![
            segment(FileView::Code, "Code", cx),
            segment(FileView::Rendered, "Rendered", cx),
        ];
        let picked = match switch.view {
            FileView::Code => 0,
            FileView::Rendered => 1,
        };
        div()
            .id(SharedString::from(format!("view-switch-{}", switch.path)))
            .flex_none()
            .child(
                Segmented::new(
                    format!("view-{}", switch.path),
                    SegmentLayout::Row {
                        segment_width: px(76.),
                    },
                    segments,
                )
                .compact()
                .picked(Some(picked)),
            )
    }

    fn image_row(&self, path: &FilePath, cx: &mut Context<Self>) -> ImageRow {
        let layout = cx.entity();
        let comment_path = path.clone();
        let on_comment: OnImageComment = Rc::new(move |window, cx| {
            layout.update(cx, |this, cx| {
                this.open_comment_box(CommentGesture::OnFile(&comment_path), window, cx);
            });
        });
        let session = self.session.read(cx);
        ImageRow::new(
            path.clone(),
            session
                .review()
                .display_options()
                .image_preview(path)
                .cloned(),
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

    fn queued_comment(&self, queued: &QueuedComment, cx: &mut Context<Self>) -> QueuedCommentNote {
        let on_discard = self.on_compose(&queued.draft.place, cx, |this, place, _, cx| {
            this.session.update(cx, |session, cx| {
                session.discard(&TextBox::Draft(place), cx);
            });
        });
        QueuedCommentNote::new(queued.clone(), on_discard)
    }

    /// The draft is read here, so typing redraws the box without new rows.
    fn composer(&self, place: &CommentPlace, cx: &mut Context<Self>) -> Option<CommentComposer> {
        let session = self.session.read(cx);
        let review = session.review();
        let CommentBox {
            draft,
            state,
            problem,
        } = review.comment_box(place)?;
        let draft = draft.clone();
        let capabilities = CommentCapabilities {
            comment_now: review.can_comment_now(),
            suggest_change: review.can_suggest_change(place),
            ask_agent: session.agent_link_is_on(),
        };
        let ask_place = review.ask_place_of(place);
        let on_type = {
            let (layout, draft) = (cx.entity(), TextBox::Draft(place.clone()));
            Rc::new(move |body: String, _: &mut Window, cx: &mut App| {
                layout.update(cx, |this, cx| {
                    this.session
                        .update(cx, |session, cx| session.type_in(&draft, &body, cx));
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
            on_stop: self.on_compose(place, cx, |this, place, window, cx| {
                this.session
                    .update(cx, |session, cx| session.leave(&TextBox::Draft(place), cx));
                window.focus(&this.focus, cx);
            }),
            on_add_to_review: self.on_compose(place, cx, |this, place, window, cx| {
                this.session.update(cx, |session, cx| {
                    session.send_comment(&place, SendMode::AddToReview, Audience::Author, cx);
                });
                window.focus(&this.focus, cx);
            }),
            on_comment_now: self.on_compose(place, cx, |this, place, window, cx| {
                this.session.update(cx, |session, cx| {
                    session.send_comment(&place, SendMode::CommentNow, Audience::Author, cx);
                });
                window.focus(&this.focus, cx);
            }),
            on_ask_agent: self.on_compose(place, cx, |this, place, window, cx| {
                this.session.update(cx, |session, cx| {
                    session.send_comment(&place, SendMode::AddToReview, Audience::Agent, cx);
                });
                window.focus(&this.focus, cx);
            }),
            on_ask_agent_now: self.on_compose(place, cx, |this, place, window, cx| {
                this.session.update(cx, |session, cx| {
                    session.send_comment(&place, SendMode::CommentNow, Audience::Agent, cx);
                });
                window.focus(&this.focus, cx);
            }),
            on_suggest_change: self.on_compose(place, cx, |this, place, window, cx| {
                // A refused or failed save is one of the review's problems, which the sidebar shows.
                this.change_review(cx, |review| review.suggest_change(&place))
                    .ok();
                window.focus(&this.composer_focus, cx);
            }),
            on_discard: self.on_compose(place, cx, |this, place, window, cx| {
                this.session.update(cx, |session, cx| {
                    session.discard(&TextBox::Draft(place), cx);
                });
                window.focus(&this.focus, cx);
            }),
        };
        let ask = ask_place.map(|ask_place| {
            let label = ask_place.lines_label().unwrap_or_default();
            self.ask_button(
                format!("ask-{}", place.describe()),
                ask_place,
                label,
                ButtonKind::Ghost,
                cx,
            )
            .into_any_element()
        });
        Some(
            CommentComposer::new(
                draft,
                self.composer_focus.clone(),
                state,
                capabilities,
                problem,
                actions,
            )
            .with_ask(ask),
        )
    }

    fn on_thread_action(&self) -> OnThreadAction {
        let session = self.session.clone();
        Rc::new(move |action, on_done, cx| {
            session.update(cx, |session, cx| session.act_on_thread(action, on_done, cx));
        })
    }

    fn on_try_hand_off_again(&self) -> OnTryHandOffAgain {
        let session = self.session.clone();
        Rc::new(move |hand_off, cx| {
            session.update(cx, |session, cx| session.try_hand_off_again(hand_off, cx));
        })
    }

    /// Reply text goes to the core, which saves it, so it survives scrolling and restarts.
    fn on_reply_draft(&self, thread_id: &ThreadId) -> OnReplyDraft {
        let (session, reply) = (self.session.clone(), TextBox::Reply(thread_id.clone()));
        Rc::new(move |change, cx| {
            session.update(cx, |session, cx| change_box(session, &reply, change, cx));
        })
    }

    /// The text of an edit goes to the core, which keeps it until it is sent or given up.
    fn on_comment_edit(&self) -> OnCommentEdit {
        let session = self.session.clone();
        Rc::new(move |comment_id, change, cx| {
            let edit = TextBox::Edit(comment_id.clone());
            session.update(cx, |session, cx| change_box(session, &edit, change, cx));
        })
    }

    fn on_delete_question(&self) -> OnDeleteQuestion {
        let session = self.session.clone();
        Rc::new(move |comment_id, question, cx| {
            session.update(cx, |session, cx| {
                session.change(cx, |review| match question {
                    DeleteQuestion::Ask => review.ask_to_delete(comment_id),
                    DeleteQuestion::Withdraw => review.withdraw_delete(),
                });
            });
        })
    }

    fn on_finish_body_edit(&self) -> OnBodyEdit {
        let session = self.session.clone();
        Rc::new(move |text, cx| {
            session.update(cx, |session, cx| {
                session.type_in(&TextBox::FinishBody, &text, cx);
            });
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

    fn thread_element(&self, view: &Arc<ThreadView>, cx: &mut Context<Self>) -> ThreadElement {
        let id = view.thread.id.clone();
        let review = self.session.read(cx).review();
        let can_comment_now = review.can_comment_now();
        let can_ask_agent = self.session.read(cx).agent_link_is_on();
        let reply = review
            .text_of(&TextBox::Reply(view.thread.id.clone()))
            .map(str::to_owned);
        let edits = view
            .thread
            .comments
            .iter()
            .filter_map(|comment| {
                let text = review.text_of(&TextBox::Edit(comment.id.clone()))?;
                Some((comment.id.clone(), text.to_owned()))
            })
            .collect();
        ThreadElement::new(
            Arc::clone(view),
            ThreadDrafts {
                can_reply_now: can_comment_now,
                can_ask_agent,
                reply,
                edits,
            },
            ThreadChanges {
                on_reply_draft: self.on_reply_draft(&view.thread.id),
                on_comment_edit: self.on_comment_edit(),
                on_delete_question: self.on_delete_question(),
                on_toggle: Box::new(cx.listener(move |this, _, _, cx| {
                    this.change_review(cx, |review| review.toggle_thread_fold(&id));
                })),
                on_action: self.on_thread_action(),
                on_try_hand_off_again: self.on_try_hand_off_again(),
            },
        )
    }

    /// `program`, which may be newer than this app, quits this one and opens the PR on this screen; it
    /// runs on its own so it outlives the app it quits.
    fn restart_app_with(
        &mut self,
        program: PathBuf,
        restart: &RestartArgs,
        cx: &mut Context<Self>,
    ) {
        if !self.may_leave(Departure::Restart, cx) {
            return;
        }
        let started = std::process::Command::new(program)
            .args(restart.args())
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .process_group(0)
            .spawn();
        if let Err(error) = started {
            post_toast(cx, ToastTone::Danger, format!("Could not restart: {error}"));
        }
    }

    /// Whether the app may be left now: with an Ask answering or a hand-off being sent the first
    /// ask only warns, and the same ask again soon after goes ahead.
    fn may_leave(&mut self, departure: Departure, cx: &mut Context<Self>) -> bool {
        let in_flight = self.in_flight(cx);
        match self
            .restart_guard
            .ask_to(departure, in_flight, Instant::now())
        {
            RestartStep::Restart => true,
            RestartStep::Warn(warning) => {
                post_toast(cx, ToastTone::Danger, warning);
                false
            }
        }
    }

    /// Opens the finish-review form in the sidebar with its body box taking the keys.
    fn open_finish_form(&mut self, cx: &mut Context<Self>) {
        self.finish_form_request = self.finish_form_request.wrapping_add(1);
        cx.notify();
    }

    /// Asks about the screen, as its toolbar's Ask button does.
    fn ask_about_screen(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(AskAt { place, label, .. }) = self.session.read(cx).rows().screen_ask().cloned()
        else {
            return;
        };
        self.ask
            .update(cx, |panel, cx| panel.open(place, label, window, cx));
    }

    /// Settings asked for from outside the window, by `guided-review settings`: opens it, never closes it.
    pub fn show_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.act(Intent::ShowSettings, window, cx);
    }

    pub fn show_pr_switcher(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.act(Intent::ShowPrSwitcher, window, cx);
    }

    /// The one place that reacts to Save having written `config.toml`. The PR on screen was built with the
    /// old settings, so it opens again, once no Ask answers and no hand-off is being typed; the panel
    /// closes either way and the wait shows in the window (see `waiting_for_reopen`).
    fn settings_saved(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.settings
            .update(cx, |settings, cx| settings.close(window, cx));
        post_toast(cx, ToastTone::Success, "Settings saved");
        let in_flight = self.in_flight(cx);
        let reopen = self.reopen_after_save.saved(in_flight);
        self.reopen_if_due(reopen, cx);
        cx.notify();
    }

    fn in_flight(&self, cx: &App) -> InFlight {
        InFlight {
            asks_answering: self.session.read(cx).review().asks().answering(),
            delivering_hand_offs: self.session.read(cx).is_delivering_hand_offs(),
        }
    }

    /// Called whenever an Ask turn, a hand-off or an Ask handed to the agent may have ended.
    fn reopen_if_work_ended(&mut self, cx: &mut Context<Self>) {
        let in_flight = self.in_flight(cx);
        let reopen = self.reopen_after_save.work_in_flight_changed(in_flight);
        self.reopen_if_due(reopen, cx);
    }

    fn reopen_if_due(&mut self, reopen: Reopen, cx: &mut Context<Self>) {
        if reopen == Reopen::Now {
            (cx.global::<PrSwitching>().reopen)();
        }
    }

    /// Typing is saved and focus leaves every box, so nothing the reviewer does reaches the session whose
    /// state file the reopen's build is reading. The veil keeps the mouse out; `act` the keys.
    pub fn hold_for_reopen(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.flush_typing(cx);
        window.focus(&self.focus, cx);
    }

    fn open_code_button(key: String, link: &CodeLink, cx: &mut Context<Self>) -> Stateful<Div> {
        let link = link.clone();
        let hint = link.clone();
        tool_button(
            &Colors::active(cx),
            key,
            open_code_icon(&link, cx),
            open_code_label(&link, cx),
        )
        .tooltip(tooltip(move |cx| open_code_hint(&hint, cx)))
        .on_click(move |_, _, cx| open_code(&link, cx))
    }

    /// The opener follows the review: a re-pin moves its head, a focus re-checks the clone.
    fn use_code_opener_of_review(&self, cx: &mut Context<Self>) {
        let opener = self.session.read(cx).review().code_opener(self.ide.clone());
        use_code_opener(opener, cx);
    }

    fn go_forward(&mut self, _: &GoForward, window: &mut Window, cx: &mut Context<Self>) {
        self.note_seen_end(cx);
        self.act(Intent::NextScreen, window, cx);
    }

    /// What the window alone knows when the reader asks for something.
    fn moment(&self, cx: &App) -> WindowMoment {
        let is_reopening = cx.global::<PrSwitching>().active.read(cx).switching_to()
            == Some(self.session.read(cx).review().pr());
        WindowMoment {
            reopening: if is_reopening {
                Reopening::Underway
            } else {
                Reopening::NotUnderway
            },
            settings: if self.settings.read(cx).is_open() {
                SettingsSheet::Open
            } else {
                SettingsSheet::Closed
            },
        }
    }

    /// Carries out what the review decides the reader's intent does.
    fn act(&mut self, intent: Intent, window: &mut Window, cx: &mut Context<Self>) {
        let effect = self
            .session
            .read(cx)
            .review()
            .decide(intent, self.moment(cx));
        match effect {
            Effect::Change(change) => self
                .session
                .update(cx, |session, cx| session.apply(change, cx)),
            Effect::Toggle(Overlay::Jump) => {
                let groups = self.session.read(cx).review().palette("", running_build());
                self.jump.update(cx, |palette, cx| {
                    if palette.is_open() {
                        palette.close(window, cx);
                    } else {
                        palette.open(groups, window, cx);
                    }
                });
            }
            Effect::Toggle(Overlay::PrSwitcher) => self.pr_switcher.update(cx, |switcher, cx| {
                if switcher.is_open() {
                    switcher.close(window, cx);
                } else {
                    switcher.open(window, cx);
                }
            }),
            Effect::Toggle(Overlay::Settings) => self.settings.update(cx, |settings, cx| {
                if settings.is_open() {
                    settings.request_close(window, cx);
                } else {
                    settings.open(window, cx);
                }
            }),
            Effect::Toggle(Overlay::Shortcuts) => self
                .shortcuts
                .update(cx, |sheet, cx| sheet.toggle(window, cx)),
            Effect::OpenSettings => self
                .settings
                .update(cx, |settings, cx| settings.open(window, cx)),
            Effect::OpenPrSwitcher => self
                .pr_switcher
                .update(cx, |switcher, cx| switcher.show(window, cx)),
            Effect::AskAboutScreen => self.ask_about_screen(window, cx),
            Effect::OpenFinishForm => self.open_finish_form(cx),
            Effect::OpenOnGitHub(pr) => open_link(&pr.to_string(), cx),
            Effect::Restart(restart) => match std::env::current_exe() {
                Ok(exe) => self.restart_app_with(exe, &restart, cx),
                Err(error) => {
                    post_toast(cx, ToastTone::Danger, format!("Could not restart: {error}"));
                }
            },
            Effect::CheckForUpdates => match cx.try_global::<Updates>().cloned() {
                Some(Updates(updater)) => updater.update(cx, Updater::check_now),
                None => post_toast(
                    cx,
                    ToastTone::Neutral,
                    "This build does not take updates".to_owned(),
                ),
            },
            Effect::Nothing | Effect::Refused(_) => {}
        }
    }

    fn scroll_code_sideways(
        &mut self,
        path: &FilePath,
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
    fn note_seen_end(&mut self, cx: &mut App) {
        let Some(last) = self.screen_list.item_count().checked_sub(1) else {
            return;
        };
        if self.screen_list.item_is_below_viewport(last) == Some(false) {
            self.session
                .update(cx, |session, _| session.saw_end_of_screen());
        }
    }

    /// Names the PR's tour agent and its status, or why tour agents are off, with Show agent.
    fn tour_agent_block(
        colors: &Colors,
        agent: &SidebarTourAgent,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let show = agent.can_show.then(|| {
            icon_label_button(
                colors,
                "show-tour-agent",
                ButtonKind::Ghost,
                ButtonSize::Small,
                Icon::Terminal,
                "Show agent",
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.session.update(cx, ReviewSession::show_tour_agent);
            }))
        });
        let retry = agent.can_retry.then(|| {
            icon_label_button(
                colors,
                "retry-tour-agent-link",
                ButtonKind::Ghost,
                ButtonSize::Small,
                Icon::Refresh,
                "Retry",
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.session.update(cx, ReviewSession::retry_agent_link);
            }))
        });
        div().flex().flex_col().gap_1().child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_xs()
                        .text_color(colors.muted_text)
                        .child(InlineMarkdown::new("tour-agent-line", &agent.line)),
                )
                .children(show)
                .children(retry),
        )
    }

    /// A row touching the current one tells the layout while the mouse is on it.
    fn touching_hover(
        &self,
        row: Stateful<Div>,
        index: usize,
        place: RowPlace,
        cx: &Context<Self>,
    ) -> Stateful<Div> {
        if !matches!(
            place,
            RowPlace::JustAboveCurrent | RowPlace::JustBelowCurrent
        ) {
            return row;
        }
        row.on_hover(cx.listener(move |this, is_hovered: &bool, _, cx| {
            let hovered = is_hovered.then_some((index, place));
            if this.hovered_touching_row != hovered {
                this.hovered_touching_row = hovered;
                cx.notify();
            }
        }))
    }

    fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = Colors::active(cx);
        let session = self.session.read(cx);
        let sidebar = session.sidebar();
        let progress = sidebar.progress;
        let places = row_places(
            &std::iter::once(sidebar.is_summary_current)
                .chain(sidebar.steps.iter().map(|step| step.is_current))
                .chain(
                    sidebar
                        .safety_net
                        .iter()
                        .map(|safety_net| safety_net.is_current),
                )
                .collect::<Vec<_>>(),
        );
        // By its index and place then: once the current row moves, a row kept from before no longer counts.
        let hovered_touching = self
            .hovered_touching_row
            .filter(|(index, place)| places.get(*index) == Some(place))
            .map(|(_, place)| place);
        let last_index = places.len().saturating_sub(1);
        let mut places = places.into_iter();
        let summary_place = places.next().unwrap_or(RowPlace::Apart);
        let step_places: Vec<RowPlace> = places.by_ref().take(sidebar.steps.len()).collect();
        let safety_net_place = places.next().unwrap_or(RowPlace::Apart);
        let summary_row = self
            .touching_hover(
                sidebar_row(
                    &colors,
                    "summary",
                    0,
                    summary_place,
                    RowMark::Page,
                    hovered_touching,
                ),
                0,
                summary_place,
                cx,
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.change_review(cx, |review| review.go_to_summary());
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
        let problems = sidebar.problems.iter().map(|shown| {
            div()
                .flex()
                .flex_col()
                .gap_0p5()
                .child(
                    div()
                        .text_xs()
                        .text_color(colors.danger)
                        .child(shown.headline.clone()),
                )
                .children(
                    shown
                        .detail
                        .clone()
                        .map(|detail| div().text_xs().text_color(colors.muted_text).child(detail)),
                )
        });
        let changes_not_shown = sidebar
            .changes_not_shown
            .clone()
            .map(|notice| div().text_xs().text_color(colors.danger).child(notice));
        let tour_agent = sidebar
            .tour_agent
            .is_shown
            .then(|| Self::tour_agent_block(&colors, &sidebar.tour_agent, cx));
        let finish_dialog = FinishReviewDialog::new(
            progress.total > 0 && progress.read == progress.total,
            sidebar.finishing,
            sidebar.approve_warning,
            sidebar.last_review.clone(),
            session
                .review()
                .text_of(&TextBox::FinishBody)
                .unwrap_or_default()
                .to_owned(),
            self.on_finish_body_edit(),
            self.on_finish_review(),
        )
        .opened_by(self.finish_form_request);
        let step_rows = sidebar
            .steps
            .iter()
            .zip(step_places.into_iter().enumerate())
            .map(|(step, (offset, place))| {
                let id: StepId = step.id.clone();
                let mark = if step.is_gone {
                    RowMark::Gone
                } else if step.is_ticked {
                    RowMark::Read
                } else {
                    RowMark::Unread
                };
                self.touching_hover(
                    sidebar_row(
                        &colors,
                        SharedString::from(format!("step-{id}")),
                        step.depth,
                        place,
                        mark,
                        hovered_touching,
                    ),
                    offset + 1,
                    place,
                    cx,
                )
                .when(step.is_skim && !step.is_current, |row| {
                    row.text_color(colors.muted_text)
                })
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.change_review(cx, |review| review.go_to(&id));
                }))
                .child(div().flex_1().min_w_0().child(InlineMarkdown::new(
                    format!("sidebar-{}", step.id),
                    &step.label,
                )))
                .children(step.has_check.then(|| check_flag(&colors)))
                .children(step.is_skim.then(|| skim_chip(&colors)))
                .children(count_badge(&colors, step.thread_count))
            });
        let safety_net_row = sidebar.safety_net.as_ref().map(|safety_net| {
            self.touching_hover(
                sidebar_row(
                    &colors,
                    "safety-net",
                    0,
                    safety_net_place,
                    RowMark::Page,
                    hovered_touching,
                ),
                last_index,
                safety_net_place,
                cx,
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.change_review(cx, |review| review.go_to_safety_net());
            }))
            .child(div().flex_1().child(SafetyNetIntro::TITLE))
            .child(row_note(
                &colors,
                match safety_net.pieces {
                    1 => "1 piece".to_owned(),
                    pieces => format!("{pieces} pieces"),
                },
            ))
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
            .child(progress_meter(&colors, progress))
            .child(finish_dialog)
            .children(tour_agent)
            .children(changes_not_shown)
            .children(problems)
            .children(sync_notices);
        div()
            .w(px(SIDEBAR_WIDTH))
            .flex_none()
            .h_full()
            .flex()
            .flex_col()
            .text_sm()
            .child(
                TitleBarArea::new("sidebar-title-bar", gpui::transparent_black())
                    .edge(TitleBarEdge::Open)
                    .child(
                        div()
                            .pl(px(TRAFFIC_LIGHTS_WIDTH))
                            .flex()
                            .items_center()
                            .gap_2()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(colors.muted_text)
                            .child("Guided review")
                            .children(build_chip(&colors)),
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
            .children(update_button(&colors, cx))
            .child(self.settings_row(&colors, cx))
    }

    /// Settings and the shortcuts sheet at the foot of the sidebar, under a quiet line with the version
    /// this app runs, so the Settings row stays at the very bottom.
    fn settings_row(&self, colors: &Colors, cx: &mut Context<Self>) -> impl IntoElement {
        let shortcuts = button(
            colors,
            "sidebar-shortcuts",
            ButtonKind::Ghost,
            ButtonSize::Regular,
        )
        .px_2()
        .gap_1p5()
        .tooltip(tooltip(|_| "Keyboard shortcuts".into()))
        .child(button_icon(
            colors,
            ButtonKind::Ghost,
            ButtonSize::Regular,
            Icon::Keyboard,
        ))
        .child(button_keys(colors, ButtonKind::Ghost, Command::Shortcuts))
        .on_click(cx.listener(|this, _, window, cx| {
            this.act(Intent::ToggleShortcuts, window, cx);
        }));
        let row = div()
            .px_2()
            .pb_2()
            .flex()
            .items_center()
            .gap_1()
            .child(
                button(
                    colors,
                    "sidebar-settings",
                    ButtonKind::Ghost,
                    ButtonSize::Regular,
                )
                .flex_1()
                .justify_start()
                .px_2()
                .child(button_icon(
                    colors,
                    ButtonKind::Ghost,
                    ButtonSize::Regular,
                    Icon::Settings,
                ))
                .child(div().flex_1().child("Settings"))
                .child(button_keys(colors, ButtonKind::Ghost, Command::Settings))
                .on_click(cx.listener(|this, _, window, cx| {
                    this.act(Intent::ToggleSettings, window, cx);
                })),
            )
            .child(shortcuts);
        let version = div().px_4().pt_2().pb_1().flex().child(
            version_link(colors, "sidebar-version", colors.faint_text())
                .text_xs()
                .line_height(px(12.)),
        );
        div().flex_none().child(version).child(row)
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
        .child(button_icon(
            &colors,
            ButtonKind::Ghost,
            ButtonSize::Small,
            Icon::Search,
        ))
        .child("Jump")
        .child(button_keys(&colors, ButtonKind::Ghost, Command::Jump))
        .on_click(cx.listener(|this, _, window, cx| this.act(Intent::ToggleJump, window, cx)));
        let github = button(
            &colors,
            "title-bar-github",
            ButtonKind::Ghost,
            ButtonSize::Small,
        )
        .child(button_icon(
            &colors,
            ButtonKind::Ghost,
            ButtonSize::Small,
            Icon::ExternalLink,
        ))
        .child("GitHub")
        .child(button_keys(
            &colors,
            ButtonKind::Ghost,
            Command::OpenOnGitHub,
        ))
        .on_click(cx.listener(|this, _, window, cx| this.act(Intent::OpenOnGitHub, window, cx)));
        let switch_pr = div()
            .id("title-bar-switch-pr")
            .min_w_0()
            .flex()
            .items_center()
            .gap_2()
            .px_2()
            .py_1()
            .rounded(px(CONTROL_RADIUS))
            .cursor_pointer()
            .hover(|button| button.bg(colors.hover))
            .active(|button| button.bg(colors.pressed))
            .tooltip(tooltip(|_| "Switch pull request".into()))
            .on_click(
                cx.listener(|this, _, window, cx| this.act(Intent::TogglePrSwitcher, window, cx)),
            )
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
            .child(sized_icon(Icon::ChevronDown, colors.muted_text, px(12.)));
        let page_share = scrolled_share(
            -self.screen_list.scroll_px_offset_for_scrollbar().y,
            self.screen_list.max_offset_for_scrollbar().y,
        );
        TitleBarArea::new("title-bar", gpui::transparent_black())
            .edge(TitleBarEdge::Open)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .px_4()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_sm()
                    .child(switch_pr)
                    .child(div().flex_1())
                    .child(jump)
                    .child(github),
            )
            .child(page_hairline(&colors, page_share))
    }

    /// Once the step's heading has scrolled away, a slim bar under the title bar keeps where the
    /// reader is and the tick in reach. Read from the list's own scroll state; nothing is built.
    fn sticky_step_bar(&self, cx: &mut Context<Self>) -> Option<impl IntoElement + use<>> {
        let heading = self.session.read(cx).rows().step_heading()?;
        let is_scrolled_away = self.screen_list.logical_scroll_top().item_ix > 0;
        if !is_scrolled_away {
            return None;
        }
        let heading = Arc::clone(heading);
        let colors = Colors::active(cx);
        let bar = div()
            .w_full()
            .h(px(44.))
            .px_6()
            .flex()
            .items_center()
            .gap_3()
            .bg(colors.background.opacity(0.97))
            .border_b_1()
            .border_color(colors.subtle_border)
            .shadow(colors.sticky_shadow())
            .child(step_place(&colors, &heading))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(InlineMarkdown::new("sticky-step-title", &heading.title)),
            )
            .child(tick_button(
                heading.is_ticked,
                heading.id.clone(),
                ButtonSize::Small,
                cx,
            ));
        Some(div().absolute().top_0().left_0().right_0().child(slide_in(
            bar,
            ("sticky-step-bar", self.screen_travel.arrival().count()),
            STATE,
            Edge::Top,
            BANNER_DROP,
        )))
    }

    /// Strips of a diagram scrolled out of the list get no layout, so they are let go here.
    fn show_diagram(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let screen_list = &self.screen_list;
        let is_off_view = |row: Option<usize>| {
            row.is_some_and(|row| {
                screen_list.item_is_above_viewport(row) == Some(true)
                    || screen_list.item_is_below_viewport(row) == Some(true)
            })
        };
        let session = self.session.read(cx);
        let rows = session.rows();
        let step_diagram = rows.diagram().map(str::to_owned);
        let step_diagram_is_off_view = is_off_view(rows.diagram_row());
        let description_diagrams: Vec<String> = rows
            .description_diagrams()
            .into_iter()
            .map(str::to_owned)
            .collect();
        let description_is_off_view = is_off_view(rows.description_row());

        Self::show_one(
            &self.diagrams,
            step_diagram.as_deref(),
            step_diagram_is_off_view,
            window,
            cx,
        );
        let sources: Vec<&str> = description_diagrams.iter().map(String::as_str).collect();
        if !self.description_diagrams.is_of(&sources) {
            self.description_diagrams = Self::new_description_diagrams(&sources, cx);
        }
        for (source, diagrams) in self.description_diagrams.clone().iter() {
            Self::show_one(diagrams, Some(source), description_is_off_view, window, cx);
        }
    }

    fn show_one(
        diagrams: &Entity<Diagrams>,
        source: Option<&str>,
        is_off_view: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        diagrams.update(cx, |diagrams, cx| {
            diagrams.show(source, window, cx);
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
        let Some(row) = self.session.read(cx).rows().row(index).cloned() else {
            return div().into_any_element();
        };
        let edges = self.session.read(cx).rows().edges(index);
        self.render_page_row(row, index, edges, selected, cx)
    }

    /// `index` tells the row's elements apart from other rows'; a row inside another uses a made-up one.
    fn render_page_row(
        &mut self,
        row: PageRow,
        index: usize,
        edges: BoxEdges,
        selected: Option<&SelectedRows>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = Colors::active(cx);
        match row {
            PageRow::SummaryColumns(columns) => self.summary_columns(&columns, index, cx),
            PageRow::SummaryTitle(title) => div()
                .pb_5()
                .flex()
                .flex_col()
                .items_start()
                .gap_2()
                .child(tour_place(&colors, "SUMMARY", TourStop::On))
                .child(
                    div()
                        .text_2xl()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(InlineMarkdown::new("summary-title", &title)),
                )
                .into_any_element(),
            PageRow::SummaryStats(stats) => summary_stats(&colors, &stats).into_any_element(),
            PageRow::RouteHeading => note_label("THE TOUR")
                .pt_6()
                .pb_2()
                .text_color(colors.muted_text)
                .into_any_element(),
            PageRow::RouteStop(stop) => route_stop(&stop, cx).into_any_element(),
            PageRow::TourLead(lead) => div()
                .pt_5()
                .pb_2()
                .max_w(READING_WIDTH)
                .flex()
                .children(match lead.as_ref() {
                    TourLead::Finish { steps, can_finish } => Some(
                        finish_card(
                            *steps,
                            *can_finish,
                            ReviewAgain::Offered,
                            "summary-lead",
                            cx,
                        )
                        .into_any_element(),
                    ),
                    other => lead_button(other, LeadLabel::Named, "summary-lead", cx),
                })
                .into_any_element(),
            PageRow::SummaryBullet(summary_bullet) => div()
                .pb_2()
                .flex()
                .items_start()
                .gap_3()
                .max_w(READING_WIDTH)
                .child(bullet(&colors, SUMMARY_LINE))
                .child(div().flex_1().child(InlineMarkdown::new(
                    format!("summary-bullet-{index}"),
                    &summary_bullet,
                )))
                .into_any_element(),
            PageRow::StepHeading(heading) => step_heading(&heading, cx).into_any_element(),
            PageRow::StepIntro(intro) => self.step_intro(&intro, index, cx).into_any_element(),
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
                let ask = heading
                    .ask
                    .as_ref()
                    .map(|ask| self.ask_at(ask, ButtonKind::Ghost, cx));
                let view_switch = heading
                    .view
                    .as_ref()
                    .map(|switch| self.view_switch(switch, cx));
                let header = PieceHeader::new(
                    heading.label.clone(),
                    heading.repin_label,
                    view_switch
                        .into_iter()
                        .chain(open_code)
                        .chain(ask)
                        .collect(),
                )
                .closing_its_box(edges.closes);
                match heading.fold.clone() {
                    Some(fold) => header
                        .with_fold_toggle(
                            div()
                                .id(SharedString::from(format!("fold-box-{}", fold.path)))
                                .flex_none()
                                .w(px(14.))
                                .h(px(14.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded(px(INNER_RADIUS))
                                .cursor_pointer()
                                .hover(|chevron| chevron.bg(colors.hover))
                                .active(|chevron| chevron.bg(colors.pressed))
                                .child(turn_to(
                                    sized_icon(Icon::ChevronRight, colors.muted_text, px(12.)),
                                    "fold-box-chevron",
                                    !fold.is_folded,
                                    1.,
                                ))
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
                let ask = heading
                    .ask
                    .as_ref()
                    .map(|ask| self.ask_at(ask, ButtonKind::Ghost, cx));
                PieceMarker::new(
                    heading.label.clone(),
                    heading.repin_label,
                    open_code.into_iter().chain(ask).collect(),
                )
                .with_edges(edges)
                .into_any_element()
            }
            PageRow::NoChangeSinceRead => {
                no_change_since_read_row(&colors, edges).into_any_element()
            }
            PageRow::BinaryFile(binary) => binary_file_row(
                &colors,
                edges,
                format!("binary-{}", binary.piece).into(),
                binary.link.clone(),
                cx,
            )
            .into_any_element(),
            PageRow::Image { path, .. } => box_row(&colors, edges)
                .child(self.image_row(&path, cx))
                .into_any_element(),
            PageRow::OnlyWhitespace => only_whitespace_row(&colors, edges).into_any_element(),
            PageRow::RenderedBlock(block) => rendered_block_row(
                &colors,
                edges,
                block.change,
                MarkdownView::new(block.key.to_string(), parsed_markdown(&block.source)),
            )
            .into_any_element(),
            PageRow::RenderedFileWaiting => {
                rendered_file_waiting_row(&colors, edges).into_any_element()
            }
            PageRow::HunkGap(gap) => hunk_gap(
                &colors,
                edges,
                SharedString::from(&gap.id),
                &gap.gap,
                SharedString::from(&gap.label),
                &self.on_gap_ask(cx),
            )
            .into_any_element(),
            PageRow::Line(line) => {
                let is_selected = selected.is_some_and(|rows| rows.contains(index, &line.location));
                let code_selection = self
                    .code_selection
                    .as_ref()
                    .filter(|selection| selection.is_on(self.session.read(cx).review().screen()))
                    .and_then(|selection| selection.range_on(index, line.code.len()));
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
                    .with_edges(edges)
                    .scrolled_sideways(scroll_x, on_scroll_x)
                    .selected(is_selected)
                    .with_code_selection(CodeSelectionHooks {
                        selected: code_selection,
                        on_gesture: Rc::new(move |gesture, byte, window, cx| {
                            on_code_gesture(gesture, TextSpot { row: index, byte }, window, cx);
                        }),
                    })
                    .with_gestures(Rc::clone(&self.on_row_gesture))
                    .into_any_element()
            }
            PageRow::Thread { view, place, .. } => {
                let thread = self.thread_element(&view, cx);
                match place {
                    ThreadRowPlace::UnderLine => hung_row().child(thread).into_any_element(),
                    ThreadRowPlace::AfterCode => div().pb_3().child(thread).into_any_element(),
                }
            }
            PageRow::QueuedComment(queued) => hung_row()
                .child(self.queued_comment(&queued, cx))
                .into_any_element(),
            PageRow::Composer { place, .. } => hung_row()
                .children(self.composer(&place, cx))
                .into_any_element(),
            PageRow::AskMarker(ask) => hung_row()
                .flex()
                .px_2()
                .py_1()
                .child(self.ask_at(&ask, ButtonKind::Ghost, cx))
                .into_any_element(),
            PageRow::PieceEnd => piece_end(),
        }
    }

    /// The summary keeps its reading width; the reviewer summary and the description sit beside it where
    /// the window has room, else under it.
    fn summary_columns(
        &mut self,
        columns: &SummaryColumns,
        index: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        const INNER_ROWS: usize = 1_000_000;
        let summary: Vec<AnyElement> = columns
            .summary
            .iter()
            .enumerate()
            .map(|(inner, row)| {
                self.render_page_row(
                    row.clone(),
                    INNER_ROWS * (index + 1) + inner,
                    BoxEdges::default(),
                    None,
                    cx,
                )
            })
            .collect();
        let rows = self.session.read(cx).rows();
        let (prepared_summary, prepared_description) = (
            rows.prepared_reviewer_summary(),
            rows.prepared_description(),
        );
        let reviewer_summary = columns
            .reviewer_summary
            .as_ref()
            .zip(prepared_summary)
            .map(|(summary, prepared)| self.reviewer_summary(summary, prepared, cx));
        let description = prepared_description.map(|description| {
            self.markdown_card(
                "PR DESCRIPTION",
                MarkdownView::new(DESCRIPTION_KEY, description)
                    .with_diagrams(self.description_diagrams.clone()),
                cx,
            )
        });
        let beside = (reviewer_summary.is_some() || description.is_some()).then(|| {
            div()
                .flex_1()
                .min_w(DESCRIPTION_MIN_WIDTH)
                .max_w(READING_WIDTH)
                .flex()
                .flex_col()
                .gap_6()
                .children(reviewer_summary)
                .children(description)
        });
        div()
            .flex()
            .flex_wrap()
            .items_start()
            .gap_x_10()
            .child(
                div()
                    .flex_none()
                    .w_full()
                    .max_w(READING_WIDTH)
                    .flex()
                    .flex_col()
                    .children(summary),
            )
            .children(beside)
            .into_any_element()
    }

    fn reviewer_summary(
        &self,
        summary: &ReviewerSummaryRow,
        prepared: Rc<PreparedMarkdown>,
        cx: &mut Context<Self>,
    ) -> Div {
        let colors = Colors::active(cx);
        let (session, buttons) = (self.session.clone(), Arc::clone(&summary.picture_buttons));
        let under_picture: UnderPicture = Rc::new(move |path, cx| {
            let state = buttons.get(path)?;
            Some(
                notes_button(
                    &Colors::active(cx),
                    SharedString::from(format!("picture-to-notes-{}", path.display())),
                    state,
                    NotesLabels::PICTURE,
                    session.clone(),
                    NoteSubject::SummaryPicture(path.to_owned()),
                )
                .into_any_element(),
            )
        });
        let markdown =
            MarkdownView::new("reviewer-summary", prepared).under_each_picture(under_picture);
        self.markdown_card("REVIEWER SUMMARY", markdown, cx)
            .child(notes_button(
                &colors,
                "summary-to-notes",
                &summary.notes_button,
                NotesLabels::SUMMARY,
                self.session.clone(),
                NoteSubject::ReviewerSummary,
            ))
    }

    fn markdown_card(
        &self,
        label: &'static str,
        markdown: MarkdownView,
        cx: &mut Context<Self>,
    ) -> Div {
        let colors = Colors::active(cx);
        div()
            .pb_4()
            .w_full()
            .max_w(READING_WIDTH)
            .flex()
            .flex_col()
            .gap_2()
            .child(note_label(label).text_color(colors.muted_text))
            .child(card(&colors).px_5().py_4().text_sm().child(markdown))
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
            toolbar_toggle(
                &colors,
                "ignore-whitespace",
                is_on,
                Icon::Space,
                "Hide whitespace",
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.change_review(cx, |review| {
                    review.display_options_mut().toggle_ignore_whitespace();
                });
            }))
        });
        let wrap = toolbar.wraps_lines.map(|is_on| {
            toolbar_toggle(&colors, "wrap-lines", is_on, Icon::WrapText, "Wrap lines")
                .tooltip(tooltip(|_| "Wrap long lines (W)".into()))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.change_review(cx, |review| {
                        review.display_options_mut().toggle_wrap_lines();
                    });
                }))
        });
        let toggles: Vec<Stateful<Div>> = whitespace.into_iter().chain(wrap).collect();
        let toggles = (!toggles.is_empty()).then(|| joined(&colors, toggles));
        let ask = toolbar
            .ask
            .as_ref()
            .map(|ask| self.ask_at(ask, ButtonKind::Tinted, cx));
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
            .children(toggles)
            .children(ask.map(|ask| div().pl_1().child(ask)))
    }

    /// The text keeps its reading width; the diagram sits beside it where the window has room, else under it.
    fn step_intro(
        &self,
        intro: &StepIntro,
        index: usize,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let colors = Colors::active(cx);
        let text = div()
            .flex_none()
            .w_full()
            .max_w(READING_WIDTH)
            .flex()
            .flex_col()
            .gap_4()
            .child(div().text_color(colors.muted_text).child(MarkdownView::new(
                format!("step-why-{index}"),
                parsed_markdown(&intro.why),
            )))
            .children(intro.notes.clone().map(StepNotesCard));
        let diagram = intro.diagram.as_ref().map(|diagram| {
            div()
                .min_w_0()
                .max_w_full()
                .flex()
                .flex_col()
                .gap_2()
                .child(DiagramView::new(self.diagrams.clone()))
                .child(notes_button(
                    &Colors::active(cx),
                    "diagram-to-notes",
                    &diagram.notes_button,
                    NotesLabels::DIAGRAM,
                    self.session.clone(),
                    NoteSubject::StepDiagram(diagram.step.clone()),
                ))
        });
        div()
            .pb_4()
            .flex()
            .flex_wrap()
            .items_start()
            .gap_x_8()
            .gap_y_4()
            .child(text)
            .children(diagram)
    }

    fn ask_at(&self, ask: &AskAt, kind: ButtonKind, cx: &mut Context<Self>) -> Stateful<Div> {
        self.ask_button(
            ask.id.to_string(),
            ask.place.clone(),
            ask.label.clone(),
            kind,
            cx,
        )
    }

    fn on_gap_ask(&self, cx: &mut Context<Self>) -> OnGapAsk {
        let layout = cx.weak_entity();
        Rc::new(move |ask, _, cx| {
            layout
                .update(cx, |this, cx| {
                    this.change_review(cx, |review| match ask {
                        GapAsk::More(more) => review.display_options_mut().show_more_context(more),
                        GapAsk::All(gap) => review.display_options_mut().show_hidden_lines(gap),
                    });
                })
                .ok();
        })
    }

    fn toggle_tick(&mut self, id: &StepId, cx: &mut Context<Self>) {
        let change = ReviewChange::ToggleTick(id.clone());
        self.session
            .update(cx, |session, cx| session.apply(change, cx));
    }
}

/// Keeps prose at a readable line length on a wide window; code still takes the full width.
const READING_WIDTH: Pixels = px(760.);
/// Narrower than this beside the summary, the description goes under it instead.
const DESCRIPTION_MIN_WIDTH: Pixels = px(420.);

/// The line height of the summary's bullets, which their dots centre on.
const SUMMARY_LINE: Pixels = px(26.);
/// How far the wash at the top of a screen reaches down before it is all `background`.
const PAGE_WASH_HEIGHT: Pixels = px(184.);
/// The list's top padding, which the wash reaches up through to the column's top edge.
const PAGE_TOP: Pixels = px(24.);

/// Every row of the page column takes the column's side margins; the first one also carries the
/// wash behind the screen's heading, so the wash scrolls away with it.
/// A refused or failed save is one of the review's problems, which the sidebar shows.
fn change_box(
    session: &mut ReviewSession,
    text_box: &TextBox,
    change: ReplyDraftChange,
    cx: &mut Context<ReviewSession>,
) {
    match change {
        ReplyDraftChange::Edit(text) => session.type_in(text_box, &text, cx),
        ReplyDraftChange::Open => {
            session.change(cx, |review| review.open_box(text_box)).ok();
        }
        ReplyDraftChange::Discard => session.discard(text_box, cx),
    }
}

fn page_column_row(colors: &Colors, index: usize, row: AnyElement) -> AnyElement {
    let column = div().relative().px_6();
    if index == 0 {
        column
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .top(-PAGE_TOP)
                    .h(PAGE_WASH_HEIGHT)
                    .bg(colors.page_wash_fill()),
            )
            .child(row)
            .into_any_element()
    } else {
        column.child(row).into_any_element()
    }
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
                .items_start()
                .gap_2()
                .child(step_place(&colors, heading))
                .child(title),
        )
        .child(tick_button(
            heading.is_ticked,
            heading.id.clone(),
            ButtonSize::Regular,
            cx,
        ))
}

/// The step's place on the tour, over its heading; a gone step's has no trail dot.
fn step_place(colors: &Colors, heading: &StepHeading) -> impl IntoElement {
    let text = match (heading.number, heading.is_gone) {
        (_, true) => "Gone since the last version".to_owned(),
        (Some((number, count)), false) => format!("Step {number} of {count}"),
        (None, false) => "Step".to_owned(),
    };
    let stop = if heading.is_gone {
        TourStop::Off
    } else {
        TourStop::On
    };
    tour_place(colors, text.to_uppercase(), stop)
}

/// Whether a screen is a stop on the trail, or a step no longer on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TourStop {
    On,
    Off,
}

/// A screen's place on the tour, over its heading: a label like every other label on the page,
/// led by the trail's dot when the screen is on the trail.
fn tour_place(colors: &Colors, text: impl Into<SharedString>, stop: TourStop) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .gap_1p5()
        .text_color(colors.muted_text)
        .when(stop == TourStop::On, |place| {
            place.child(
                div()
                    .size(px(6.))
                    .rounded_full()
                    .bg(colors.trail.left_right()),
            )
        })
        .child(note_label(text))
}

fn repin_notes(colors: &Colors, gone_pieces: usize, text_may_be_old: bool) -> impl IntoElement {
    let note = |text: String| chip(colors, Tone::Warning).py_0p5().child(text);
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

/// Above Settings, only while a newer release is offered, installing, or installed and waiting for a
/// restart.
fn update_button(colors: &Colors, cx: &App) -> Option<impl IntoElement + use<>> {
    let Updates(updater) = cx.try_global::<Updates>()?.clone();
    let (action, is_installing) = {
        let read = updater.read(cx);
        (read.action()?.clone(), read.is_installing())
    };
    let kind = if is_installing {
        ButtonKind::Secondary
    } else {
        ButtonKind::Tinted
    };
    Some(
        div().flex_none().px_4().pt_2().child(
            icon_label_button(
                colors,
                "update-app",
                kind,
                ButtonSize::Regular,
                Icon::ArrowUp,
                action,
            )
            .w_full()
            .when(!is_installing, |button| {
                button.on_click(move |_, _, cx| updater.update(cx, Updater::act))
            }),
        ),
    )
}

fn screen_footer(footer: &ScreenFooter, cx: &mut Context<OneStepLayout>) -> AnyElement {
    let colors = Colors::active(cx);
    let previous = footer.has_previous.then(|| {
        button(
            &colors,
            "footer-previous",
            ButtonKind::Ghost,
            ButtonSize::Regular,
        )
        .on_click(cx.listener(|this, _, _, cx| {
            this.change_review(cx, |review| review.go_back());
        }))
        .child("Previous")
        .child(button_keys(
            &colors,
            ButtonKind::Ghost,
            Command::PreviousScreen,
        ))
    });
    let next_label = footer.next.as_ref().map(|next| match next {
        NextScreen::Step { title } => title.clone(),
        NextScreen::SafetyNet => SafetyNetIntro::TITLE.to_owned(),
    });
    if let Some(TourLead::Finish { steps, can_finish }) = &footer.lead {
        return div()
            .pt_8()
            .pb_10()
            .flex()
            .items_center()
            .gap_3()
            .children(previous)
            .child(finish_card(
                *steps,
                *can_finish,
                ReviewAgain::NotOffered,
                "footer",
                cx,
            ))
            .into_any_element();
    }
    let action = match (&footer.ticks, footer.next.is_some()) {
        (Some(_), true) => Some(("Mark as read and go on", Onward::Goes)),
        (Some(_), false) => Some(("Mark as read", Onward::Stays)),
        (None, true) => Some(("Next", Onward::Goes)),
        (None, false) => None,
    };
    let lead = footer
        .lead
        .as_ref()
        .and_then(|lead| lead_button(lead, LeadLabel::Short, "footer-lead", cx));
    let primary = action.map(|(label, onward)| {
        button(
            &colors,
            "footer-next",
            ButtonKind::Primary,
            ButtonSize::Large,
        )
        .on_click(cx.listener(|this, _, window, cx| {
            this.act(Intent::MarkReadAndGoOn, window, cx);
        }))
        .child(label)
        .child(button_keys(
            &colors,
            ButtonKind::Primary,
            match onward {
                Onward::Goes => Command::NextScreen,
                Onward::Stays => Command::ToggleRead,
            },
        ))
    });
    let up_next = next_label.map(|title| {
        div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .gap_0p5()
            .child(
                note_label("Up next:")
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(colors.muted_text),
            )
            .child(
                div()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(colors.text)
                    .truncate()
                    .child(InlineMarkdown::new("footer-up-next", &title)),
            )
    });
    let the_end = (footer.next.is_none()).then(|| {
        div()
            .flex_1()
            .min_w_0()
            .text_sm()
            .text_color(colors.muted_text)
            .child(if footer.ticks.is_some() {
                "This is the last step."
            } else {
                "That was the last step. Finish the review in the sidebar."
            })
    });
    div()
        .pt_8()
        .pb_10()
        .flex()
        .items_center()
        .gap_3()
        .children(previous)
        .child(
            card(&colors)
                .flex_1()
                .min_w_0()
                .pl_5()
                .pr_3()
                .py_3()
                .flex()
                .items_center()
                .gap_4()
                .children(up_next)
                .children(the_end)
                .children(lead)
                .children(primary),
        )
        .into_any_element()
}

/// How big the PR is, in the numbers the review truly has.
fn summary_stats(colors: &Colors, stats: &SummaryStatsRow) -> impl IntoElement {
    let item = |icon_name: Icon, text: SharedString| {
        div()
            .flex()
            .items_center()
            .gap_1p5()
            .child(sized_icon(icon_name, colors.muted_text, px(14.)))
            .child(text)
    };
    let dot = || div().size(px(3.)).rounded_full().bg(colors.border);
    div()
        .pb_5()
        .flex()
        .flex_wrap()
        .items_center()
        .gap_3()
        .text_sm()
        .text_color(colors.muted_text)
        .child(item(Icon::Milestone, SharedString::from(&stats.steps)))
        .child(dot())
        .child(item(Icon::FileText, SharedString::from(&stats.files)))
        .child(dot())
        .child(
            div()
                .flex()
                .gap_2()
                .font_family(crate::app::components::palette::CODE_FONT)
                .child(
                    div()
                        .text_color(colors.success)
                        .child(SharedString::from(&stats.added)),
                )
                .child(
                    div()
                        .text_color(colors.danger)
                        .child(SharedString::from(&stats.removed)),
                ),
        )
        .children(stats.open_threads.clone().map(|threads| {
            div()
                .flex()
                .items_center()
                .gap_3()
                .child(dot())
                .child(item(Icon::MessageSquare, SharedString::from(threads)))
        }))
}

/// One top-level step on the route: its number on the route line, its title and why, and whether
/// it is read or has something to check. A click goes there.
fn route_stop(stop: &RouteStopRow, cx: &mut Context<OneStepLayout>) -> impl IntoElement {
    let colors = Colors::active(cx);
    let disc = div()
        .size(px(24.))
        .rounded_full()
        .flex()
        .items_center()
        .justify_center()
        .text_xs()
        .font_weight(FontWeight::SEMIBOLD)
        .border_1()
        .map(|disc| {
            if stop.is_read {
                disc.bg(colors.success_background)
                    .border_color(colors.success_border)
                    .child(sized_icon(Icon::Check, colors.success, px(13.)))
            } else {
                disc.bg(colors.background)
                    .border_color(colors.current_border)
                    .text_color(colors.accent)
                    .child(SharedString::from(&stop.number))
            }
        });
    let line = |is_shown: bool| {
        div()
            .w(px(2.))
            .flex_1()
            .when(is_shown, |line| line.bg(colors.current_border))
    };
    let id = stop.id.clone();
    div()
        .id(SharedString::from(format!("route-{}", stop.id)))
        .max_w(READING_WIDTH)
        .flex()
        .gap_3()
        .px_2()
        .rounded(px(CONTROL_RADIUS))
        .cursor_pointer()
        .hover(|row| row.bg(colors.hover))
        .active(|row| row.bg(colors.pressed))
        .on_click(cx.listener(move |this, _, _, cx| {
            let id = id.clone();
            this.change_review(cx, move |review| review.go_to(&id));
        }))
        .child(
            div()
                .flex_none()
                .w(px(24.))
                .flex()
                .flex_col()
                .items_center()
                .child(
                    div()
                        .h(px(10.))
                        .w(px(2.))
                        .when(!stop.is_first, |line| line.bg(colors.current_border)),
                )
                .child(disc)
                .child(line(!stop.is_last)),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .py_2p5()
                .flex()
                .flex_col()
                .gap_0p5()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .min_w_0()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(if stop.is_read {
                                    colors.muted_text
                                } else {
                                    colors.text
                                })
                                .child(InlineMarkdown::new(
                                    format!("route-title-{}", stop.id),
                                    &stop.title,
                                )),
                        )
                        .children(stop.has_check.then(|| check_flag(&colors)))
                        .children(stop.details.clone().map(|details| {
                            div()
                                .flex_none()
                                .text_xs()
                                .text_color(colors.muted_text)
                                .child(SharedString::from(details))
                        })),
                )
                .children((!stop.why.is_empty()).then(|| {
                    div()
                        .text_sm()
                        .text_color(colors.muted_text)
                        .line_clamp(2)
                        .child(InlineMarkdown::new(
                            format!("route-why-{}", stop.id),
                            &stop.why,
                        ))
                })),
        )
}

/// Whether the finish card also offers to read the tour again from its first step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReviewAgain {
    Offered,
    NotOffered,
}

/// The way into the tour: its start, or the first step not read yet. Finishing has its own card.
/// Whether the lead names the step it goes to, or a label beside it already does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LeadLabel {
    Named,
    Short,
}

fn lead_button(
    lead: &TourLead,
    label_kind: LeadLabel,
    id: &'static str,
    cx: &mut Context<OneStepLayout>,
) -> Option<AnyElement> {
    let colors = Colors::active(cx);
    let (step, label, command) = match lead {
        TourLead::Start(first) => (
            first.id.clone(),
            div().child("Start the tour").into_any_element(),
            Command::NextScreen,
        ),
        TourLead::Continue(next) if label_kind == LeadLabel::Short => (
            next.id.clone(),
            div().child("Continue").into_any_element(),
            Command::NextUnreadStep,
        ),
        TourLead::Continue(next) => (
            next.id.clone(),
            div()
                .flex()
                .min_w_0()
                .gap_1()
                .child(div().flex_none().child("Continue at"))
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .child(InlineMarkdown::new(format!("{id}-title"), &next.title)),
                )
                .into_any_element(),
            Command::NextUnreadStep,
        ),
        TourLead::Finish { .. } | TourLead::Nowhere => return None,
    };
    Some(
        button(&colors, id, ButtonKind::Primary, ButtonSize::Large)
            .flex_shrink(1.)
            .min_w_0()
            .max_w(px(460.))
            .on_click(cx.listener(move |this, _, _, cx| {
                let step = step.clone();
                this.change_review(cx, move |review| review.go_to(&step));
            }))
            .child(label)
            .child(button_keys(&colors, ButtonKind::Primary, command))
            .into_any_element(),
    )
}

/// The tour's end once every step is read: a calm success card whose check pops in once, with
/// Finish review when the PR takes a review from the reader.
fn finish_card(
    steps: usize,
    can_finish: bool,
    review_again: ReviewAgain,
    id: &'static str,
    cx: &mut Context<OneStepLayout>,
) -> impl IntoElement {
    let colors = Colors::active(cx);
    let title = if steps == 1 {
        "The one step is read".to_owned()
    } else {
        format!("All {steps} steps read")
    };
    let finish = can_finish.then(|| {
        icon_label_button(
            &colors,
            SharedString::from(format!("{id}-finish")),
            ButtonKind::Primary,
            ButtonSize::Large,
            Icon::ListChecks,
            "Finish review",
        )
        .on_click(cx.listener(|this, _, _, cx| this.open_finish_form(cx)))
    });
    let again = (review_again == ReviewAgain::Offered).then(|| {
        icon_label_button(
            &colors,
            SharedString::from(format!("{id}-again")),
            ButtonKind::Ghost,
            ButtonSize::Regular,
            Icon::RotateCcw,
            "Review again",
        )
        .on_click(cx.listener(|this, _, _, cx| {
            this.change_review(cx, |review| {
                review.go_to_summary();
                review.go_forward();
            });
        }))
    });
    callout(&colors, Tone::Success)
        .flex_1()
        .min_w_0()
        .pl_4()
        .pr_3()
        .flex()
        .items_center()
        .gap_4()
        .child(
            div()
                .flex_none()
                .size(px(36.))
                .rounded_full()
                .bg(colors.trail_done.diagonal())
                .flex()
                .items_center()
                .justify_center()
                .child(pop_in(
                    sized_icon(Icon::Check, colors.on_accent_fill, px(20.)),
                    SharedString::from(format!("{id}-check")),
                )),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap_0p5()
                .child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(colors.text)
                        .child(title),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(colors.muted_text)
                        .child(if can_finish {
                            "Every step of the tour is read. Send your review when you are ready."
                        } else {
                            "Every step of the tour is read."
                        }),
                ),
        )
        .children(again)
        .children(finish)
}

/// Whether the footer's primary action moves on to another screen, which its key says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Onward {
    Goes,
    Stays,
}

fn tool_button(
    colors: &Colors,
    id: impl Into<ElementId>,
    icon: Icon,
    label: &'static str,
) -> Stateful<Div> {
    icon_label_button(
        colors,
        id,
        ButtonKind::Ghost,
        ButtonSize::Small,
        icon,
        label,
    )
}

/// A diff toolbar toggle, a part of the toolbar's joined group: its icon and name, and a check
/// while it is on.
fn toolbar_toggle(
    colors: &Colors,
    id: &'static str,
    is_on: bool,
    icon: Icon,
    label: &'static str,
) -> Stateful<Div> {
    let kind = if is_on {
        ButtonKind::JoinedOn
    } else {
        ButtonKind::Joined
    };
    icon_label_button(colors, id, kind, ButtonSize::Small, icon, label).when(is_on, |toggle| {
        toggle.child(sized_icon(Icon::Check, colors.accent, px(12.)))
    })
}

fn tick_button(
    is_ticked: bool,
    id: StepId,
    size: ButtonSize,
    cx: &mut Context<OneStepLayout>,
) -> impl IntoElement {
    let colors = Colors::active(cx);
    let kind = if is_ticked {
        ButtonKind::Done
    } else {
        ButtonKind::Secondary
    };
    button(&colors, "tick", kind, size)
        .gap_0()
        .on_click(cx.listener(move |this, _, _, cx| this.toggle_tick(&id, cx)))
        .child(grow_when_shown(
            sized_icon(Icon::Check, colors.success, px(14.)),
            "tick-check",
            is_ticked,
            px(14.),
            px(6.),
        ))
        .child(if is_ticked { "Read" } else { "Mark as read" })
        .child(button_keys(&colors, kind, Command::ToggleRead).ml_2())
}

fn progress_meter(colors: &Colors, progress: ReadProgress) -> impl IntoElement {
    let share = progress.share();
    div()
        .flex()
        .flex_col()
        .gap_2()
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
                .h(px(6.))
                .w_full()
                .rounded_full()
                .bg(colors.subtle_border)
                .child(glide_to_share(
                    div()
                        .h_full()
                        .rounded_full()
                        .bg(colors.trail_fill(TrailStage::of(progress))),
                    "progress-fill",
                    share,
                )),
        )
}

/// How far down the screen the reader is, gliding after the scroll; at the end the green fades in
/// over the trail. The steps read are the sidebar's meter.
fn page_hairline(colors: &Colors, share: f32) -> impl IntoElement {
    let is_at_end = share >= 1.;
    let green = glide_to(
        div()
            .absolute()
            .inset_0()
            .bg(colors.trail_fill(TrailStage::Complete)),
        "page-hairline-green",
        if is_at_end { 1. } else { 0. },
        |green, shown| green.opacity(shown.clamp(0., 1.)),
    );
    div()
        .absolute()
        .left_0()
        .right_0()
        .bottom_0()
        .h(px(2.))
        .bg(colors.subtle_border)
        .child(glide_to_share(
            div()
                .relative()
                .h_full()
                .bg(colors.trail_fill(TrailStage::UnderWay))
                .child(green),
            "page-hairline-fill",
            share,
        ))
}

impl Focusable for OneStepLayout {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for OneStepLayout {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = Colors::active(cx);
        let is_composing = self.session.read(cx).review().composing().is_some();
        let key_context = if is_composing {
            format!("{} {}", Self::KEY_CONTEXT, CommentComposer::KEY_CONTEXT)
        } else {
            Self::KEY_CONTEXT.to_owned()
        };
        let frame_painted_marker = self.frame_stats.frame_started();
        let switching_veil = cx
            .global::<PrSwitching>()
            .active
            .read(cx)
            .switching_to()
            .map(SwitchingVeil::to);
        self.run_scroll_script(window, cx);
        self.note_seen_end(cx);
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
        #[cfg_attr(
            not(feature = "snapshots"),
            expect(unused_mut, reason = "the gallery picture has no step")
        )]
        let mut sticky_bar = self.sticky_step_bar(cx);
        let screen = div().size_full().child(
            list(
                self.screen_list.clone(),
                cx.processor(move |this, index, _window, cx| {
                    let row = this.render_row(index, selected.as_ref().as_ref(), cx);
                    page_column_row(&Colors::active(cx), index, row)
                }),
            )
            .pt(PAGE_TOP)
            .size_full(),
        );
        let screen = slide_in(
            screen,
            ("screen", self.screen_travel.arrival().count()),
            SCREEN,
            match self.screen_travel.heading() {
                Travel::Forward => Edge::Right,
                Travel::Back => Edge::Left,
            },
            SCREEN_SLIDE,
        );
        #[cfg(feature = "snapshots")]
        let screen = if cx
            .try_global::<crate::app::snapshots::StagedGallery>()
            .is_some_and(|staged| staged.0)
        {
            sticky_bar = None;
            gallery::gallery(&colors)
        } else {
            screen.into_any_element()
        };
        let screen = div()
            .relative()
            .flex_1()
            .min_h_0()
            .child(screen)
            .children(sticky_bar);
        div()
            .key_context(key_context.as_str())
            .track_focus(&self.focus)
            // Before the press reaches what is under the pointer, so a double click's word, chosen
            // there, is not the selection cleared here.
            .capture_any_mouse_down(cx.listener(|this, event: &gpui::MouseDownEvent, _, cx| {
                if event.button == MouseButton::Left && ProseSelections::clear_unless_dragging(cx) {
                    cx.notify();
                }
                if event.button == MouseButton::Left
                    && this
                        .code_selection
                        .as_ref()
                        .is_some_and(|selection| !selection.is_dragging())
                {
                    this.code_selection = None;
                    cx.notify();
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| this.end_row_selection(window, cx)),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| this.end_row_selection(window, cx)),
            )
            .on_action(cx.listener(Self::go_forward))
            .on_action(cx.listener(|this, _: &GoBack, window, cx| {
                this.act(Intent::PreviousScreen, window, cx);
            }))
            .on_action(cx.listener(|this, _: &ToggleWrapLines, window, cx| {
                this.act(Intent::ToggleWrapLines, window, cx);
            }))
            .on_action(cx.listener(|this, _: &ToggleJump, window, cx| {
                this.act(Intent::ToggleJump, window, cx);
            }))
            .on_action(cx.listener(|this, _: &ToggleSettings, window, cx| {
                this.act(Intent::ToggleSettings, window, cx);
            }))
            .on_action(cx.listener(|this, _: &GoToNextUnread, window, cx| {
                this.act(Intent::NextUnreadStep, window, cx);
            }))
            .on_action(cx.listener(|this, _: &ToggleRead, window, cx| {
                this.act(Intent::ToggleRead, window, cx);
            }))
            .on_action(cx.listener(|this, _: &ToggleShortcuts, window, cx| {
                this.act(Intent::ToggleShortcuts, window, cx);
            }))
            .on_action(cx.listener(|this, _: &OpenPrOnGitHub, window, cx| {
                this.act(Intent::OpenOnGitHub, window, cx);
            }))
            .on_action(cx.listener(|this, _: &CloseWindow, window, cx| {
                if this.may_leave(Departure::CloseWindow, cx) {
                    window.remove_window();
                }
            }))
            .on_action(cx.listener(|this, _: &Quit, _, cx| {
                if this.may_leave(Departure::Quit, cx) {
                    cx.quit();
                }
            }))
            .on_action(cx.listener(Self::copy_selection))
            .size_full()
            .flex()
            .bg(colors.sidebar_ground())
            .font_family(TEXT_FONT)
            .text_color(colors.text)
            .child(self.sidebar(cx))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .flex()
                    .flex_col()
                    .bg(colors.background)
                    .rounded_l(px(CARD_RADIUS))
                    .border_l_1()
                    .border_color(colors.subtle_border)
                    .shadow(colors.page_shadow())
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
                    .children(self.reopen_after_save.waiting_line().map(|line| {
                        dropping_in(
                            notice_band(&colors, Tone::Warning).child(line.to_owned()),
                            "waiting-line-band",
                        )
                    }))
                    .children(self.selection_notice.clone().map(|notice| {
                        dropping_in(
                            notice_band(&colors, Tone::Warning).child(notice),
                            "selection-notice-band",
                        )
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
            .when(self.ask.read(cx).is_shown(cx), |row| {
                row.child(self.ask.clone())
            })
            .child(self.settings.clone())
            .child(self.loading.clone())
            .children(switching_veil)
            .child(self.jump.clone())
            .child(self.pr_switcher.clone())
            .child(self.shortcuts.clone())
            .child(self.picture_zoom.clone())
            .children(cx.try_global::<Toaster>().map(|toaster| toaster.0.clone()))
            .children(frame_painted_marker)
    }
}

/// Sideways scroll of the file boxes of one screen; another screen starts unscrolled.
#[derive(Default)]
struct CodeScrollX {
    screen: Option<Screen>,
    by_path: HashMap<FilePath, Pixels>,
}

impl CodeScrollX {
    fn of(&self, screen: &Screen, path: &FilePath) -> Pixels {
        match &self.screen {
            Some(scrolled) if scrolled == screen => {
                self.by_path.get(path).copied().unwrap_or(Pixels::ZERO)
            }
            _ => Pixels::ZERO,
        }
    }

    fn scroll(&mut self, screen: &Screen, path: &FilePath, delta: Pixels, max: Pixels) {
        if self.screen.as_ref() != Some(screen) {
            self.screen = Some(screen.clone());
            self.by_path.clear();
        }
        let offset = self.by_path.entry(path.clone()).or_insert(Pixels::ZERO);
        *offset = (*offset + delta).clamp(Pixels::ZERO, max);
    }
}

/// How far down its page the reader is, 0 at the top and 1 at the end; a page that fits is read whole.
fn scrolled_share(scrolled: Pixels, scrollable: Pixels) -> f32 {
    if scrollable <= Pixels::ZERO {
        return 1.;
    }
    (scrolled / scrollable).clamp(0., 1.)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_page_line_follows_the_scroll_from_top_to_end() {
        assert_eq!(scrolled_share(px(0.), px(400.)), 0.);
        assert_eq!(scrolled_share(px(100.), px(400.)), 0.25);
        assert_eq!(scrolled_share(px(400.), px(400.)), 1.);
    }

    #[test]
    fn a_page_that_fits_the_window_is_seen_whole() {
        assert_eq!(scrolled_share(px(0.), px(0.)), 1.);
    }
}

/// The words of a button that puts something under Notes in the PR description.
#[derive(Clone, Copy)]
struct NotesLabels {
    add: &'static str,
    adding: &'static str,
    done: &'static str,
    tooltip: &'static str,
}

impl NotesLabels {
    const DIAGRAM: Self = Self {
        add: "Add to PR notes",
        adding: "Adding to PR notes…",
        done: "In PR notes",
        tooltip: "Put this diagram under Notes in the PR description, folded",
    };
    const PICTURE: Self = Self {
        add: "Add to PR notes",
        adding: "Uploading to PR notes…",
        done: "In PR notes",
        tooltip: "Upload this picture to GitHub and put it under Notes in the PR description, folded",
    };
    const SUMMARY: Self = Self {
        add: "Add summary to PR notes",
        adding: "Adding summary to PR notes…",
        done: "Summary in PR notes",
        tooltip: "Put this summary under Notes in the PR description, folded, its pictures uploaded to GitHub",
    };
}

fn notes_button(
    colors: &Colors,
    id: impl Into<ElementId>,
    state: &NotesButton,
    labels: NotesLabels,
    session: Entity<ReviewSession>,
    subject: NoteSubject,
) -> Div {
    let id = id.into();
    let small = |kind, icon| {
        button(colors, id.clone(), kind, ButtonSize::Small).child(button_icon(
            colors,
            kind,
            ButtonSize::Small,
            icon,
        ))
    };
    let add = small(ButtonKind::Ghost, Icon::FilePlus)
        .tooltip(tooltip(move |_| labels.tooltip.into()))
        .on_click(move |_, _, cx| {
            session.update(cx, |session, cx| session.add_to_notes(&subject, cx));
        })
        .child(labels.add);
    let (shown, failure) = match state {
        NotesButton::Add => (add, None),
        NotesButton::Adding => (
            small(ButtonKind::Ghost, Icon::FilePlus).child(labels.adding),
            None,
        ),
        NotesButton::InNotes => (
            small(ButtonKind::Done, Icon::Check).child(labels.done),
            None,
        ),
        NotesButton::Failed(reason) => (
            add,
            Some(
                div()
                    .flex()
                    .gap_1()
                    .child("Could not add it:")
                    .child(SharedString::from(Arc::clone(reason))),
            ),
        ),
    };
    div()
        .flex()
        .items_center()
        .gap_2()
        .text_xs()
        .text_color(colors.muted_text)
        .child(shown)
        .children(failure)
}
