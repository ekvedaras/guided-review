use crate::app_folders::AppFolders;
use std::sync::Arc;
use std::time::Instant;

use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, App, Context, Entity, EventEmitter, FocusHandle, FontWeight, Global,
    InteractiveElement, IntoElement, KeyDownEvent, ListAlignment, ListOffset, ListState,
    ParentElement, Pixels, Render, SharedString, StatefulInteractiveElement, Styled, Subscription,
    Window, div, list, px,
};

use crate::app::tour_writer;
use crate::config::Config;
use crate::github::GitHub;
use crate::local_change_sets::find_local_rows;
use crate::opened_subject::OpenedChangeSet;
use crate::review_subject::ReviewSubject;
use crate::viewer::{
    ActivePr, LineAnchor, ListLine, LocalRow, Pick, PrSwitcher, Presence, PullsOfBranchPort,
    RelevantPullRequestsPort, RowKind, SwitcherRow, TourAgentPort, TourFact, WhyListed,
    WritingFact,
};

use super::button::{ButtonKind, ButtonSize, icon_label_button};
use super::chip::{chip, notice_band};
use super::icon::{Icon, sized_icon};
use super::key_cap::{KeyLabel, key_hint};
use super::motion::OVERLAY;
use super::overlay::{
    close_presence, modal_layer_for, overlay_panel, picker_footer, picker_row_at, rise_for,
};
use super::palette::{CODE_FONT, Colors, INNER_RADIUS, Tone};
use super::scrollbar::{ScrollbarDrag, TRACK_WIDTH, drag_listener, scrollbar};
use super::sidebar_row::RowPlace;
use super::text_box::TextBox;
use super::text_input::Lines;
use super::thread_view::avatar_circle;

/// A row is two lines high, a repo's name one: `PrSwitcher::ROW_LINES` and `HEADING_LINES`.
const LINE_HEIGHT: Pixels = px(28.);
/// A repo's name with the gap above it.
const HEADING_HEIGHT: Pixels = px(34.);
/// Room under the last row, so it does not touch the footer.
const END_GAP: Pixels = px(8.);
const ROW_HEIGHT: Pixels = px(56.);
const LEAD_SIZE: f32 = 30.;

/// What the switcher needs from the app: where the list comes from, and how to ask the app to show
/// a PR. Nothing in it belongs to one PR, so it may live as long as the app.
#[derive(Clone)]
pub struct PrSwitching {
    pub relevant: Arc<dyn RelevantPullRequestsPort>,
    pub folders: AppFolders,
    pub show: Arc<dyn Fn(ReviewSubject) + Send + Sync>,
    /// Asks the app to open the PR on screen again, after a save of the settings.
    pub reopen: Arc<dyn Fn() + Send + Sync>,
    /// Tells the app a review's files were deleted with Drop review, so its window shows it no more.
    pub dropped: Arc<dyn Fn(ReviewSubject) + Send + Sync>,
    /// The PR shown, the tours being written and the agent replies not seen.
    pub active: Entity<ActivePr>,
    /// Herdr, where tour agents write tours.
    pub agents: Arc<dyn TourAgentPort>,
    /// Which branch a PR is opened from, so the reviewer's agent on it writes its tour.
    pub head_branches: Arc<dyn PullsOfBranchPort>,
    /// The list, kept from one opening to the next and from one PR to the next.
    pub list: Entity<PrSwitcher>,
}

impl Global for PrSwitching {}

impl PrSwitching {
    /// Reads the list and whether herdr answers off the UI thread. The rows listed so far stay until the
    /// answers are in, and whoever shows the list is told when they are.
    pub fn refresh(&self, cx: &mut App) {
        self.list.update(cx, |list, _| list.refreshing());
        let relevant = Arc::clone(&self.relevant);
        let (list, active, folders, agents) = (
            self.list.clone(),
            self.active.clone(),
            self.folders.clone(),
            Arc::clone(&self.agents),
        );
        cx.spawn(async move |cx| {
            let answer = cx
                .background_executor()
                .spawn(async move { relevant.relevant_pull_requests() })
                .await;
            list.update(cx, |list, cx| {
                list.loaded(answer, &folders);
                list.check_tours(&folders, active.read(cx));
                cx.notify();
            });
            // After the PRs, so a slow disk never holds their rows back.
            let looked_in = folders.clone();
            let found = cx
                .background_executor()
                .spawn(async move {
                    // A config that cannot be read is Settings' to report; here it only names no clones.
                    let config = Config::load_from(&looked_in).unwrap_or_default();
                    find_local_rows(&looked_in, &config, agents.as_ref(), &GitHub::from_env())
                })
                .await;
            list.update(cx, |list, cx| {
                list.local_found(found, &folders);
                list.check_tours(&folders, active.read(cx));
                cx.notify();
            });
        })
        .detach();
    }
}

