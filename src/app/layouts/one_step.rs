use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, App, AppContext, ClipboardItem, Context, Div, ElementId, Entity, FocusHandle,
    Focusable, InteractiveElement, IntoElement, ListOffset, ListState, MouseButton,
    NavigationDirection, ParentElement, Pixels, Render, SharedString, Stateful,
    StatefulInteractiveElement, Styled, Subscription, Window, actions, div, list, px,
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
use crate::app::components::agent_picker::AgentPicker;
use crate::app::components::button::{ButtonKind, ButtonSize, button_keys, icon_label_button};
use crate::app::components::chip::{dropping_in, notice_band};
use crate::app::components::comment_composer::CommentComposer;
use crate::app::components::comments_sheet::CommentsSheet;
use crate::app::components::diagram::{DiagramSet, Diagrams};
use crate::app::components::diff_view::{
    CodeGesture, OnImageComment, OnNamePointer, OnRowGesture, RowGesture,
};
use crate::app::components::files_sheet::FilesSheet;
use crate::app::components::finding_view::FindingQuestions;
use crate::app::components::findings_sheet::FindingsSheet;
use crate::app::components::guided_tour_banner::GuidedTourReadyBanner;
use crate::app::components::icon::Icon;
use crate::app::components::image_row::ImageRow;
use crate::app::components::jump_palette::{JumpEvent, JumpPalette};
use crate::app::components::loading_screen::LoadingScreen;
use crate::app::components::markdown_view::PreparedMarkdown;
use crate::app::components::motion::{Edge, SCREEN, SCREEN_SLIDE, slide_in};
use crate::app::components::new_commits_banner::NewCommitsBanner;
use crate::app::components::palette::{CARD_RADIUS, CODE_FONT, Colors, TEXT_FONT, Tone};
use crate::app::components::picture_zoom::{PictureZoom, use_picture_zoom};
use crate::app::components::pr_switcher::{
    DropShownReview, PrSwitcherView, PrSwitching, SwitcherPlace,
};
use crate::app::components::refresh_banner::RefreshBanner;
use crate::app::components::scrollbar::{ScrollbarDrag, drag_listener};
use crate::app::components::segmented::{Segment, SegmentLayout, Segmented};
use crate::app::components::settings_panel::{Placement, SettingsEvent, SettingsPanel};
use crate::app::components::shortcuts_sheet::ShortcutsSheet;
use crate::app::components::sidebar_row::RowPlace;
use crate::app::components::switching_veil::SwitchingVeil;
use crate::app::components::toast_stack::{Toaster, post_toast};
use crate::app::components::tooltip::tooltip;
use crate::app::frame_stats::{FrameStatsHook, ScriptMove};
use crate::app::prose_selection::ProseSelections;
use crate::app::review_session::{ReviewSession, SessionEvent, SessionWiring, TourWork};
use crate::app::updater::{UpdateInstalled, Updater, Updates};
use crate::code_source::CodeSource;
use crate::file_path::FilePath;
use crate::findings::FindingId;
use crate::ide::{Ide, NoIde};
use crate::tour::ToursOnDisk;
use crate::update::running_build;
use crate::viewer::{
    AgentReplied, AiReviewAction, CodeHost, CodeLink, Command, CommentGesture, Departure, Effect,
    FileToggle, FileView, HandOffId, InFlight, Intent, LabelSwitch, LineLocation, Overlay,
    PieceLabels, PressStep, Refusal, Reopen, ReopenAfterSave, Reopening, RestartArgs, RestartGuard,
    RestartStep, Review, Screen, ScreenTravel, SettingsSheet, ToastTone, Travel, ViewSwitch,
    WindowMoment,
};
use crate::viewer::{FixReply, IssueDetails, NameHover, StationKind, WhyReply};
use issue_card::IssueHover;
use name_card_hover::NameCardPlace;

mod agent_picker;
mod comments;
mod comments_list;
mod files_list;
mod findings;
#[cfg(feature = "snapshots")]
mod gallery;
mod issue_card;
mod name_card_hover;
mod page;
#[cfg(feature = "snapshots")]
mod scenes;
mod scroll;
mod sidebar;
mod stations_view;
mod title_bar;

