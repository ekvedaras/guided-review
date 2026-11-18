use crate::app_folders::AppFolders;
use std::sync::Arc;
use std::time::Instant;

use gpui::prelude::FluentBuilder;
use gpui::{
    App, Context, Entity, EventEmitter, FocusHandle, FontWeight, Global, InteractiveElement,
    IntoElement, KeyDownEvent, ParentElement, Pixels, Render, ScrollEvent, SharedString,
    StatefulInteractiveElement, Styled, Subscription, Window, div, px,
};

use crate::app::tour_writer;
use crate::config::Config;
use crate::github::GitHub;
use crate::local_change_sets::find_local_rows;
use crate::opened_subject::OpenedChangeSet;
use crate::review_subject::ReviewSubject;
use crate::viewer::{
    ActivePr, LocalRow, Pick, PrSwitcher, Presence, RelevantPullRequestsPort, RowKind, ScreenLine,
    SwitcherRow, TourAgentPort, TourFact, WhyListed, WritingFact,
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
use super::scrollbar::{ScrollThumb, TRACK_WIDTH, scrollbar};
use super::sidebar_row::RowPlace;
use super::text_box::TextBox;
use super::text_input::Lines;
use super::thread_view::avatar_circle;

/// A row is two lines high, a repo's name one: `PrSwitcher::ROW_LINES` and `HEADING_LINES`.
const LINE_HEIGHT: Pixels = px(28.);
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
    /// Wheel movement not yet enough for a whole row.
    scrolled: Pixels,
    /// Why the picked row did not open.
    refusal: Option<SharedString>,
    notice: Option<SharedString>,
    switching: PrSwitching,
    focus: FocusHandle,
    owner_focus: FocusHandle,
    _list_changes: Subscription,
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
            scrolled: px(0.),
            refusal: None,
            notice: None,
            switching,
            focus: cx.focus_handle(),
            owner_focus,
            _list_changes: list_changes,
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

    fn refresh_shown(&mut self, cx: &mut Context<Self>) {
        #[cfg(feature = "snapshots")]
        if self.staged_at_end {
            self.switching.list.update(cx, |list, _| list.select_last());
        }
        let list = self.switching.list.read(cx);
        self.shown = list.visible().map(|row| ShownRow::of(row, list)).collect();
        self.notice = list.notice().map(Into::into);
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
        self.refusal = None;
        cx.notify();
    }

    fn on_scroll(&mut self, event: &ScrollEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.scrolled += event.delta.pixel_delta(ROW_HEIGHT).y;
        while self.scrolled.abs() >= ROW_HEIGHT {
            let up = self.scrolled > px(0.);
            self.scrolled += if up { -ROW_HEIGHT } else { ROW_HEIGHT };
            let rows = if up { -1 } else { 1 };
            self.switching
                .list
                .update(cx, |list, _| list.scroll_by(rows));
        }
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

    /// The repo, or folder, whose rows follow, with how many there are; a hairline parts it from the
    /// group above.
    fn heading(name: &str, rows: usize, is_first: bool, colors: &Colors) -> impl IntoElement {
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
            )
    }
}

impl Render for PrSwitcherView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = Colors::active(cx);
        if self.presence.shown().is_none() {
            return div().into_any_element();
        }
        let list = self.switching.list.read(cx);
        let (first_shown, rows_on_screen, is_loading, is_refreshing) = (
            list.first_shown(),
            list.rows_on_screen(),
            list.is_loading(),
            list.is_refreshing(),
        );
        let screen = list.screen();
        let (selected, hovered) = (list.selected(), list.hovered());
        let row_at = |line: Option<&ScreenLine>| match line {
            Some(ScreenLine::Row(nth)) => Some(*nth),
            _ => None,
        };
        let lines: Vec<_> = screen
            .iter()
            .enumerate()
            .filter_map(|(at, line)| match line {
                ScreenLine::Heading { name, rows } => {
                    Some(Self::heading(name, *rows, at == 0, &colors).into_any_element())
                }
                ScreenLine::Row(nth) => {
                    let above = at
                        .checked_sub(1)
                        .and_then(|above| row_at(screen.get(above)));
                    let below = row_at(screen.get(at + 1));
                    let place = if *nth == selected {
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
                    self.shown.get(*nth).map(|shown| {
                        self.row(*nth, shown, (place, hovered_touching), cx)
                            .into_any_element()
                    })
                }
            })
            .collect();
        let overflows = rows_on_screen < self.shown.len();
        let scrollbar = rows_thumb(first_shown, rows_on_screen, self.shown.len()).map(|thumb| {
            scrollbar(&colors, "pr-switcher-scrollbar", thumb, false)
                .top(px(8.))
                .h(LINE_HEIGHT * PrSwitcher::SCREEN_LINES as f32)
        });
        let empty_text = if is_loading {
            "Loading your pull requests from GitHub..."
        } else {
            "No pull request matches."
        };
        let message = self.refusal.clone().or_else(|| self.notice.clone());
        let closes_on_scrim = self.place == SwitcherPlace::OverReview;
        modal_layer_for(&colors, "pr-switcher-scrim", &self.presence)
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
                    .child(
                        div()
                            .id("pr-switcher-list")
                            .relative()
                            .py_2()
                            .flex()
                            .flex_col()
                            .on_scroll(cx.listener(Self::on_scroll))
                            .when(overflows, |list| {
                                list.pr(TRACK_WIDTH - px(8.))
                                    .h(LINE_HEIGHT * PrSwitcher::SCREEN_LINES as f32 + px(16.))
                            })
                            .children(lines)
                            .when(self.shown.is_empty(), |list| {
                                list.child(
                                    div()
                                        .px_4()
                                        .py_3()
                                        .text_color(colors.muted_text)
                                        .child(empty_text),
                                )
                            })
                            .children(scrollbar),
                    )
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
                            .when(is_refreshing && !is_loading, |footer| {
                                footer.child("Refreshing from GitHub...")
                            }),
                    ),
                "pr-switcher-rise",
                &self.presence,
            ))
            .into_any_element()
    }
}

/// Where the rows on screen sit in the whole list, drawn as a thumb on the track beside the rows.
fn rows_thumb(first_shown: usize, on_screen: usize, total: usize) -> Option<ScrollThumb> {
    let track = LINE_HEIGHT * PrSwitcher::SCREEN_LINES as f32;
    let row = track / on_screen.max(1) as f32;
    ScrollThumb::of(
        track,
        row * first_shown as f32,
        row * total.saturating_sub(on_screen) as f32,
    )
}
