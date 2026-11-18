//! The card a hover of the title bar's issue button shows: what GitHub says of the issue, or for a
//! ticket, its link.

use std::rc::Rc;

use gpui::prelude::FluentBuilder;
use gpui::{
    Div, InteractiveElement, IntoElement, ParentElement, ScrollHandle, SharedString,
    StatefulInteractiveElement, Styled, div, px,
};

use crate::app::components::markdown_view::{MarkdownView, PreparedMarkdown};
use crate::app::components::palette::{CARD_RADIUS, Colors, Tone};
use crate::issue::IssueLink;
use crate::viewer::{IssueDetails, IssueState};

const ISSUE_CARD_WIDTH: f32 = 560.;
/// How tall the issue's description is before it scrolls.
const ISSUE_BODY_HEIGHT: f32 = 420.;
const MIN_THUMB_HEIGHT: f32 = 24.;

/// Which of the issue button, or the rows of its list, and which card the mouse is on; the card
/// shows while it is on either, so the mouse can cross the gap between them.
#[derive(Default)]
pub struct IssueHover {
    pub target: Option<usize>,
    pub card: Option<usize>,
}

impl IssueHover {
    pub fn shown(&self) -> Option<usize> {
        self.target.or(self.card)
    }
}

/// Which side of what opens it a card sits on.
#[derive(Clone, Copy)]
pub enum CardSide {
    Below,
    Right,
}

fn state_tone(state: IssueState) -> Tone {
    match state {
        IssueState::Open => Tone::Success,
        IssueState::ClosedAsCompleted | IssueState::Merged => Tone::Accent,
        IssueState::ClosedAsNotPlanned | IssueState::Closed => Tone::Neutral,
    }
}

fn pill(colors: &Colors, text: &str) -> Div {
    div()
        .px_2()
        .rounded_full()
        .border_1()
        .border_color(colors.border)
        .text_xs()
        .text_color(colors.muted_text)
        .child(SharedString::from(text.to_owned()))
}

/// `details` and `body` are `None` for a ticket, and for an issue GitHub has not been read for yet.
pub fn issue_card(
    colors: &Colors,
    link: &IssueLink,
    details: Option<&IssueDetails>,
    body: Option<Rc<PreparedMarkdown>>,
    scroll: &ScrollHandle,
) -> Div {
    let card = div()
        .w(px(ISSUE_CARD_WIDTH))
        .p_3()
        .flex()
        .flex_col()
        .gap_2()
        .text_sm()
        .bg(colors.overlay)
        .border_1()
        .border_color(colors.border)
        .rounded(px(CARD_RADIUS))
        .shadow(colors.overlay_shadow());
    let Some(details) = details else {
        return card.child(
            div()
                .truncate()
                .text_color(colors.muted_text)
                .child(SharedString::from(link.to_string())),
        );
    };
    let tone = colors.tone(state_tone(details.state));
    let state = div()
        .px_2()
        .rounded_full()
        .border_1()
        .border_color(tone.border)
        .bg(tone.background)
        .text_color(tone.text)
        .text_xs()
        .child(details.state.label());
    let author = details.author.as_deref().map(|author| {
        div()
            .text_xs()
            .text_color(colors.muted_text)
            .child(SharedString::from(format!("by {author}")))
    });
    let assignees = (!details.assignees.is_empty()).then(|| {
        div()
            .text_xs()
            .text_color(colors.muted_text)
            .child(SharedString::from(format!(
                "Assigned to {}",
                details.assignees.join(", ")
            )))
    });
    card.child(
        div()
            .flex()
            .gap_2()
            .child(
                div()
                    .flex_none()
                    .text_color(colors.muted_text)
                    .child(link.label()),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child(SharedString::from(details.title.to_string())),
            ),
    )
    .child(
        div()
            .flex()
            .items_center()
            .gap_2()
            .child(state)
            .children(author),
    )
    .when(!details.labels.is_empty(), |card| {
        card.child(
            div()
                .flex()
                .flex_wrap()
                .gap_1()
                .children(details.labels.iter().map(|label| pill(colors, label))),
        )
    })
    .children(assignees)
    .children(body.map(|body| {
        div()
            .relative()
            .pt_2()
            .border_t_1()
            .border_color(colors.border)
            .child(
                div()
                    .id("issue-card-body")
                    .max_h(px(ISSUE_BODY_HEIGHT))
                    .pr_3()
                    .overflow_y_scroll()
                    .track_scroll(scroll)
                    .child(MarkdownView::new(format!("issue-card-{link}"), body)),
            )
            .children(scroll_thumb(colors, scroll))
    }))
}

/// A thumb on the description's right edge, shown only when some of it is out of sight.
fn scroll_thumb(colors: &Colors, scroll: &ScrollHandle) -> Option<impl IntoElement + use<>> {
    let hidden = scroll.max_offset().y;
    let view = scroll.bounds().size.height;
    if hidden <= px(0.) || view <= px(0.) {
        return None;
    }
    let height = (view * (view / (view + hidden))).max(px(MIN_THUMB_HEIGHT));
    let scrolled = (-scroll.offset().y / hidden).clamp(0., 1.);
    Some(
        div()
            .absolute()
            .right_0()
            .top(px(8.) + (view - height) * scrolled)
            .w(px(5.))
            .h(height)
            .rounded_full()
            .bg(colors.muted_text),
    )
}