actions!(
    one_step,
    [
        GoForward,
        GoBack,
        HistoryBack,
        HistoryForward,
        ToggleJump,
        CopySelection,
        ToggleWrapLines,
        ToggleWhitespace,
        ToggleSettings,
        GoToNextUnread,
        FocusNextChange,
        FocusPreviousChange,
        ReadDown,
        ReadUp,
        ShowMoreAbove,
        ShowMoreBelow,
        ToggleRenderedView,
        TogglePieceLabels,
        ToggleFold,
        ToggleDraft,
        ToggleAutoMerge,
        ToggleRead,
        ToggleShortcuts,
        ToggleFindingsList,
        ToggleCommentsList,
        ToggleFilesList,
        OpenPrOnHost,
        OpenIssue,
        AskToOpenPr,
        TogglePrSwitcher,
        CloseWindow
    ]
);

use crate::app::Quit;

type OnTextSpot = Rc<dyn Fn(CodeGesture, TextSpot, &mut Window, &mut App)>;

/// What the layout talks to outside the window.
pub struct Collaborators {
    pub host: Arc<dyn CodeHost>,
    /// `None` opens all code on the host.
    pub ide: Result<Ide, NoIde>,
    pub tour_work: TourWork,
}

/// The one layout: step tree on the left, one screen on the right (the summary, or one step).
///
/// It draws from the `ReviewSession` and forwards the reader's input to it. Looks-only state (the drag
/// selection, hover) is read when drawn.
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
    /// The Ask why boxes open on findings.
    finding_questions: FindingQuestions,
    code_selection: Option<CodeSelection>,
    /// The sidebar row touching the current one that the mouse is on, so the current row squares the corners they share.
    hovered_touching_row: Option<(usize, RowPlace)>,
    /// The station of the sidebar's stepper the reviewer clicked, whose card shows over the one picked for them.
    picked_station: Option<StationKind>,
    /// The mouse is on the title bar's status chip or on the list of checks it opens.
    checks_hover: ChecksHover,
    /// The title bar lists the review's issues, as `I` does when there are several.
    issue_list_open: bool,
    issue_card_scroll: gpui::ScrollHandle,
    /// The mouse is on the issue button, a row of its list, or the card of an issue.
    issue_hover: IssueHover,
    /// The description of the issue whose card shows, prepared once for as long as the card shows it.
    issue_card_body: std::cell::RefCell<Option<(Arc<IssueDetails>, Rc<PreparedMarkdown>)>>,
    on_code_gesture: OnTextSpot,
    on_name_pointer: OnNamePointer,
    /// When the Definition card of a name in a diff line is open.
    name_hover: NameHover,
    /// Where the card goes if the pointer rests on the name it is on now.
    name_card_waits_at: gpui::Bounds<Pixels>,
    name_card_place: Option<NameCardPlace>,
    /// Counts the asks to open the finish-review form; the form opens when it sees a new one.
    finish_form_request: u32,
    /// Which way the reader went to the screen on show, so its content comes in from that side.
    screen_travel: ScreenTravel,
    /// How far each file box on the screen is scrolled sideways, by path.
    code_scroll_x: CodeScrollX,
    /// Why the last drag opened no comment box.
    selection_notice: Option<String>,
    page_drag: ScrollbarDrag,
    issue_card_drag: ScrollbarDrag,
    frame_stats: FrameStatsHook,
    #[cfg(feature = "snapshots")]
    _snapshots: Option<gpui::Task<()>>,
    jump: Entity<JumpPalette>,
    pr_switcher: Entity<PrSwitcherView>,
    shortcuts: Entity<ShortcutsSheet>,
    findings: Entity<FindingsSheet>,
    comments_list: Entity<CommentsSheet>,
    files_list: Entity<FilesSheet>,
    agent_picker: Entity<AgentPicker>,
    picture_zoom: Entity<PictureZoom>,
    settings: Entity<SettingsPanel>,
    loading: Entity<LoadingScreen>,
    /// Lives and dies with this window's session: whatever ends the session (the reopen itself, or a
    /// switch to another PR) ends a wait for it, so no state has to be carried across the root swap.
    reopen_after_save: ReopenAfterSave,
    restart_guard: RestartGuard,
    /// `None` opens all code on the host.
    ide: Result<Ide, NoIde>,
    _session_subscriptions: [Subscription; 2],
    _keys_back_to_page: Subscription,
}

impl OneStepLayout {
    pub const KEY_CONTEXT: &str = "OneStepLayout";