/// Where the switcher shows: over the PR on screen, or alone in the window while no PR is open yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwitcherPlace {
    OverReview,
    /// It stays open while a picked PR opens or its tour is written, as there is nothing behind it;
    /// closing it closes the window.
    WholeWindow,
}

/// The switcher was closed while it filled the window.
pub struct SwitcherDismissed;

/// The reviewer dropped the review on screen: its session deletes the files, so it writes nothing more.
pub struct DropShownReview;

/// The overlay behind the PR title: the reviewer's relevant PRs under a query box.
pub struct PrSwitcherView {
    place: SwitcherPlace,
    presence: Presence,
    query: String,
    shown: Vec<ShownRow>,
    /// The list as drawn, and what each line holds, to find it again when the list changes.
    lines: Vec<ListLine>,
    anchors: Vec<Option<LineAnchor>>,
    list_state: ListState,
    /// The line under the pointer, which stays where it is when the list changes.
    hovered_line: Option<usize>,
    /// Why the picked row did not open.
    refusal: Option<SharedString>,
    notice: Option<SharedString>,
    switching: PrSwitching,
    focus: FocusHandle,
    owner_focus: FocusHandle,
    _list_changes: Subscription,
    drag: ScrollbarDrag,
    /// A snapshot scene keeps the last row selected as the rows come in.
    #[cfg(feature = "snapshots")]
    staged_at_end: bool,
}

/// What a row says of what is reviewed, the same shape for a PR and a local review: what it is, its
/// title or branch, then a quiet line of facts.
struct RowWords {
    lead: Lead,
    title: SharedString,
    title_is_branch: bool,
    facts: SharedString,
    is_draft: bool,
}

/// What leads a row: the PR author's picture, or a folder for a local review.
enum Lead {
    Author {
        login: SharedString,
        avatar_url: Option<SharedString>,
    },
    LocalFolder,
}

/// A row with its texts made when the list or the query changes, not every frame.
struct ShownRow {
    words: RowWords,
    tour: TourFact,
    tour_label: Option<&'static str>,
    has_unseen_reply: bool,
    is_active: bool,
    /// What the folder of a local review is now, when it is no longer the snapshot.
    disk_label: Option<&'static str>,
    /// A local row whose branch has an open PR: "acme/shop#7 is open".
    open_pr: Option<SharedString>,
    offers_drop_review: bool,
}

impl ShownRow {
    fn of(row: &SwitcherRow, switcher: &PrSwitcher) -> Self {
        let words = match &row.kind {
            RowKind::PullRequest(pull_request) => RowWords {
                lead: Lead::Author {
                    login: pull_request.author.login.to_string().into(),
                    avatar_url: pull_request
                        .author
                        .avatar_url
                        .as_deref()
                        .map(|url| url.to_owned().into()),
                },
                title: pull_request.title.clone().into(),
                title_is_branch: false,
                facts: format!(
                    "#{} · {} · {}",
                    pull_request.url.number,
                    pull_request.author.login,
                    match pull_request.why_listed {
                        WhyListed::Mine => "mine",
                        WhyListed::ReviewRequested => "review requested",
                        WhyListed::Involved => "involved",
                    }
                )
                .into(),
                is_draft: pull_request.is_draft,
            },
            RowKind::Local(local) => RowWords {
                lead: Lead::LocalFolder,
                title: local.change_set.branch().as_str().to_owned().into(),
                title_is_branch: true,
                facts: std::iter::once("local review".to_owned())
                    .chain(Some(changes_of(local)).filter(|changes| !changes.is_empty()))
                    .collect::<Vec<_>>()
                    .join(" · ")
                    .into(),
                is_draft: false,
            },
        };
        Self {
            words,
            tour: row.tour,
            tour_label: switcher.tour_label(row),
            has_unseen_reply: row.has_unseen_reply,
            is_active: row.is_active,
            disk_label: row.local().and_then(LocalRow::disk_label),
            open_pr: row
                .local()
                .and_then(|local| local.open_pr.as_ref())
                .map(|pr| format!("{} is open", pr.short()).into()),
            offers_drop_review: row.local().is_some_and(LocalRow::offers_drop_review),
        }
    }
}

