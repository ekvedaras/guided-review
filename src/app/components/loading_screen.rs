use gpui::prelude::FluentBuilder;
use gpui::{
    Context, Entity, FontWeight, InteractiveElement, IntoElement, ParentElement, Render,
    SharedString, StatefulInteractiveElement, Styled, Window, div, px,
};

use super::button::{ButtonKind, ButtonSize, button};
use super::palette::{CODE_FONT, Colors, TEXT_FONT};
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
                    button(
                        &colors,
                        "loading-try-again",
                        ButtonKind::Primary,
                        ButtonSize::Regular,
                    )
                    .child("Try again")
                    .on_click(move |_, _, cx| tour_writer::try_again(retry_pr.clone(), cx)),
                )
            })
            .child(
                button(
                    &colors,
                    "loading-show-agent",
                    ButtonKind::Secondary,
                    ButtonSize::Regular,
                )
                .child("Show agent")
                .on_click(move |_, _, cx| tour_writer::show_agent(show_pr.clone(), cx)),
            )
            .children(shown.back_to.clone().map(|back_to| {
                button(
                    &colors,
                    "loading-back",
                    ButtonKind::Ghost,
                    ButtonSize::Regular,
                )
                .child(back_to)
                .on_click(|_, _, cx| tour_writer::stop_waiting(cx))
            }));
        div()
            .absolute()
            .top(px(TITLE_BAR_HEIGHT))
            .left_0()
            .right_0()
            .bottom_0()
            .flex()
            .flex_col()
            .items_center()
            .pt(px(80.))
            .gap_4()
            .bg(colors.background)
            .occlude()
            .font_family(TEXT_FONT)
            .text_color(colors.text)
            .child(
                div()
                    .text_xl()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(shown.title.clone()),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(colors.muted_text)
                    .child(shown.status.clone()),
            )
            .children(shown.failure.clone().map(|failure| {
                div()
                    .w(px(860.))
                    .px_4()
                    .py_2()
                    .rounded_md()
                    .text_sm()
                    .bg(colors.warning_background)
                    .text_color(colors.warning)
                    .child(failure)
            }))
            .child(
                div()
                    .id("loading-screen-lines")
                    .w(px(860.))
                    .h(px(360.))
                    .overflow_y_scroll()
                    .p_3()
                    .rounded_md()
                    .border_1()
                    .border_color(colors.border)
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
            .child(actions)
            .into_any_element()
    }
}