    pub fn new(
        review: Review,
        source: Arc<dyn CodeSource>,
        collaborators: Collaborators,
        carried_settings: Option<Entity<SettingsPanel>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let Collaborators {
            host,
            ide,
            tour_work,
        } = collaborators;
        let focus = cx.focus_handle();
        let frame_stats = FrameStatsHook::from_env();
        let session = cx.new(|cx| {
            ReviewSession::new(
                review,
                SessionWiring {
                    source,
                    host,
                    tour_work,
                    frame_stats: frame_stats.clone(),
                },
                window,
                cx,
            )
        });
        let session_subscriptions = [
            cx.observe(&session, |this, session, cx| {
                this.screen_travel.arrive(session.read(cx).review());
                this.use_code_opener_of_review(cx);
                this.reopen_if_work_ended(cx);
                this.refresh_comments_list(cx);
                this.refresh_files_list(cx);
                cx.notify();
            }),
            cx.subscribe_in(&session, window, |this, _, event, window, cx| match event {
                SessionEvent::Reopened => this.reopened(cx),
                SessionEvent::CommentBoxTakesKeysAgain => window.focus(&this.composer_focus, cx),
            }),
        ];
        // A text box that is sent, given up or saved goes, and with it what had the focus; gpui
        // then dispatches keys from the window's root, outside this layout's key context.
        let keys_back_to_page = cx.on_focus_lost(window, |this, window, cx| {
            window.focus(&this.focus, cx);
        });
        let diagrams = Self::new_diagrams(cx);
        let jump = cx.new(|cx| JumpPalette::new(focus.clone(), cx));
        let switching = cx.global::<PrSwitching>().clone();
        let loading = cx.new(|cx| LoadingScreen::new(&switching.active, cx));
        cx.observe(&switching.active, |this, _, cx| {
            this.guided_tour_follows_active_pr(cx);
            cx.notify();
        })
        .detach();
        let settings = if let Some(settings) = carried_settings {
            settings.update(cx, |settings, cx| {
                settings.move_to(focus.clone(), Placement::SheetOverReview, cx);
            });
            settings
        } else {
            let folders = switching.folders.clone();
            cx.new(|cx| SettingsPanel::new(folders, focus.clone(), Placement::SheetOverReview, cx))
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
        cx.subscribe(&pr_switcher, |this, _, DropShownReview, cx| {
            this.session.update(cx, ReviewSession::drop_review);
        })
        .detach();
        let shortcuts = cx.new(|cx| ShortcutsSheet::new(focus.clone(), cx));
        let findings = cx.new(|cx| FindingsSheet::new(focus.clone(), cx));
        cx.subscribe_in(&findings, window, |this, sheet, event, window, cx| {
            this.findings_asked(sheet, event, window, cx);
        })
        .detach();
        let comments_list = cx.new(|cx| CommentsSheet::new(focus.clone(), cx));
        cx.subscribe_in(&comments_list, window, |this, _, event, _, cx| {
            this.comments_list_asked(event, cx);
        })
        .detach();
        let files_list = cx.new(|cx| FilesSheet::new(focus.clone(), cx));
        cx.subscribe_in(&files_list, window, |this, _, event, _, cx| {
            this.files_list_asked(event, cx);
        })
        .detach();
        let agent_picker = cx.new(|cx| AgentPicker::new(focus.clone(), cx));
        cx.subscribe_in(&agent_picker, window, |this, _, event, _, cx| {
            this.agent_picker_asked(event, cx);
        })
        .detach();
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
                JumpEvent::Picked(target) => this.jump_to(target, cx),
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
        let layout_handle = cx.weak_entity();
        let on_name_pointer: OnNamePointer = Rc::new(move |pointer, _window, cx| {
            layout_handle
                .update(cx, |this, cx| this.on_name_pointer(pointer, cx))
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
            finding_questions: FindingQuestions::default(),
            code_selection: None,
            hovered_touching_row: None,
            picked_station: None,
            checks_hover: ChecksHover::default(),
            issue_list_open: false,
            issue_card_scroll: gpui::ScrollHandle::new(),
            issue_hover: IssueHover::default(),
            issue_card_body: std::cell::RefCell::new(None),
            on_code_gesture,
            on_name_pointer,
            name_hover: NameHover::default(),
            name_card_waits_at: gpui::Bounds::default(),
            name_card_place: None,
            selection_notice: None,
            page_drag: ScrollbarDrag::default(),
            issue_card_drag: ScrollbarDrag::default(),
            frame_stats,
            #[cfg(feature = "snapshots")]
            _snapshots: scenes::start(window, cx),
            jump,
            pr_switcher,
            shortcuts,
            findings,
            comments_list,
            files_list,
            agent_picker,
            picture_zoom,
            settings,
            loading,
            reopen_after_save: ReopenAfterSave::default(),
            restart_guard: RestartGuard::default(),
            ide,
            _session_subscriptions: session_subscriptions,
            _keys_back_to_page: keys_back_to_page,
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

    pub fn fix_replied(
        &mut self,
        finding: &FindingId,
        reply: &FixReply,
        cx: &mut Context<Self>,
    ) -> anyhow::Result<()> {
        self.session
            .update(cx, |session, cx| session.fix_replied(finding, reply, cx))
    }

    pub fn why_replied(
        &mut self,
        finding: &FindingId,
        reply: &WhyReply,
        cx: &mut Context<Self>,
    ) -> anyhow::Result<()> {
        self.session
            .update(cx, |session, cx| session.why_replied(finding, reply, cx))
    }

    pub fn findings_catch_up(
        &self,
        cx: &Context<Self>,
    ) -> impl FnOnce() -> anyhow::Result<()> + Send + use<> {
        self.session.read(cx).findings_catch_up()
    }

    pub fn findings_done(&mut self, cx: &mut Context<Self>) -> usize {
        self.session.update(cx, ReviewSession::findings_done)
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

    /// To the screen, scrolled to the row the jump lands on.
    fn jump_to(&mut self, target: &crate::viewer::JumpTarget, cx: &mut Context<Self>) {
        let row = self.session.update(cx, |session, cx| {
            session.change(cx, |review| review.jump_to(target));
            session.rows().row_of_jump(target)
        });
        self.screen_list.scroll_to(ListOffset {
            item_ix: row,
            offset_in_item: px(0.),
        });
    }

    fn on_row_gesture(&mut self, gesture: RowGesture, at: &LineLocation, cx: &mut Context<Self>) {
        if gesture == RowGesture::Press {
            self.selection_notice = None;
        }
        self.session.update(cx, |session, cx| match gesture {
            RowGesture::Press => {
                session.focus_change_of(at, cx);
                session.select_from(at.clone(), cx);
            }
            RowGesture::Over => session.extend_selection(at.clone(), cx),
        });
    }

    fn on_code_gesture(&mut self, gesture: CodeGesture, at: TextSpot, cx: &mut Context<Self>) {
        if gesture != CodeGesture::Over {
            self.dismiss_name_card(cx);
        }
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
    /// on the rows.
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
            Ok(Some(_)) => window.focus(&self.composer_focus, cx),
            Ok(None) => {}
            Err(refusal) => self.selection_notice = Some(refusal.to_string()),
        }
        cx.notify();
    }

    /// Shows or hides the piece labels of one file box; the key `L` does the same on the focused change's file.
    fn labels_toggle(&self, switch: &LabelSwitch, cx: &mut Context<Self>) -> Stateful<Div> {
        let colors = Colors::active(cx);
        let path = switch.path.clone();
        let labels = switch.labels;
        let segment = Segment::new(
            format!("piece-labels-on-{path}"),
            "Labels",
            cx.listener(move |this, _, _, cx| {
                this.change_review(cx, |review| {
                    review
                        .display_options_mut()
                        .show_piece_labels(&path, labels.flipped());
                });
            }),
        );
        div()
            .id(SharedString::from(format!("piece-labels-{}", switch.path)))
            .flex_none()
            .tooltip(tooltip(|_| {
                "Show or hide the piece labels of the focused change's file".into()
            }))
            .child(
                Segmented::new(
                    format!("piece-labels-{}", switch.path),
                    SegmentLayout::Row {
                        segment_width: px(64.),
                    },
                    vec![segment],
                )
                .compact()
                .picked((labels == PieceLabels::Shown).then_some(0))
                .keys(button_keys(
                    &colors,
                    ButtonKind::Ghost,
                    Command::TogglePieceLabels,
                    cx,
                )),
            )
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
                .picked(Some(picked))
                .keys(button_keys(
                    &Colors::active(cx),
                    ButtonKind::Ghost,
                    Command::ToggleRenderedView,
                    cx,
                )),
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
            .args(restart.args(&crate::app_link::build()))
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .process_group(0)
            .spawn();
        if let Err(error) = started {
            post_toast(cx, ToastTone::Danger, format!("Could not restart: {error}"));
        }
    }

    /// Whether the app may be left now: with a hand-off being sent the first ask only warns, and the
    /// same ask again soon after goes ahead.
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

    /// The app keeps what it knows of the PR's guided tour in `ActivePr`, which outlives the review; the
    /// review shows it. Nothing here moves the screen.
    fn guided_tour_follows_active_pr(&mut self, cx: &mut Context<Self>) {
        let switching = cx.global::<PrSwitching>().clone();
        let review = self.session.read(cx).review();
        if review.basic_mode_label().is_none() {
            return;
        }
        let subject = review.subject();
        let on_disk = ToursOnDisk::of(&switching.folders, subject);
        let progress = switching
            .active
            .read(cx)
            .guided_tour_progress_of(subject, on_disk);
        if review.guided_tour_progress() != &progress {
            self.session.update(cx, |session, cx| {
                session.change(cx, |review| review.guided_tour_is(progress));
            });
        }
    }

    /// Write guided tour, from the overview, the sidebar and the palette: the review takes the
    /// asking, and the app's writer hands the task to the PR's tour agent in the background.
    fn write_guided_tour(&mut self, cx: &mut Context<Self>) {
        let subject = self.session.read(cx).review().subject().clone();
        let asked = self.session.update(cx, |session, cx| {
            session.change(cx, |review| review.ask_to_write_guided_tour())
        });
        match asked {
            Ok(()) => crate::app::tour_writer::write_in_background(subject, cx),
            Err(refusal) => post_toast(cx, ToastTone::Neutral, refusal.reason().to_owned()),
        }
    }

    /// Run AI review, from the summary, the sidebar and the palette: the review takes the asking, and the
    /// session starts the review agent.
    fn run_ai_review(&mut self, cx: &mut Context<Self>) {
        let asked = self.session.update(cx, ReviewSession::run_ai_review);
        if let Err(refusal) = asked {
            post_toast(cx, ToastTone::Neutral, refusal.reason().to_owned());
        }
    }

    /// `D` and the palette's actions: the first ask says what asking again does.
    fn change_readiness(&mut self, cx: &mut Context<Self>) {
        let step = self.session.update(cx, |session, cx| {
            session.change(cx, |review| review.ask_to_change_readiness(Instant::now()))
        });
        tell_press(step, cx);
    }

    /// `Shift+M`, the title bar's button and the palette's actions: the first ask says what asking
    /// again does.
    fn change_merging(&mut self, cx: &mut Context<Self>) {
        let step = self.session.update(cx, |session, cx| {
            session.change(cx, |review| review.ask_to_change_merging(Instant::now()))
        });
        tell_press(step, cx);
    }

    /// Switch to guided tour, from the banner, the sidebar and the palette: the reviewer's click only.
    fn switch_to_guided_tour(&mut self, cx: &mut Context<Self>) {
        self.session
            .update(cx, ReviewSession::switch_to_guided_tour);
    }

    /// Opens the finish-review form in the sidebar with its body box taking the keys.
    fn open_finish_form(&mut self, cx: &mut Context<Self>) {
        self.finish_form_request = self.finish_form_request.wrapping_add(1);
        self.picked_station = Some(StationKind::YourReview);
        cx.notify();
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
            delivering_hand_offs: self.session.read(cx).is_delivering_hand_offs(),
        }
    }

    /// Called whenever a hand-off may have ended.
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
        let is_reopening = cx
            .global::<PrSwitching>()
            .active
            .read(cx)
            .is_switching_to(self.session.read(cx).review().subject());
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
    /// To the next finding, scrolled to its row; the screen's top when its row is not on the page.
    fn go_to_next_finding(&mut self, cx: &mut Context<Self>) {
        let row = self.session.update(cx, |session, cx| {
            let id = session.change(cx, |review| review.go_to_next_finding())?;
            session.rows().row_of_finding(&id)
        });
        self.screen_list.scroll_to(ListOffset {
            item_ix: row.unwrap_or(0),
            offset_in_item: px(0.),
        });
    }

    /// After the review went to a finding's screen: its row is on the page the session just built.
    fn scroll_to_finding(&mut self, id: &FindingId, cx: &mut Context<Self>) {
        let row = self.session.read(cx).rows().row_of_finding(id);
        self.screen_list.scroll_to(ListOffset {
            item_ix: row.unwrap_or(0),
            offset_in_item: px(0.),
        });
    }

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
            Effect::ToggleFocusedFile(FileToggle::RenderedView) => {
                self.change_review(cx, Review::toggle_focused_file_view);
            }
            Effect::ToggleFocusedFile(FileToggle::PieceLabels) => {
                self.change_review(cx, Review::toggle_focused_piece_labels);
            }
            Effect::ToggleScreenFold => {
                self.change_review(cx, Review::toggle_screen_fold);
            }
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
            Effect::Toggle(Overlay::Findings) => {
                let list = self.session.read(cx).review().findings_list();
                self.findings
                    .update(cx, |sheet, cx| sheet.toggle(list, window, cx));
            }
            Effect::Toggle(Overlay::Comments) => {
                let list = self.session.read(cx).review().comments_list();
                self.comments_list
                    .update(cx, |sheet, cx| sheet.toggle(list, window, cx));
            }
            Effect::Toggle(Overlay::Files) => {
                let list = self.session.read(cx).review().files_list();
                self.files_list
                    .update(cx, |sheet, cx| sheet.toggle(list, window, cx));
            }
            Effect::OpenSettings => self
                .settings
                .update(cx, |settings, cx| settings.open(window, cx)),
            Effect::OpenPrSwitcher => self
                .pr_switcher
                .update(cx, |switcher, cx| switcher.show(window, cx)),
            Effect::NextFinding => self.go_to_next_finding(cx),
            Effect::OpenFinishForm => self.open_finish_form(cx),
            Effect::SendTasks => self.session.update(cx, ReviewSession::send_tasks),
            Effect::OpenPr => self.session.update(cx, ReviewSession::open_pr),
            Effect::OpenOnHost(pr) => open_link(&pr.to_string(), cx),
            Effect::OpenChecksOnHost(pr) => open_link(&pr.checks_page(), cx),
            Effect::OpenCheck(url) => open_link(&url, cx),
            Effect::OpenIssue(link) => {
                self.issue_list_open = false;
                self.issue_hover = IssueHover::default();
                open_link(&link.to_string(), cx);
            }
            Effect::ListIssues => {
                self.issue_list_open = !self.issue_list_open;
                self.issue_hover = IssueHover::default();
                cx.notify();
            }
            Effect::ChangeReadiness => self.change_readiness(cx),
            Effect::ChangeMerging => self.change_merging(cx),
            Effect::Restart(restart) => match crate::platform::running_binary() {
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
            Effect::WriteGuidedTour => self.write_guided_tour(cx),
            Effect::RunAiReview => self.run_ai_review(cx),
            Effect::Refused(Refusal::AiReview(refusal)) => {
                post_toast(cx, ToastTone::Neutral, refusal.reason().to_owned());
            }
            Effect::SwitchToGuidedTour => self.switch_to_guided_tour(cx),
            Effect::Refused(Refusal::GuidedTour(refusal)) => {
                post_toast(cx, ToastTone::Neutral, refusal.reason().to_owned());
            }
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
        let font_id = window.text_system().resolve_font(&gpui::font(*CODE_FONT));
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
}

/// How far the wash at the top of a screen reaches down before it is all `background`.
const PAGE_WASH_HEIGHT: Pixels = px(184.);
/// The list's top padding, which the wash reaches up through to the column's top edge.
const PAGE_TOP: Pixels = px(24.);
const STICKY_STEP_BAR_HEIGHT: Pixels = px(44.);
/// The accent bar of the focused change sits in the page's side margin, against the file box's edge.
const FOCUS_BAR_WIDTH: Pixels = px(3.);
const FOCUS_BAR_LEFT: Pixels = px(15.);

/// Every row of the page column takes the column's side margins; the first one also carries the
/// wash behind the screen's heading, so the wash scrolls away with it.
/// A refused or failed save is one of the review's problems, which the sidebar shows.
/// Which of the status chip and its list of checks the mouse is on; the list shows while it is on
/// either, so the mouse can cross the gap between them.
#[derive(Default)]
struct ChecksHover {
    chip: bool,
    list: bool,
}

impl ChecksHover {
    fn is_on(&self) -> bool {
        self.chip || self.list
    }
}

/// A key that changes the PR on the host says why it does nothing, or what its next press does.
fn tell_press(step: PressStep, cx: &mut App) {
    match step {
        PressStep::Nothing | PressStep::Sent => {}
        PressStep::Refused(note) | PressStep::Ask(note) => {
            post_toast(cx, ToastTone::Neutral, note);
        }
    }
}

fn page_column_row(
    colors: &Colors,
    index: usize,
    row: AnyElement,
    is_in_focused_change: bool,
) -> AnyElement {
    let column = div()
        .relative()
        .px_6()
        .when(is_in_focused_change, |column| {
            column.child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left(FOCUS_BAR_LEFT)
                    .w(FOCUS_BAR_WIDTH)
                    .bg(colors.accent),
            )
        });
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
fn ai_review_icon(action: AiReviewAction) -> Icon {
    match action {
        AiReviewAction::TryAgain => Icon::Refresh,
        AiReviewAction::Run | AiReviewAction::RunAgain => Icon::Sparkles,
    }
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
        let name_card = self.name_card_layer(window, cx);
        let switching_veil = cx
            .global::<PrSwitching>()
            .active
            .read(cx)
            .switching_to()
            .map(SwitchingVeil::to);
        let is_writing_tour = cx
            .global::<PrSwitching>()
            .active
            .read(cx)
            .loading()
            .is_some();
        self.run_scroll_script(window, cx);
        self.note_seen_end(cx);
        self.follow_focus_to_scroll(cx);
        self.show_diagram(window, cx);
        let session = self.session.read(cx);
        let selected = Rc::new(
            session
                .review()
                .row_selection()
                .and_then(|selection| session.rows().selected_rows(selection)),
        );
        let old_head = session.old_head().cloned();
        let guided_tour_banner = session.review().guided_tour_banner();
        let repin_progress = session.repin_progress();
        let refresh_brief = session.refresh_brief().clone();
        let refresh_progress = session.refresh_progress();
        #[cfg_attr(
            not(feature = "snapshots"),
            expect(unused_mut, reason = "the gallery picture has no step")
        )]
        let mut sticky_bar = self.sticky_step_bar(cx);
        let pinned_header_top = if sticky_bar.is_some() {
            STICKY_STEP_BAR_HEIGHT
        } else {
            Pixels::ZERO
        };
        #[cfg_attr(
            not(feature = "snapshots"),
            expect(unused_mut, reason = "the gallery picture has no code")
        )]
        let mut pinned_header = self.pinned_file_header(pinned_header_top, cx);
        let screen = div().size_full().child(
            list(
                self.screen_list.clone(),
                cx.processor(move |this, index, _window, cx| {
                    let row = this.render_row(index, selected.as_ref().as_ref(), cx);
                    let is_focused = this.session.read(cx).rows().is_in_focused_change(index);
                    page_column_row(&Colors::active(cx), index, row, is_focused)
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
            pinned_header = None;
            gallery::gallery(&colors)
        } else {
            screen.into_any_element()
        };
        let screen = div()
            .relative()
            .flex_1()
            .min_h_0()
            .when(!is_writing_tour, |area| {
                area.child(screen)
                    .children(pinned_header)
                    .children(sticky_bar)
                    .children(self.page_scrollbar(cx))
            });
        div()
            .key_context(key_context.as_str())
            .track_focus(&self.focus)
            .when(self.is_dragging_a_scrollbar(), |root| {
                root.child(drag_listener(
                    cx.weak_entity(),
                    |this: &mut Self, pointer, cx| this.drag_scrollbar_to(pointer.y, cx),
                    |this: &mut Self, pointer, cx| this.end_scrollbar_drag(pointer, cx),
                ))
            })
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
            .on_action(cx.listener(|this, _: &HistoryBack, window, cx| {
                this.act(Intent::HistoryBack, window, cx);
            }))
            .on_action(cx.listener(|this, _: &HistoryForward, window, cx| {
                this.act(Intent::HistoryForward, window, cx);
            }))
            .on_mouse_down(
                MouseButton::Navigate(NavigationDirection::Back),
                cx.listener(|this, _, window, cx| this.act(Intent::HistoryBack, window, cx)),
            )
            .on_mouse_down(
                MouseButton::Navigate(NavigationDirection::Forward),
                cx.listener(|this, _, window, cx| this.act(Intent::HistoryForward, window, cx)),
            )
            .on_action(cx.listener(|this, _: &FocusNextChange, _, cx| {
                this.focus_change_by(Review::focus_next_change, cx);
            }))
            .on_action(cx.listener(|this, _: &FocusPreviousChange, _, cx| {
                this.focus_change_by(Review::focus_previous_change, cx);
            }))
            .on_action(cx.listener(|this, _: &ReadDown, _, cx| {
                this.read_toward(Review::read_down, cx);
            }))
            .on_action(cx.listener(|this, _: &ReadUp, _, cx| {
                this.read_toward(Review::read_up, cx);
            }))
            .on_action(cx.listener(|this, _: &ShowMoreAbove, _, cx| {
                this.change_review(cx, Review::show_more_above);
            }))
            .on_action(cx.listener(|this, _: &ShowMoreBelow, _, cx| {
                this.change_review(cx, Review::show_more_below);
            }))
            .on_action(cx.listener(|this, _: &ToggleRenderedView, window, cx| {
                this.act(Intent::ToggleRenderedView, window, cx);
            }))
            .on_action(cx.listener(|this, _: &TogglePieceLabels, window, cx| {
                this.act(Intent::TogglePieceLabels, window, cx);
            }))
            .on_action(cx.listener(|this, _: &ToggleDraft, window, cx| {
                this.act(Intent::ToggleDraft, window, cx);
            }))
            .on_action(cx.listener(|this, _: &ToggleAutoMerge, window, cx| {
                this.act(Intent::ToggleAutoMerge, window, cx);
            }))
            .on_action(cx.listener(|this, _: &ToggleFold, window, cx| {
                this.act(Intent::ToggleFold, window, cx);
            }))
            .on_action(cx.listener(|this, _: &ToggleWrapLines, window, cx| {
                this.act(Intent::ToggleWrapLines, window, cx);
            }))
            .on_action(cx.listener(|this, _: &ToggleWhitespace, window, cx| {
                if this.session.read(cx).rows().offers_hiding_whitespace() {
                    this.act(Intent::ToggleWhitespace, window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &ToggleJump, window, cx| {
                this.act(Intent::ToggleJump, window, cx);
            }))
            .on_action(cx.listener(|this, _: &TogglePrSwitcher, window, cx| {
                this.act(Intent::TogglePrSwitcher, window, cx);
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
            .on_action(cx.listener(|this, _: &ToggleCommentsList, window, cx| {
                this.act(Intent::ToggleCommentsList, window, cx);
            }))
            .on_action(cx.listener(|this, _: &ToggleFilesList, window, cx| {
                this.act(Intent::ToggleFilesList, window, cx);
            }))
            .on_action(cx.listener(|this, _: &ToggleFindingsList, window, cx| {
                this.act(Intent::ToggleFindingsList, window, cx);
            }))
            .on_action(cx.listener(|this, _: &ToggleShortcuts, window, cx| {
                this.act(Intent::ToggleShortcuts, window, cx);
            }))
            .on_action(cx.listener(|this, _: &OpenPrOnHost, window, cx| {
                this.act(Intent::OpenOnHost, window, cx);
            }))
            .on_action(cx.listener(|this, _: &OpenIssue, window, cx| {
                this.act(Intent::OpenIssue, window, cx);
            }))
            .on_action(cx.listener(|this, _: &AskToOpenPr, window, cx| {
                this.act(Intent::OpenPr, window, cx);
            }))
            .when(self.issue_list_open, |root| {
                root.on_key_down(cx.listener(Self::pick_issue_by_key))
            })
            .when(self.name_hover.shown().is_some(), |root| {
                root.on_key_down(cx.listener(|this, event, _, cx| {
                    this.close_name_card_by_key(event, cx);
                }))
            })
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
            .bg(if is_writing_tour {
                gpui::Background::from(colors.background)
            } else {
                colors.sidebar_ground()
            })
            .font_family(*TEXT_FONT)
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
                    .when(!is_writing_tour, |page| {
                        page.border_l_1()
                            .border_color(colors.subtle_border)
                            .shadow(colors.page_shadow())
                    })
                    .child(self.title_bar(is_writing_tour, cx))
                    .children(old_head.clone().map(|old_head| {
                        NewCommitsBanner::new(
                            old_head,
                            repin_progress,
                            cx.listener(|this, _, _, cx| {
                                this.session.update(cx, ReviewSession::repin);
                            }),
                        )
                    }))
                    .children(guided_tour_banner.map(|banner| {
                        GuidedTourReadyBanner::new(
                            banner,
                            cx.listener(|this, _, window, cx| {
                                this.act(Intent::SwitchToGuidedTour, window, cx);
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
                                    cx.listener(|this, _, window, cx| {
                                        if this.session.read(cx).tour_agent_must_be_picked() {
                                            this.open_agent_picker(
                                                crate::viewer::AgentRole::Tour,
                                                window,
                                                cx,
                                            );
                                        } else {
                                            this.session.update(cx, ReviewSession::refresh_tour);
                                        }
                                    }),
                                    cx.listener(|this, _, _, cx| {
                                        this.session.read(cx).cancel_refresh();
                                    }),
                                )
                            }),
                    )
                    .child(screen),
            )
            .child(self.settings.clone())
            .child(self.loading.clone())
            .children(switching_veil)
            .child(self.jump.clone())
            .child(self.pr_switcher.clone())
            .child(self.shortcuts.clone())
            .child(self.findings.clone())
            .child(self.comments_list.clone())
            .child(self.files_list.clone())
            .child(self.agent_picker.clone())
            .child(self.picture_zoom.clone())
            .children(name_card)
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