/// "1 commit, 2 uncommitted files": what the reviewer would find in the folder.
fn changes_of(local: &LocalRow) -> String {
    let plural =
        |count: usize, noun: &str| format!("{count} {noun}{}", if count == 1 { "" } else { "s" });
    [
        local.commits_ahead.map(|commits| plural(commits, "commit")),
        local
            .uncommitted_files
            .map(|files| format!("{} uncommitted", plural(files, "file"))),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(", ")
}

impl EventEmitter<SwitcherDismissed> for PrSwitcherView {}
impl EventEmitter<DropShownReview> for PrSwitcherView {}

impl PrSwitcherView {
    pub const KEY_CONTEXT: &str = "PrSwitcherInput";

    pub fn new(
        switching: PrSwitching,
        place: SwitcherPlace,
        owner_focus: FocusHandle,
        cx: &mut Context<Self>,
    ) -> Self {
        let list_changes = cx.observe(&switching.list, |this, _, cx| {
            if this.presence.is_open() {
                this.refresh_shown(cx);
                cx.notify();
            }
        });
        let mut view = Self {
            place,
            presence: Presence::lasting(OVERLAY),
            query: String::new(),
            shown: Vec::new(),
            lines: Vec::new(),
            anchors: Vec::new(),
            list_state: ListState::new(0, ListAlignment::Top, px(200.)).measure_all(),
            hovered_line: None,
            refusal: None,
            notice: None,
            switching,
            focus: cx.focus_handle(),
            owner_focus,
            _list_changes: list_changes,
            drag: ScrollbarDrag::default(),
            #[cfg(feature = "snapshots")]
            staged_at_end: false,
        };
        view.refresh_shown(cx);
        view
    }

    pub fn is_open(&self) -> bool {
        self.presence.is_open()
    }

    pub fn open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.presence.open(Instant::now());
        self.query.clear();
        self.refusal = None;
        let (folders, active) = (
            self.switching.folders.clone(),
            self.switching.active.clone(),
        );
        self.switching.list.update(cx, |list, cx| {
            list.filter("");
            list.check_tours(&folders, active.read(cx));
            list.select_active();
        });
        self.refresh_shown(cx);
        self.list_state.scroll_to(ListOffset::default());
        self.reveal_selected(cx);
        window.focus(&self.focus, cx);
        self.switching.refresh(cx);
        cx.notify();
    }

    /// Opens it, or leaves it as it is when it is open already.
    pub fn show(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.presence.is_open() {
            self.open(window, cx);
        }
    }

    pub fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.place == SwitcherPlace::WholeWindow {
            cx.emit(SwitcherDismissed);
            return;
        }
        if !self.presence.is_open() {
            return;
        }
        #[cfg(feature = "snapshots")]
        {
            self.staged_at_end = false;
        }
        close_presence(self, cx, |this| &mut this.presence);
        window.focus(&self.owner_focus, cx);
        cx.notify();
    }

    /// The picked PR did not open; the reviewer may pick another.
    pub fn refuse(&mut self, reason: String, cx: &mut Context<Self>) {
        self.refusal = Some(reason.into());
        cx.notify();
    }

    /// After a pick, the switcher goes away unless it is all the window holds.
    fn picked(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.place {
            SwitcherPlace::OverReview => self.close(window, cx),
            SwitcherPlace::WholeWindow => {
                let (folders, active) = (
                    self.switching.folders.clone(),
                    self.switching.active.clone(),
                );
                self.switching.list.update(cx, |list, cx| {
                    list.check_tours(&folders, active.read(cx));
                });
                self.refresh_shown(cx);
                cx.notify();
            }
        }
    }

    /// Opens it with the last row selected, which stays selected as the rows come in.
    #[cfg(feature = "snapshots")]
    pub fn open_at_end(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.staged_at_end = true;
        self.open(window, cx);
    }

    /// Takes the list's rows. When its lines changed, the line under the pointer, or else the top one,
    /// stays where it was on screen, so a click lands on the row it was aimed at.
    fn refresh_shown(&mut self, cx: &mut Context<Self>) {
        #[cfg(feature = "snapshots")]
        if self.staged_at_end {
            self.switching.list.update(cx, |list, _| list.select_last());
        }
        let scrolled = self.scrolled();
        let kept_line = self
            .hovered_line
            .filter(|line| {
                let top = top_of(&self.lines, *line);
                top >= scrolled && top < scrolled + self.list_height()
            })
            .unwrap_or_else(|| self.list_state.logical_scroll_top().item_ix);
        let kept = self
            .anchors
            .get(kept_line)
            .cloned()
            .flatten()
            .map(|anchor| (anchor, top_of(&self.lines, kept_line) - scrolled));
        let list = self.switching.list.read(cx);
        self.shown = list.visible().map(|row| ShownRow::of(row, list)).collect();
        self.notice = list.notice().map(Into::into);
        let lines = list.lines();
        if lines == self.lines {
            return;
        }
        let line_now = kept
            .as_ref()
            .map(|(anchor, on_screen)| (list.line_after_change(anchor, kept_line), *on_screen));
        self.anchors = (0..lines.len()).map(|line| list.anchor_at(line)).collect();
        self.lines = lines;
        self.hovered_line = None;
        self.list_state.reset(self.lines.len());
        if let Some((line, on_screen)) = line_now {
            self.scroll_to_px(top_of(&self.lines, line) - on_screen);
        }
        #[cfg(feature = "snapshots")]
        if self.staged_at_end {
            self.reveal_selected(cx);
        }
    }

    /// How far the list is scrolled, from its lines' fixed heights, so it is right before gpui has
    /// measured them.
    fn scrolled(&self) -> Pixels {
        let top = self.list_state.logical_scroll_top();
        top_of(&self.lines, top.item_ix) + top.offset_in_item
    }

    fn scroll_to_px(&self, scrolled: Pixels) {
        let most = (content_height(&self.lines) + END_GAP - self.list_height()).max(px(0.));
        self.list_state
            .scroll_to(offset_at(&self.lines, scrolled.clamp(px(0.), most)));
    }

    /// The list's height: all of it, up to eight rows and a repo's name.
    fn list_height(&self) -> Pixels {
        (content_height(&self.lines) + END_GAP).min(ROW_HEIGHT * 8. + HEADING_HEIGHT)
    }

    /// Scrolls only as far as the selected row needs to be in full sight, below the repo name that
    /// stays at the top.
    fn reveal_selected(&mut self, cx: &mut Context<Self>) {
        let list = self.switching.list.read(cx);
        let Some(line) = list.line_of_row(list.selected()) else {
            return;
        };
        let (top, scrolled, height) = (
            top_of(&self.lines, line),
            self.scrolled(),
            self.list_height(),
        );
        let bottom = top
            + ROW_HEIGHT
            + if line + 1 == self.lines.len() {
                END_GAP
            } else {
                px(0.)
            };
        if top - HEADING_HEIGHT < scrolled {
            self.scroll_to_px(top - HEADING_HEIGHT);
        } else if bottom > scrolled + height {
            self.scroll_to_px(bottom - height);
        }
    }

    fn pick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.switching.active.read(cx).switching_to().is_some() {
            return;
        }
        let selected = self.switching.list.read(cx).selected();
        match self.switching.list.read(cx).pick(selected) {
            Ok(Pick::Show(pr)) => {
                self.picked(window, cx);
                self.switching.active.update(cx, |active, cx| {
                    active.picked(pr.clone());
                    cx.notify();
                });
                (self.switching.show)(pr);
            }
            Ok(Pick::Write(pr)) => {
                tour_writer::write(pr, cx);
                self.picked(window, cx);
            }
            Err(refusal) => {
                self.refusal = Some(refusal.to_string().into());
                cx.notify();
            }
        }
    }

    /// Drop review of the `nth` row, whose branch is gone: its files go off the UI thread, then its row.
    fn drop_review(&mut self, nth: usize, cx: &mut Context<Self>) {
        let Some(subject) = self.switching.list.read(cx).drop_offer(nth) else {
            return;
        };
        let Some(change_set) = subject.local().cloned() else {
            return;
        };
        if self.switching.active.read(cx).is_shown(&subject) {
            cx.emit(DropShownReview);
            return;
        }
        let (folders, list, dropped) = (
            self.switching.folders.clone(),
            self.switching.list.clone(),
            Arc::clone(&self.switching.dropped),
        );
        cx.spawn(async move |this, cx| {
            let dropping = change_set.clone();
            let result = cx
                .background_executor()
                .spawn(async move { OpenedChangeSet::new(&dropping, &folders).drop_review() })
                .await;
            this.update(cx, |this, cx| match result {
                Ok(()) => {
                    list.update(cx, |list, cx| {
                        list.dropped(&subject);
                        cx.notify();
                    });
                    dropped(subject);
                }
                Err(error) => this.refuse(format!("Could not drop the review: {error:#}"), cx),
            })
            .ok();
        })
        .detach();
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        match event.keystroke.key.as_str() {
            "down" => self.switching.list.update(cx, |list, _| list.select_next()),
            "up" => self
                .switching
                .list
                .update(cx, |list, _| list.select_previous()),
            _ => return,
        }
        self.reveal_selected(cx);
        self.refusal = None;
        cx.notify();
    }

    fn query_box(&self, cx: &mut Context<Self>) -> TextBox {
        let (on_edit, on_submit, on_cancel) = (cx.entity(), cx.entity(), cx.entity());
        TextBox::new("pr-switcher-query", &self.focus, &self.query, Lines::Single)
            .on_edit(move |query, _, cx| {
                on_edit.update(cx, |this, cx| {
                    this.switching
                        .list
                        .update(cx, |list, _| list.filter(&query));
                    this.query = query;
                    this.refusal = None;
                    this.refresh_shown(cx);
                    this.list_state.scroll_to(ListOffset::default());
                    cx.notify();
                });
            })
            .on_submit(move |window, cx| on_submit.update(cx, |this, cx| this.pick(window, cx)))
            .on_cancel(move |window, cx| on_cancel.update(cx, |this, cx| this.close(window, cx)))
    }

    fn row(
        &self,
        index: usize,
        shown: &ShownRow,
        place: (RowPlace, Option<RowPlace>),
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let colors = Colors::active(cx);
        let is_writing = shown.tour.writing != WritingFact::None;
        let lead = match &shown.words.lead {
            Lead::Author { login, avatar_url } => {
                avatar_circle(&colors, login, avatar_url.as_deref(), LEAD_SIZE, cx)
            }
            Lead::LocalFolder => div()
                .flex_none()
                .size(px(LEAD_SIZE))
                .rounded(px(INNER_RADIUS))
                .bg(colors.inset)
                .border_1()
                .border_color(colors.subtle_border)
                .flex()
                .items_center()
                .justify_center()
                .child(sized_icon(Icon::FolderGit, colors.muted_text, px(14.)))
                .into_any_element(),
        };
        let title = div()
            .min_w_0()
            .truncate()
            .line_height(px(20.))
            .font_weight(FontWeight::MEDIUM)
            .text_color(if is_writing {
                colors.muted_text
            } else {
                colors.text
            })
            .when(shown.words.title_is_branch, |title| {
                title.font_family(*CODE_FONT).text_size(px(13.))
            })
            .child(shown.words.title.clone());
        let facts = div()
            .min_w_0()
            .truncate()
            .line_height(px(16.))
            .text_xs()
            .text_color(colors.muted_text)
            .child(shown.words.facts.clone());
        let (place, hovered_touching) = place;
        picker_row_at(
            &colors,
            SharedString::from(format!("pr-switcher-row-{index}")),
            place,
            hovered_touching,
        )
        .h(ROW_HEIGHT)
        .px_3()
        .on_hover(cx.listener(move |this, is_hovered: &bool, _, cx| {
            let line = this
                .lines
                .iter()
                .position(|line| *line == ListLine::Row(index));
            if *is_hovered {
                this.hovered_line = line;
            } else if this.hovered_line == line {
                this.hovered_line = None;
            }
            this.switching.list.update(cx, |list, _| {
                if *is_hovered {
                    list.hover(index);
                } else {
                    list.unhover(index);
                }
            });
        }))
        .on_click(cx.listener(move |this, _, window, cx| {
            this.switching.list.update(cx, |list, _| list.select(index));
            this.pick(window, cx);
        }))
        .child(lead)
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(title)
                .child(facts),
        )
        .child(self.states(index, shown, &colors, cx))
    }

    /// The row's states, always in this order at its right edge, so the same fact is in the same place
    /// on every row.
    fn states(
        &self,
        index: usize,
        shown: &ShownRow,
        colors: &Colors,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let tour_tone = match shown.tour.writing {
            WritingFact::Failed => Tone::Warning,
            WritingFact::None | WritingFact::Writing => Tone::Neutral,
        };
        div()
            .flex_none()
            .flex()
            .items_center()
            .gap_1p5()
            .when(shown.words.is_draft, |states| {
                states.child(chip(colors, Tone::Neutral).child("draft"))
            })
            .children(
                shown
                    .open_pr
                    .clone()
                    .map(|label| chip(colors, Tone::Neutral).child(label)),
            )
            .children(
                shown
                    .disk_label
                    .map(|label| chip(colors, Tone::Warning).child(label)),
            )
            .when(shown.has_unseen_reply, |states| {
                states.child(chip(colors, Tone::Accent).child("agent replied"))
            })
            .when(shown.is_active, |states| {
                states.child(chip(colors, Tone::Accent).child("viewing"))
            })
            .children(shown.tour_label.map(|label| {
                let is_guided = shown.tour.writing == WritingFact::None;
                chip(colors, tour_tone)
                    .when(is_guided, |chip| {
                        chip.child(sized_icon(Icon::Sparkles, colors.accent, px(12.)))
                    })
                    .child(label)
            }))
            .when(shown.offers_drop_review, |states| {
                states.child(
                    icon_label_button(
                        colors,
                        SharedString::from(format!("pr-switcher-drop-{index}")),
                        ButtonKind::Secondary,
                        ButtonSize::Small,
                        Icon::Trash,
                        "Drop review",
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.drop_review(index, cx);
                    })),
                )
            })
    }

    /// The repo, or folder, whose rows follow, with how many there are. A gap above parts it from the
    /// rows of the repo before, and a hairline from the second repo on.
    fn heading(name: &str, rows: usize, is_first: bool, colors: &Colors) -> impl IntoElement {
        div()
            .h(HEADING_HEIGHT)
            .pt(HEADING_HEIGHT - LINE_HEIGHT)
            .child(
                div()
                    .h(LINE_HEIGHT)
                    .mx_2()
                    .px(px(13.))
                    .flex()
                    .items_end()
                    .gap_2()
                    .pb(px(5.))
                    .when(!is_first, |heading| {
                        heading.border_t_1().border_color(colors.subtle_border)
                    })
                    .text_xs()
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(colors.text)
                            .child(SharedString::from(name.to_owned())),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_color(colors.muted_text)
                            .child(SharedString::from(rows.to_string())),
                    ),
            )
    }
}

