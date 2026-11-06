use std::path::PathBuf;
use std::sync::Arc;

use gpui::prelude::FluentBuilder;
use gpui::{
    Context, Entity, FocusHandle, FontWeight, Global, InteractiveElement, IntoElement,
    KeyDownEvent, ParentElement, Render, SharedString, StatefulInteractiveElement, Styled, Window,
    div, px,
};

use crate::app::tour_writer;
use crate::pull_request::PullRequestUrl;
use crate::viewer::{
    ActivePr, Pick, PrSwitcher, RelevantPullRequestsPort, SwitcherRow, TourAgentPort, TourFact,
    WhyListed,
};

use super::palette::{Colors, TEXT_FONT};
use super::text_box::TextBox;
use super::text_input::Lines;

const VISIBLE_ROWS: usize = 10;

/// What the switcher needs from the app: where the list comes from, and how to ask the app to show
/// a PR. Nothing in it belongs to one PR, so it may live as long as the app.
#[derive(Clone)]
pub struct PrSwitching {
    pub relevant: Arc<dyn RelevantPullRequestsPort>,
    pub home: PathBuf,
    pub show: Arc<dyn Fn(PullRequestUrl) + Send + Sync>,
    /// The PR shown, the tours being written and the agent replies not seen.
    pub active: Entity<ActivePr>,
    /// Herdr, where tour agents write tours.
    pub agents: Arc<dyn TourAgentPort>,
}

impl Global for PrSwitching {}

/// The overlay behind the PR title: the reviewer's relevant PRs under a query box.
pub struct PrSwitcherView {
    is_open: bool,
    core: PrSwitcher,
    query: String,
    shown: Vec<ShownRow>,
    selected: usize,
    /// Why the picked row did not open.
    refusal: Option<SharedString>,
    notice: Option<SharedString>,
    switching: PrSwitching,
    focus: FocusHandle,
    owner_focus: FocusHandle,
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
        }
    }
}

impl PrSwitcherView {
    pub const KEY_CONTEXT: &str = "PrSwitcherInput";

    pub fn new(switching: PrSwitching, owner_focus: FocusHandle, cx: &mut Context<Self>) -> Self {
        Self {
            is_open: false,
            core: PrSwitcher::default(),
            query: String::new(),
            shown: Vec::new(),
            selected: 0,
            refusal: None,
            notice: None,
            switching,
            focus: cx.focus_handle(),
            owner_focus,
        }
    }

    pub fn is_open(&self) -> bool {
        self.is_open
    }

