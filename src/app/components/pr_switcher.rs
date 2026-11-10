use std::path::PathBuf;
use std::sync::Arc;

use gpui::prelude::FluentBuilder;
use gpui::{
    App, Context, Entity, FocusHandle, FontWeight, Global, InteractiveElement, IntoElement,
    KeyDownEvent, ParentElement, Pixels, Render, ScrollEvent, SharedString,
    StatefulInteractiveElement, Styled, Subscription, Window, div, px,
};

use crate::app::tour_writer;
use crate::pull_request::PullRequestUrl;
use crate::viewer::{
    ActivePr, Pick, PrSwitcher, RelevantPullRequestsPort, SwitcherRow, TourAgentPort, TourFact,
    WhyListed,
};

use super::chip::{chip, notice_band};
use super::icon::{Icon, sized_icon};
use super::key_cap::{KeyLabel, key_hint};
use super::motion::rise_in;
use super::overlay::{modal_layer, overlay_panel, picker_footer, picker_row};
use super::palette::{Colors, Tone};
use super::text_box::TextBox;
use super::text_input::Lines;

const ROW_HEIGHT: Pixels = px(36.);
const MIN_THUMB_HEIGHT: Pixels = px(20.);

/// What the switcher needs from the app: where the list comes from, and how to ask the app to show
/// a PR. Nothing in it belongs to one PR, so it may live as long as the app.
#[derive(Clone)]
pub struct PrSwitching {
    pub relevant: Arc<dyn RelevantPullRequestsPort>,
    pub home: PathBuf,
    pub show: Arc<dyn Fn(PullRequestUrl) + Send + Sync>,
    /// Asks the app to open the PR on screen again, after a save of the settings.
    pub reopen: Arc<dyn Fn() + Send + Sync>,
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
        let (relevant, agents) = (Arc::clone(&self.relevant), Arc::clone(&self.agents));
        let (list, active, home) = (self.list.clone(), self.active.clone(), self.home.clone());
        cx.spawn(async move |cx| {
            let (answer, link) = cx
                .background_executor()
                .spawn(async move { (relevant.relevant_pull_requests(), agents.link()) })
                .await;
            list.update(cx, |list, cx| {
                list.loaded(answer, &home);
                list.agent_link(link);
                list.check_tours(&home, active.read(cx));
                cx.notify();
            });
        })
        .detach();
    }
}

/// The overlay behind the PR title: the reviewer's relevant PRs under a query box.
pub struct PrSwitcherView {
    is_open: bool,
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
}

/// A row with its texts made when the list or the query changes, not every frame.
struct ShownRow {
    pr: SharedString,
    title: SharedString,
    author: SharedString,
    why: &'static str,
    is_draft: bool,
    tour: TourFact,
    tour_label: &'static str,
    has_unseen_reply: bool,
    is_active: bool,
}

impl ShownRow {
    fn of(row: &SwitcherRow, switcher: &PrSwitcher) -> Self {
        let pull_request = &row.pull_request;
        Self {
            pr: pull_request.url.short().into(),
            title: pull_request.title.clone().into(),
            author: pull_request.author.clone().into(),
            why: match pull_request.why_listed {
                WhyListed::Mine => "mine",
                WhyListed::ReviewRequested => "review requested",
                WhyListed::Involved => "involved",
            },
            is_draft: pull_request.is_draft,
            tour: row.tour,
            tour_label: switcher.tour_label(row),
            has_unseen_reply: row.has_unseen_reply,
            is_active: row.is_active,
        }
    }
}

impl PrSwitcherView {
    pub const KEY_CONTEXT: &str = "PrSwitcherInput";

    pub fn new(switching: PrSwitching, owner_focus: FocusHandle, cx: &mut Context<Self>) -> Self {
        let list_changes = cx.observe(&switching.list, |this, _, cx| {
            if this.is_open {
                this.refresh_shown(cx);
                cx.notify();
            }
        });
        let mut view = Self {
            is_open: false,
            query: String::new(),
            shown: Vec::new(),
            scrolled: px(0.),
            refusal: None,
            notice: None,
            switching,
            focus: cx.focus_handle(),
            owner_focus,
            _list_changes: list_changes,
        };
        view.refresh_shown(cx);
        view
    }

    pub fn is_open(&self) -> bool {
        self.is_open
    }