impl PrSwitcherView {
    /// What line `ix` of the list draws: a repo's name, or a row joined to the selected one beside it.
    fn line_element(&mut self, ix: usize, cx: &mut Context<Self>) -> AnyElement {
        div()
            .w_full()
            .flex()
            .flex_col()
            .child(self.line_content(ix, cx))
            .into_any_element()
    }

    fn line_content(&mut self, ix: usize, cx: &mut Context<Self>) -> AnyElement {
        let colors = Colors::active(cx);
        let row_at = |line: Option<&ListLine>| match line {
            Some(ListLine::Row(nth)) => Some(*nth),
            _ => None,
        };
        match self.lines.get(ix).cloned() {
            Some(ListLine::Heading { name, rows }) => {
                Self::heading(&name, rows, ix == 0, &colors).into_any_element()
            }
            Some(ListLine::Row(nth)) => {
                let list = self.switching.list.read(cx);
                let (selected, hovered) = (list.selected(), list.hovered());
                let above = ix
                    .checked_sub(1)
                    .and_then(|above| row_at(self.lines.get(above)));
                let below = row_at(self.lines.get(ix + 1));
                let place = if nth == selected {
                    RowPlace::Current
                } else if below == Some(selected) {
                    RowPlace::JustAboveCurrent
                } else if above == Some(selected) {
                    RowPlace::JustBelowCurrent
                } else {
                    RowPlace::Apart
                };
                let hovered_touching = match hovered {
                    Some(hovered) if hovered != selected && above == Some(hovered) => {
                        Some(RowPlace::JustAboveCurrent)
                    }
                    Some(hovered) if hovered != selected && below == Some(hovered) => {
                        Some(RowPlace::JustBelowCurrent)
                    }
                    _ => None,
                };
                match self.shown.get(nth) {
                    Some(shown) => self
                        .row(nth, shown, (place, hovered_touching), cx)
                        .into_any_element(),
                    None => div().into_any_element(),
                }
            }
            None => div().into_any_element(),
        }
    }