    pub fn open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.is_open = true;
        self.query.clear();
        self.core.filter("");
        self.check_tours(cx);
        self.refusal = None;
        self.refresh_shown();
        window.focus(&self.focus, cx);
        self.load(cx);
        cx.notify();
    }

    pub fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.is_open = false;
        window.focus(&self.owner_focus, cx);
        cx.notify();
    }

    fn check_tours(&mut self, cx: &Context<Self>) {
        let active = self.switching.active.read(cx);
        self.core.check_tours(&self.switching.home, active);
    }

    /// The list and whether herdr answers are read off the UI thread; what was shown stays until the
    /// answers are in.
    fn load(&self, cx: &mut Context<Self>) {
        let relevant = Arc::clone(&self.switching.relevant);
        let agents = Arc::clone(&self.switching.agents);
        cx.spawn(async move |this, cx| {
            let (answer, link) = cx
                .background_executor()
                .spawn(async move { (relevant.relevant_pull_requests(), agents.link()) })
                .await;
            this.update(cx, |this, cx| {
                this.core.loaded(answer, &this.switching.home);
                this.core.agent_link(link);
                this.check_tours(cx);
                this.refresh_shown();
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn refresh_shown(&mut self) {
        self.shown = self
            .core
            .visible()
            .map(|row| ShownRow::of(row, &self.core))
            .collect();
        self.selected = self.selected.min(self.shown.len().saturating_sub(1));
        self.notice = self.core.notice().map(Into::into);
    }

    fn pick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.core.pick(self.selected) {
            Ok(Pick::Show(pr)) => {
                self.close(window, cx);
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
            "down" => self.selected = (self.selected + 1).min(self.shown.len().saturating_sub(1)),
            "up" => self.selected = self.selected.saturating_sub(1),
            _ => return,
        }
        self.refusal = None;
        cx.notify();
    }

    fn query_box(&self, cx: &mut Context<Self>) -> TextBox {
        let (on_edit, on_submit, on_cancel) = (cx.entity(), cx.entity(), cx.entity());
        TextBox::new("pr-switcher-query", &self.focus, &self.query, Lines::Single)
            .on_edit(move |query, _, cx| {
                on_edit.update(cx, |this, cx| {
                    this.core.filter(&query);
                    this.query = query;
                    this.selected = 0;
                    this.refusal = None;
                    this.refresh_shown();
                    cx.notify();
                });
            })
            .on_submit(move |window, cx| on_submit.update(cx, |this, cx| this.pick(window, cx)))
            .on_cancel(move |window, cx| on_cancel.update(cx, |this, cx| this.close(window, cx)))
    }

    fn row(&self, index: usize, shown: &ShownRow, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = Colors::active(cx);
        let is_selected = index == self.selected;
        let has_tour = shown.tour == TourFact::Ready;
        let tour_color = match shown.tour {
            TourFact::Ready => colors.accent,
            TourFact::WritingFailed => colors.warning,
            TourFact::Missing | TourFact::Writing => colors.muted_text,
        };
        div()
            .id(SharedString::from(format!("pr-switcher-row-{index}")))
            .mx_2()
            .px_2()
            .h(px(36.))
            .flex()
            .items_center()
            .gap_3()
            .rounded_md()
            .overflow_hidden()
            .cursor_pointer()
            .when(!has_tour, |row| row.text_color(colors.muted_text))
            .map(|row| {
                if is_selected {
                    row.bg(colors.current)
                } else {
                    row.hover(|row| row.bg(colors.hover))
                }
            })
            .on_click(cx.listener(move |this, _, window, cx| {
                this.selected = index;
                this.pick(window, cx);
            }))
            .child(
                div()
                    .flex_none()
                    .text_xs()
                    .text_color(colors.muted_text)
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
                row.child(
                    div()
                        .flex_none()
                        .text_xs()
                        .text_color(colors.muted_text)
                        .child("draft"),
                )
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
            .when(shown.has_unseen_reply, |row| {
                row.child(
                    div()
                        .flex_none()
                        .text_xs()
                        .text_color(colors.accent)
                        .child("agent replied"),
                )
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
        let first_shown = self.selected.saturating_sub(VISIBLE_ROWS - 1);
        let rows: Vec<_> = self
            .shown
            .iter()
            .enumerate()
            .skip(first_shown)
            .take(VISIBLE_ROWS)
            .map(|(index, shown)| self.row(index, shown, cx).into_any_element())
            .collect();
        let empty_text = if self.core.is_loading() {
            "Loading your pull requests from GitHub..."
        } else {
            "No pull request matches."
        };
        let message = self.refusal.clone().or_else(|| self.notice.clone());
        div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .flex()
            .items_start()
            .justify_center()
            .bg(gpui::rgba(0x00000066))
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|this, _, window, cx| this.close(window, cx)),
            )
            .child(
                div()
                    .id("pr-switcher")
                    .key_context(Self::KEY_CONTEXT)
                    .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_key_down(cx.listener(Self::on_key_down))
                    .mt(px(80.))
                    .w(px(860.))
                    .h_auto()
                    .flex()
                    .flex_col()
                    .overflow_hidden()
                    .bg(colors.background)
                    .border_1()
                    .border_color(colors.border)
                    .rounded_lg()
                    .shadow_lg()
                    .font_family(TEXT_FONT)
                    .text_sm()
                    .text_color(colors.text)
                    .child(
                        div()
                            .px_4()
                            .py_3()
                            .text_base()
                            .border_b_1()
                            .border_color(colors.border)
                            .child(self.query_box(cx)),
                    )
                    .child(div().py_2().flex().flex_col().children(rows).when(
                        self.shown.is_empty(),
                        |list| {
                            list.child(
                                div()
                                    .px_4()
                                    .py_3()
                                    .text_color(colors.muted_text)
                                    .child(empty_text),
                            )
                        },
                    ))
                    .children(message.map(|message| {
                        div()
                            .px_4()
                            .py_2()
                            .text_xs()
                            .bg(colors.warning_background)
                            .text_color(colors.warning)
                            .child(message)
                    }))
                    .child(
                        div()
                            .px_4()
                            .py_2()
                            .flex()
                            .gap_4()
                            .border_t_1()
                            .border_color(colors.subtle_border)
                            .bg(colors.surface)
                            .rounded_b_md()
                            .text_xs()
                            .text_color(colors.muted_text)
                            .child("↑ ↓  move")
                            .child("↵  open")
                            .child("esc  close"),
                    ),
            )
            .into_any_element()
    }
}
