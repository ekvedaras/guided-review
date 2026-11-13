use gpui::prelude::FluentBuilder;
use gpui::{
    Context, Entity, FontWeight, InteractiveElement, IntoElement, ParentElement, Render,
    SharedString, StatefulInteractiveElement, Styled, Window, div, px,
};

use super::button::{ButtonKind, ButtonSize, icon_label_button};
use super::icon::Icon;
use super::motion::{OVERLAY, busy_dot, busy_sweep, fade_in};
use super::palette::{CARD_RADIUS, CODE_FONT, Colors, SIDEBAR_WIDTH, TEXT_FONT};
use super::title_bar::TITLE_BAR_HEIGHT;
use crate::app::tour_writer;
use crate::pull_request::PullRequestUrl;
use crate::viewer::{ActivePr, AgentStatus, TourWriting, WritingStage};

/// What the window shows while a tour agent writes the tour of the PR the reviewer picked: its status,
/// its last screen lines and, when it ended without a tour, why, with Try again and Show agent.
pub struct LoadingScreen {
    shown: Option<Shown>,
}

/// The texts, made when the writing changes, not every frame.
struct Shown {
    pr: PullRequestUrl,
    title: SharedString,
    status: SharedString,
    failure: Option<SharedString>,
    lines: Vec<SharedString>,
    back_to: Option<SharedString>,
}

impl LoadingScreen {
    pub fn new(active: &Entity<ActivePr>, cx: &mut Context<Self>) -> Self {
        cx.observe(active, |this, active, cx| {
            this.shown = Shown::of(active.read(cx));
            cx.notify();
        })
        .detach();
        Self {
            shown: Shown::of(active.read(cx)),
        }
    }
}

impl Shown {
    fn of(active: &ActivePr) -> Option<Self> {
        let (pr, writing) = active.loading()?;
        Some(Self {
            pr: pr.clone(),
            title: format!("Writing the tour of {}", pr.short()).into(),
            status: status_line(writing).into(),
            failure: match &writing.stage {
                WritingStage::Failed(reason) => Some(reason.clone().into()),
                WritingStage::Starting | WritingStage::Writing => None,
            },
            lines: writing.screen.iter().cloned().map(Into::into).collect(),
            back_to: active
                .shown()
                .map(|shown| format!("Back to {}", shown.short()).into()),
        })
    }
}

fn status_line(writing: &TourWriting) -> String {
    let Some(terminal) = &writing.terminal else {
        return "Starting a tour agent in herdr...".to_owned();
    };
    match writing.status {
        None | Some(AgentStatus::Unreported) => format!("Tour agent {terminal}: starting"),
        Some(AgentStatus::WaitingForYou) => format!(
            "Tour agent {terminal} waits for you in its terminal: show the agent to answer it"
        ),
        Some(status) => format!("Tour agent {terminal}: {status}"),
    }
}

impl Render for LoadingScreen {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(shown) = &self.shown else {
            return div().into_any_element();
        };
        let colors = Colors::active(cx);
        let (show_pr, retry_pr) = (shown.pr.clone(), shown.pr.clone());
        let actions = div()
            .flex()
            .gap_2()
            .when(shown.failure.is_some(), |actions| {
                actions.child(
                    icon_label_button(
                        &colors,
                        "loading-try-again",
                        ButtonKind::Primary,
                        ButtonSize::Regular,
                        Icon::Refresh,
                        "Try again",
                    )
                    .on_click(move |_, _, cx| tour_writer::try_again(retry_pr.clone(), cx)),
                )
            })
            .child(
                icon_label_button(
                    &colors,
                    "loading-show-agent",
                    ButtonKind::Secondary,
                    ButtonSize::Regular,
                    Icon::Terminal,
                    "Show agent",
                )
                .on_click(move |_, _, cx| tour_writer::show_agent(show_pr.clone(), cx)),
            )
            .children(shown.back_to.clone().map(|back_to| {
                icon_label_button(
                    &colors,
                    "loading-back",
                    ButtonKind::Ghost,
                    ButtonSize::Regular,
                    Icon::ArrowLeft,
                    back_to,
                )
                .on_click(|_, _, cx| tour_writer::stop_waiting(cx))
            }));
        let is_busy = shown.failure.is_none();
        let status = div()
            .flex()
            .items_center()
            .gap_2()
            .text_sm()
            .text_color(colors.muted_text)
            .when(is_busy, |status| {
                status.child(busy_dot(&colors, "loading-dot"))
            })
            .child(shown.status.clone());
        let page = div()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .items_center()
            .pt(px(80.))
            .px_6()
            .gap_4()
            .bg(colors.background)
            .font_family(TEXT_FONT)
            .text_color(colors.text)
            .child(
                div()
                    .text_2xl()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(shown.title.clone()),
            )
            .child(status)
            .child(div().w(px(320.)).when(is_busy, |track| {
                track.child(busy_sweep(&colors, "loading-sweep"))
            }))
            .children(shown.failure.clone().map(|failure| {
                div()
                    .w_full()
                    .max_w(px(860.))
                    .px_4()
                    .py_2()
                    .rounded(px(CARD_RADIUS))
                    .border_1()
                    .border_color(colors.warning_border)
                    .text_sm()
                    .bg(colors.warning_background)
                    .text_color(colors.warning)
                    .child(failure)
            }))
            .child(
                div()
                    .id("loading-screen-lines")
                    .w_full()
                    .max_w(px(860.))
                    .h(px(360.))
                    .overflow_y_scroll()
                    .p_3()
                    .rounded(px(CARD_RADIUS))
                    .bg(colors.surface)
                    .font_family(CODE_FONT)
                    .text_xs()
                    .text_color(colors.muted_text)
                    .children(
                        shown
                            .lines
                            .iter()
                            .map(|line| div().whitespace_nowrap().child(line.clone())),
                    ),
            )
            .child(actions);
        let screen = div()
            .absolute()
            .top(px(TITLE_BAR_HEIGHT))
            .left_0()
            .right_0()
            .bottom_0()
            .flex()
            .occlude()
            .child(
                div()
                    .w(px(SIDEBAR_WIDTH))
                    .flex_none()
                    .h_full()
                    .bg(colors.sidebar_ground()),
            )
            .child(page);
        fade_in(screen, "loading-screen", OVERLAY).into_any_element()
    }
}