    pub fn open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.is_open = true;
        self.query.clear();
        self.refusal = None;
        let (home, active) = (self.switching.home.clone(), self.switching.active.clone());
        self.switching.list.update(cx, |list, cx| {
            list.filter("");
            list.check_tours(&home, active.read(cx));
            list.select_active();
        });
        self.refresh_shown(cx);
        window.focus(&self.focus, cx);
        self.switching.refresh(cx);
        cx.notify();
    }

    pub fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.is_open = false;
        window.focus(&self.owner_focus, cx);
        cx.notify();
    }

    fn refresh_shown(&mut self, cx: &Context<Self>) {
        let list = self.switching.list.read(cx);
        self.shown = list.visible().map(|row| ShownRow::of(row, list)).collect();
        self.notice = list.notice().map(Into::into);
    }

    fn pick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let selected = self.switching.list.read(cx).selected();
        match self.switching.list.read(cx).pick(selected) {
            Ok(Pick::Show(pr)) => {
                self.close(window, cx);
                self.switching.active.update(cx, |active, cx| {
                    active.picked(pr.clone());
                    cx.notify();
                });
                (self.switching.show)(pr);
            }
            Ok(Pick::Write(pr)) => {
                self.close(window, cx);
                tour_writer::write(pr, cx);
            }
            Err(refusal) => {
                self.refusal = Some(refusal.to_string().into());
                cx.notify();
            }
        }
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

    fn row(&self, index: usize, shown: &ShownRow, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = Colors::active(cx);
        let is_selected = index == self.switching.list.read(cx).selected();
        let has_tour = shown.tour == TourFact::Ready;
        let tour_color = match shown.tour {
            TourFact::Ready => colors.accent,
            TourFact::WritingFailed => colors.warning,
            TourFact::Missing | TourFact::Writing => colors.muted_text,
        };
        picker_row(
            &colors,
            SharedString::from(format!("pr-switcher-row-{index}")),
            is_selected,
        )
        .h(px(36.))
        .when(!has_tour, |row| row.text_color(colors.muted_text))
        .on_click(cx.listener(move |this, _, window, cx| {
            this.switching.list.update(cx, |list, _| list.select(index));
            this.pick(window, cx);
        }))
        .child(
            div()
                .flex_none()
                .size(px(6.))
                .rounded_full()
                .when(shown.is_active, |dot| dot.bg(colors.accent)),
        )
        .child(
            div()
                .flex_none()
                .text_xs()
                .text_color(if shown.is_active {
                    colors.accent
                } else {
                    colors.muted_text
                })
                .child(shown.pr.clone()),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .font_weight(FontWeight::MEDIUM)
                .child(shown.title.clone()),
        )
        .when(shown.is_draft, |row| {
            row.child(chip(&colors, Tone::Neutral).child("draft"))
        })
        .child(
            div()
                .flex_none()
                .text_xs()
                .text_color(colors.muted_text)
                .child(shown.author.clone()),
        )
        .child(
            div()
                .flex_none()
                .w(px(96.))
                .text_xs()
                .text_color(colors.muted_text)
                .child(shown.why),
        )
        .when(shown.is_active, |row| {
            row.child(chip(&colors, Tone::Accent).child("viewing"))
        })
        .when(shown.has_unseen_reply, |row| {
            row.child(chip(&colors, Tone::Accent).child("agent replied"))
        })
        .child(
            div()
                .flex_none()
                .w(px(88.))
                .text_xs()
                .text_color(tour_color)
                .child(shown.tour_label),
        )
    }
}

impl Render for PrSwitcherView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = Colors::active(cx);
        if !self.is_open {
            return div().into_any_element();
        }
        let list = self.switching.list.read(cx);
        let (first_shown, is_loading, is_refreshing) =
            (list.first_shown(), list.is_loading(), list.is_refreshing());
        let rows: Vec<_> = self
            .shown
            .iter()
            .enumerate()
            .skip(first_shown)
            .take(PrSwitcher::ROWS_SHOWN)
            .map(|(index, shown)| self.row(index, shown, cx).into_any_element())
            .collect();
        let scrollbar = scrollbar(first_shown, self.shown.len(), &colors);
        let empty_text = if is_loading {
            "Loading your pull requests from GitHub..."
        } else {
            "No pull request matches."
        };
        let message = self.refusal.clone().or_else(|| self.notice.clone());
        modal_layer(&colors, "pr-switcher-scrim")
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|this, _, window, cx| this.close(window, cx)),
            )
            .child(rise_in(
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
                            .children(rows)
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
            ))
            .into_any_element()
    }
}

/// A thumb on the right edge of the rows: where the rows shown sit in the whole list. It shows only
/// when some rows are out of sight.
fn scrollbar(first_shown: usize, total: usize, colors: &Colors) -> Option<impl IntoElement> {
    if total <= PrSwitcher::ROWS_SHOWN {
        return None;
    }
    let track = ROW_HEIGHT * PrSwitcher::ROWS_SHOWN as f32;
    let share = |rows: usize| track * (rows as f32 / total as f32);
    let height = share(PrSwitcher::ROWS_SHOWN).max(MIN_THUMB_HEIGHT);
    let top = px(8.) + share(first_shown).min(track - height);
    Some(
        div()
            .absolute()
            .right(px(2.))
            .top(top)
            .w(px(5.))
            .h(height)
            .rounded_full()
            .bg(colors.muted_text),
    )
}