    /// The name of the repo whose rows are at the top, stuck there while they scroll under it, and
    /// pushed up by the next repo's name as it arrives.
    fn sticky_heading(&self, colors: &Colors) -> Option<impl IntoElement + use<>> {
        let top = self.list_state.logical_scroll_top().item_ix;
        let (name, rows) = self
            .lines
            .get(..=top)?
            .iter()
            .rev()
            .find_map(|line| match line {
                ListLine::Heading { name, rows } => Some((name.clone(), *rows)),
                ListLine::Row(_) => None,
            })?;
        let next = self
            .lines
            .iter()
            .enumerate()
            .skip(top + 1)
            .find(|(_, line)| matches!(line, ListLine::Heading { .. }))
            .map(|(line, _)| top_of(&self.lines, line) - self.scrolled());
        let pushed = next.map_or(px(0.), |next_top| (next_top - HEADING_HEIGHT).min(px(0.)));
        Some(
            div()
                .absolute()
                .left_0()
                .right_0()
                .top(pushed)
                .bg(colors.overlay)
                .child(Self::heading(&name, rows, true, colors)),
        )
    }
}

impl Render for PrSwitcherView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = Colors::active(cx);
        if self.presence.shown().is_none() {
            return div().into_any_element();
        }
        let switcher = self.switching.list.read(cx);
        let (is_loading, is_refreshing, is_finding_local) = (
            switcher.is_loading(),
            switcher.is_refreshing(),
            switcher.is_finding_local(),
        );
        let busy: Vec<&str> = [
            (is_refreshing && !is_loading).then_some("Refreshing from GitHub..."),
            is_finding_local.then_some("Looking in your folders..."),
        ]
        .into_iter()
        .flatten()
        .collect();
        let overflows = content_height(&self.lines) + END_GAP > self.list_height();
        let is_dragging = self.drag.is_dragging();
        let scrollbar = ScrollbarDrag::thumb(&self.list_state)
            .filter(|_| overflows)
            .map(|thumb| {
                scrollbar(&colors, "pr-switcher-scrollbar", thumb, is_dragging)
                    .top_0()
                    .bottom_0()
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(|this, event: &gpui::MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            this.drag.press(&this.list_state, event.position.y);
                            cx.notify();
                        }),
                    )
            });
        let sticky = self.sticky_heading(&colors).filter(|_| overflows);
        let empty_text = if is_loading {
            "Loading your pull requests from GitHub..."
        } else {
            "No pull request matches."
        };
        let message = self.refusal.clone().or_else(|| self.notice.clone());
        let closes_on_scrim = self.place == SwitcherPlace::OverReview;
        let rows = if self.shown.is_empty() {
            div()
                .px_4()
                .py_3()
                .text_color(colors.muted_text)
                .child(empty_text)
                .into_any_element()
        } else {
            div()
                .id("pr-switcher-list")
                .relative()
                .h(self.list_height())
                .overflow_hidden()
                .when(overflows, |list| list.pr(TRACK_WIDTH - px(8.)))
                .child(
                    list(
                        self.list_state.clone(),
                        cx.processor(|this, ix, _window, cx| this.line_element(ix, cx)),
                    )
                    .pb(END_GAP)
                    .size_full(),
                )
                .children(sticky)
                .children(scrollbar)
                .into_any_element()
        };
        modal_layer_for(&colors, "pr-switcher-scrim", &self.presence)
            .when(is_dragging, |layer| {
                layer.child(drag_listener(
                    cx.weak_entity(),
                    |this: &mut Self, pointer, cx| {
                        this.drag.drag_to(&this.list_state, pointer.y);
                        cx.notify();
                    },
                    |this: &mut Self, _, cx| {
                        this.drag.release(&this.list_state);
                        cx.notify();
                    },
                ))
            })
            .when(closes_on_scrim, |scrim| {
                scrim.on_mouse_down(
                    gpui::MouseButton::Left,
                    cx.listener(|this, _, window, cx| this.close(window, cx)),
                )
            })
            .child(rise_for(
                overlay_panel(&colors)
                    .id("pr-switcher")
                    .key_context(Self::KEY_CONTEXT)
                    .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_key_down(cx.listener(Self::on_key_down))
                    .mt(px(80.))
                    .w(px(860.))
                    .h_auto()
                    .child(
                        div()
                            .px_4()
                            .py_3()
                            .flex()
                            .items_center()
                            .gap_3()
                            .text_base()
                            .border_b_1()
                            .border_color(colors.subtle_border)
                            .child(sized_icon(Icon::Search, colors.muted_text, px(16.)))
                            .child(div().flex_1().min_w_0().child(self.query_box(cx))),
                    )
                    .child(rows)
                    .children(message.map(|message| {
                        notice_band(&colors, Tone::Warning)
                            .px_4()
                            .border_t_1()
                            .text_xs()
                            .child(message)
                    }))
                    .child(
                        picker_footer(&colors)
                            .child(key_hint(
                                &colors,
                                &[
                                    KeyLabel::Icon(Icon::ArrowUp),
                                    KeyLabel::Icon(Icon::ArrowDown),
                                ],
                                "move",
                            ))
                            .child(key_hint(&colors, &[KeyLabel::Icon(Icon::Enter)], "open"))
                            .child(key_hint(&colors, &[KeyLabel::Text("esc")], "close"))
                            .child(div().flex_1())
                            .when(!busy.is_empty(), |footer| footer.child(busy.join(" · "))),
                    ),
                "pr-switcher-rise",
                &self.presence,
            ))
            .into_any_element()
    }
}

/// A line's height: every row and every repo name is drawn at a fixed height, so where a line sits is
/// known before gpui measures it.
fn line_height(line: &ListLine) -> Pixels {
    match line {
        ListLine::Heading { .. } => HEADING_HEIGHT,
        ListLine::Row(_) => ROW_HEIGHT,
    }
}

fn top_of(lines: &[ListLine], line: usize) -> Pixels {
    lines
        .iter()
        .take(line)
        .map(line_height)
        .fold(px(0.), |sum, height| sum + height)
}

fn content_height(lines: &[ListLine]) -> Pixels {
    top_of(lines, lines.len())
}

/// The line `scrolled` falls in, and how far into it.
fn offset_at(lines: &[ListLine], scrolled: Pixels) -> ListOffset {
    let mut top = px(0.);
    for (item_ix, line) in lines.iter().enumerate() {
        let height = line_height(line);
        if scrolled < top + height {
            return ListOffset {
                item_ix,
                offset_in_item: scrolled - top,
            };
        }
        top += height;
    }
    ListOffset {
        item_ix: lines.len().saturating_sub(1),
        offset_in_item: px(0.),
    }
}
