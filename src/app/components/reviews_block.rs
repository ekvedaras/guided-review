use std::sync::Arc;

use gpui::{
    AnyElement, App, InteractiveElement, IntoElement, ParentElement, RenderOnce, SharedString,
    StatefulInteractiveElement, Styled, Window, div, px,
};

use crate::app::sidebar_rows::{SidebarReviews, SidebarYourReview};
use crate::viewer::{CLAMPED_BODY_LINES, ReviewerLine, ReviewerStatus, StanceAge};

use super::icon::{Icon, sized_icon};
use super::motion::turn_to;
use super::palette::{Colors, Tone};
use super::step_notes::note_label;
use super::thread_view::avatar_circle;
use super::tooltip::tooltip;

/// One line of the "Last review" text, so its icon centres on the first line however it wraps.
const SINCE_LINE_HEIGHT: gpui::Pixels = px(16.);
const REVIEWER_AVATAR_SIZE: f32 = 18.;

/// The sidebar's reviews: where the viewer stands, in the colour of that state, then who else
/// reviewed or is asked to, each by name with their state.
#[derive(IntoElement)]
pub struct ReviewsBlock {
    reviews: Arc<SidebarReviews>,
}

impl ReviewsBlock {
    pub fn new(reviews: Arc<SidebarReviews>) -> Self {
        Self { reviews }
    }
}

fn tone_of(status: ReviewerStatus) -> Tone {
    match status {
        ReviewerStatus::Approved => Tone::Success,
        ReviewerStatus::ChangesRequested => Tone::Danger,
        ReviewerStatus::Waiting => Tone::Warning,
        ReviewerStatus::Commented | ReviewerStatus::Dismissed => Tone::Neutral,
    }
}

fn icon_of(status: ReviewerStatus) -> Icon {
    match status {
        ReviewerStatus::Approved => Icon::CircleCheck,
        ReviewerStatus::ChangesRequested => Icon::FilePen,
        ReviewerStatus::Commented => Icon::MessageSquare,
        ReviewerStatus::Dismissed => Icon::Minus,
        ReviewerStatus::Waiting => Icon::Clock,
    }
}

fn label_of(status: ReviewerStatus) -> &'static str {
    match status {
        ReviewerStatus::Approved => "Approved",
        ReviewerStatus::ChangesRequested => "Changes requested",
        ReviewerStatus::Commented => "Commented",
        ReviewerStatus::Dismissed => "Dismissed",
        ReviewerStatus::Waiting => "Waiting",
    }
}

impl RenderOnce for ReviewsBlock {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let colors = Colors::active(cx);
        let is_body_open = window.use_keyed_state("your-review-body-open", cx, |_, _| false);
        let yours = self
            .reviews
            .yours
            .as_ref()
            .map(|yours| your_review(&colors, yours, &is_body_open, cx));
        let mut rows = Vec::with_capacity(self.reviews.others.len());
        for (place, line) in self.reviews.others.iter().enumerate() {
            rows.push(reviewer_row(&colors, place, line, cx));
        }
        let others = (!rows.is_empty()).then(|| {
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    note_label("Reviewers")
                        .text_color(colors.muted_text)
                        .pb_0p5(),
                )
                .children(rows)
        });
        div()
            .flex()
            .flex_col()
            .gap_3()
            .children(yours)
            .children(others)
    }
}

fn your_review(
    colors: &Colors,
    yours: &SidebarYourReview,
    is_body_open: &gpui::Entity<bool>,
    cx: &mut App,
) -> AnyElement {
    let tone = colors.tone(tone_of(yours.status));
    let since = (!yours.since.is_empty()).then(|| {
        let (icon, color) = if yours.is_marked_done {
            (Some(Icon::Check), colors.muted_text)
        } else {
            (None, colors.muted_text)
        };
        div()
            .flex()
            .items_start()
            .gap_1()
            .text_xs()
            .line_height(SINCE_LINE_HEIGHT)
            .text_color(color)
            .children(icon.map(|icon| {
                div()
                    .flex_none()
                    .h(SINCE_LINE_HEIGHT)
                    .flex()
                    .items_center()
                    .child(sized_icon(icon, color, px(12.)))
            }))
            .child(div().min_w_0().child(yours.since.clone()))
    });
    let is_open = *is_body_open.read(cx);
    let body = (!yours.body.is_empty()).then(|| {
        let text = div()
            .text_xs()
            .text_color(colors.muted_text)
            .child(yours.body.clone());
        if yours.is_body_long && !is_open {
            text.line_clamp(CLAMPED_BODY_LINES)
        } else {
            text
        }
    });
    let toggle = yours.is_body_long.then(|| {
        let is_body_open = is_body_open.clone();
        div()
            .id("your-review-body-toggle")
            .flex()
            .items_center()
            .gap_1()
            .text_xs()
            .cursor_pointer()
            .text_color(colors.muted_text)
            .hover(|toggle| toggle.text_color(colors.text))
            .role(gpui::Role::Button)
            .aria_label("Show or hide your review text")
            .aria_expanded(is_open)
            .on_click(move |_, _, cx| {
                is_body_open.update(cx, |open, cx| {
                    *open = !*open;
                    cx.notify();
                });
            })
            .child(turn_to(
                sized_icon(Icon::ChevronRight, colors.muted_text, px(12.)),
                "your-review-body-chevron",
                is_open,
                1.,
            ))
            .child(if is_open { "Show less" } else { "Show all" })
    });
    div()
        .id("your-review")
        .flex()
        .flex_col()
        .gap_1p5()
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(avatar_circle(
                    colors,
                    "You",
                    yours.avatar_url.as_deref(),
                    REVIEWER_AVATAR_SIZE,
                    cx,
                ))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_xs()
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_color(tone.text)
                        .child(yours.title.clone()),
                ),
        )
        .children(since)
        .children(body)
        .children(toggle)
        .into_any_element()
}

fn reviewer_row(colors: &Colors, place: usize, line: &ReviewerLine, cx: &mut App) -> AnyElement {
    let is_old = line.age == StanceAge::BeforeNewCommits;
    let status_color = if is_old {
        colors.muted_text
    } else {
        colors.tone(tone_of(line.status)).text
    };
    let hint: SharedString = if is_old {
        "On an older commit: the PR has new commits since".into()
    } else {
        label_of(line.status).into()
    };
    div()
        .id(("reviewer", place))
        .tooltip(tooltip(move |_| hint.clone()))
        .flex()
        .items_center()
        .gap_2()
        .min_h(px(24.))
        .child(avatar_circle(
            colors,
            &line.name,
            line.avatar_url.as_deref(),
            REVIEWER_AVATAR_SIZE,
            cx,
        ))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_xs()
                .text_color(colors.sidebar_text)
                .child(SharedString::from(Arc::clone(&line.name))),
        )
        .child(
            div()
                .flex_none()
                .flex()
                .items_center()
                .gap_1()
                .text_xs()
                .text_color(status_color)
                .child(sized_icon(icon_of(line.status), status_color, px(12.)))
                .child(label_of(line.status))
                .children(is_old.then_some("· older")),
        )
        .into_any_element()
}
