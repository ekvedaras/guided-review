//! The sidebar's stepper: the review's stations as icons in a row, the card of one of them under it,
//! and one line for each other station that needs the reviewer.

use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, Context, Div, FontWeight, InteractiveElement, IntoElement, ParentElement, Rgba,
    SharedString, StatefulInteractiveElement, Styled, div, px,
};

use super::OneStepLayout;
use crate::app::components::button::{ButtonKind, ButtonSize, icon_button};
use crate::app::components::icon::{Icon, sized_icon};
use crate::app::components::palette::{CARD_RADIUS, Colors, INNER_RADIUS, Tone};
use crate::app::components::reviews_block::ReviewsBlock;
use crate::app::components::tooltip::tooltip;
use crate::app::review_session::ReviewSession;
use crate::app::sidebar_rows::{SidebarProblem, SidebarRows};
use crate::viewer::{Station, StationHealth, StationKind};

/// The round icon of a station in the stepper.
const NODE_SIZE: f32 = 32.;
/// The square behind the icon at the head of a card, and the room the icon of a line under it takes,
/// so the icons of both stand in one column.
const TILE_SIZE: f32 = 22.;

const fn icon_of(kind: StationKind) -> Icon {
    match kind {
        StationKind::Tour => Icon::Sparkles,
        StationKind::AiReview => Icon::ListChecks,
        StationKind::Fixes => Icon::Zap,
        StationKind::YourReview => Icon::Send,
        StationKind::GitHub => Icon::Globe,
    }
}

const fn tone_of(health: StationHealth) -> Tone {
    match health {
        StationHealth::Failed => Tone::Danger,
        StationHealth::Waiting => Tone::Warning,
        StationHealth::Working => Tone::Accent,
        StationHealth::Done => Tone::Success,
        StationHealth::NotStarted => Tone::Neutral,
    }
}

fn ink(colors: &Colors, health: StationHealth) -> Rgba {
    colors.tone(tone_of(health)).text
}

/// The ground behind a station's icon where it is the one shown; a station not started has the
/// quiet hover ground, as the neutral tone's ground is the card's own.
fn ground(colors: &Colors, health: StationHealth) -> Rgba {
    match health {
        StationHealth::NotStarted => colors.hover,
        health => colors.tone(tone_of(health)).background,
    }
}

pub(super) fn problem_line(colors: &Colors, problem: &SidebarProblem) -> Div {
    div()
        .flex()
        .gap_2()
        .text_xs()
        .child(
            div()
                .flex_none()
                .h(px(16.))
                .flex()
                .items_center()
                .child(sized_icon(Icon::CircleAlert, colors.danger, px(12.))),
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
                        .text_color(colors.danger)
                        .child(problem.headline.clone()),
                )
                .children(
                    problem
                        .detail
                        .clone()
                        .map(|detail| div().text_color(colors.muted_text).child(detail)),
                ),
        )
}

fn quiet(colors: &Colors, text: impl Into<SharedString>) -> Div {
    div()
        .text_xs()
        .text_color(colors.muted_text)
        .child(text.into())
}

impl OneStepLayout {
    /// Everything between the progress bar and the steps: problems about this Mac, the stepper, the
    /// card of one station and a line for each other station that needs the reviewer.
    pub(super) fn stations_section(
        &self,
        colors: &Colors,
        sidebar: &SidebarRows,
        is_everything_read: bool,
        cx: &Context<Self>,
    ) -> Div {
        let open = sidebar.stations.open(self.picked_station);
        let mac_problems: Vec<&SidebarProblem> = sidebar
            .problems
            .iter()
            .filter(|problem| problem.home.is_none())
            .collect();
        let also_needing_you: Vec<AnyElement> = sidebar
            .stations
            .also_needing_you(open)
            .map(|station| Self::station_line(colors, station, cx).into_any_element())
            .collect();
        div()
            .flex()
            .flex_col()
            .gap_2()
            .when(!mac_problems.is_empty(), |section| {
                section.child(
                    div()
                        .p_3()
                        .rounded(px(CARD_RADIUS))
                        .bg(colors.danger_background)
                        .flex()
                        .flex_col()
                        .gap_2()
                        .children(
                            mac_problems
                                .into_iter()
                                .map(|problem| problem_line(colors, problem)),
                        ),
                )
            })
            .child(Self::stepper(colors, sidebar.stations.all(), open, cx))
            .children(
                sidebar.stations.get(open).map(|station| {
                    self.station_card(colors, sidebar, station, is_everything_read, cx)
                }),
            )
            .when(!also_needing_you.is_empty(), |section| {
                section.child(div().flex().flex_col().children(also_needing_you))
            })
    }

    fn stepper(
        colors: &Colors,
        stations: &[Station],
        open: StationKind,
        cx: &Context<Self>,
    ) -> Div {
        let last = stations.len().saturating_sub(1);
        let rail = |is_drawn: bool| {
            div()
                .flex_1()
                .h(px(2.))
                .rounded_full()
                .when(is_drawn, |rail| rail.bg(colors.border))
        };
        div()
            .flex()
            .children(stations.iter().enumerate().map(|(at, station)| {
                let kind = station.kind;
                let is_open = kind == open;
                let tip = SharedString::from(format!("{}: {}", kind.name(), station.state));
                div()
                    .id(SharedString::from(format!("station-{}", kind.short_name())))
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap_1()
                    .cursor_pointer()
                    .child(
                        div()
                            .w_full()
                            .flex()
                            .items_center()
                            .child(rail(at > 0))
                            .child(
                                div()
                                    .flex_none()
                                    .mx_1()
                                    .size(px(NODE_SIZE))
                                    .rounded_full()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .map(|node| {
                                        if is_open {
                                            node.bg(ground(colors, station.health))
                                        } else {
                                            node.group_hover("station", |node| {
                                                node.bg(colors.hover)
                                            })
                                        }
                                    })
                                    .child(sized_icon(
                                        icon_of(kind),
                                        ink(colors, station.health),
                                        px(16.),
                                    )),
                            )
                            .child(rail(at < last)),
                    )
                    .child(
                        div()
                            .text_xs()
                            .whitespace_nowrap()
                            .when(is_open, |label| {
                                label
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(colors.text)
                            })
                            .when(!is_open, |label| label.text_color(colors.muted_text))
                            .child(kind.short_name()),
                    )
                    .group("station")
                    .tooltip(tooltip(move |_| tip.clone()))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.picked_station = Some(kind);
                        cx.notify();
                    }))
            }))
    }

    /// The open station: its icon, name and state, the button that shows its agent, then what it
    /// holds.
    fn station_card(
        &self,
        colors: &Colors,
        sidebar: &SidebarRows,
        station: &Station,
        is_everything_read: bool,
        cx: &Context<Self>,
    ) -> Div {
        let problems = sidebar
            .problems
            .iter()
            .filter(|problem| problem.home == Some(station.kind))
            .map(|problem| problem_line(colors, problem).into_any_element());
        let body: Vec<AnyElement> = self
            .station_body(colors, sidebar, station, is_everything_read, cx)
            .into_iter()
            .chain(problems)
            .collect();
        div()
            .p_3()
            .rounded(px(CARD_RADIUS))
            .bg(colors.surface)
            .shadow(colors.card_shadow())
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .h(px(TILE_SIZE))
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_xs()
                    .child(
                        div()
                            .flex_none()
                            .size(px(TILE_SIZE))
                            .rounded(px(INNER_RADIUS))
                            .bg(ground(colors, station.health))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(sized_icon(
                                icon_of(station.kind),
                                ink(colors, station.health),
                                px(12.),
                            )),
                    )
                    .child(
                        div()
                            .flex_none()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(station.kind.name()),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_color(ink(colors, station.health))
                            .child(SharedString::from(station.state.clone())),
                    )
                    .children(Self::show_agent(colors, sidebar, station.kind, cx)),
            )
            .when(!body.is_empty(), |card| {
                card.child(div().flex().flex_col().gap_2().children(body))
            })
    }

    fn show_agent(
        colors: &Colors,
        sidebar: &SidebarRows,
        kind: StationKind,
        cx: &Context<Self>,
    ) -> Option<AnyElement> {
        let button = |id: &'static str| {
            icon_button(
                colors,
                id,
                ButtonKind::Ghost,
                ButtonSize::Small,
                Icon::Terminal,
                "Show agent",
            )
        };
        match kind {
            StationKind::Tour if sidebar.tour_agent.can_show => Some(
                button("show-tour-agent")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.session.update(cx, ReviewSession::show_tour_agent);
                    }))
                    .into_any_element(),
            ),
            StationKind::AiReview if sidebar.ai_review.can_show_agent => Some(
                button("show-review-agent")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.session.update(cx, ReviewSession::show_review_agent);
                    }))
                    .into_any_element(),
            ),
            _ => None,
        }
    }

    fn station_body(
        &self,
        colors: &Colors,
        sidebar: &SidebarRows,
        station: &Station,
        is_everything_read: bool,
        cx: &Context<Self>,
    ) -> Vec<AnyElement> {
        match station.kind {
            StationKind::Tour => match &sidebar.guided_tour {
                Some(guided_tour) => {
                    vec![Self::guided_tour_block(colors, guided_tour, cx).into_any_element()]
                }
                None => Self::tour_agent_block(colors, &sidebar.tour_agent, cx)
                    .map(IntoElement::into_any_element)
                    .into_iter()
                    .collect::<Vec<_>>(),
            },
            StationKind::AiReview => {
                vec![Self::ai_review_block(colors, &sidebar.ai_review, cx).into_any_element()]
            }
            StationKind::Fixes => {
                Self::send_fixes_button(colors, sidebar.ai_review.fixes_to_send, cx)
                    .into_iter()
                    .collect()
            }
            StationKind::YourReview => {
                let session = self.session.read(cx);
                let finish = match session.review().finishes_with_send_tasks() {
                    Some(queued) => (queued > 0)
                        .then(|| Self::send_tasks_button(colors, queued, is_everything_read, cx))
                        .map(IntoElement::into_any_element),
                    None => Some(self.finish_review_dialog(sidebar, is_everything_read, cx)),
                };
                sidebar
                    .changes_not_shown
                    .clone()
                    .map(|notice| {
                        div()
                            .text_xs()
                            .text_color(colors.danger)
                            .child(notice)
                            .into_any_element()
                    })
                    .into_iter()
                    .chain((!sidebar.reviews.is_empty()).then(|| {
                        ReviewsBlock::new(std::sync::Arc::clone(&sidebar.reviews))
                            .into_any_element()
                    }))
                    .chain(finish)
                    .collect()
            }
            StationKind::GitHub => sidebar
                .viewed_sync_off
                .clone()
                .map(|reason| quiet(colors, format!("Viewed marks are not synced: {reason}")))
                .into_iter()
                .chain((station.health == StationHealth::Waiting).then(|| {
                    quiet(
                        colors,
                        "Files you ticked show as not viewed on GitHub until the marks go through. \
                         The app tries again on its own.",
                    )
                }))
                .map(IntoElement::into_any_element)
                .collect(),
        }
    }

    /// A station that needs the reviewer besides the open one: its icon, name and state on one line,
    /// which opens its card.
    fn station_line(colors: &Colors, station: &Station, cx: &Context<Self>) -> impl IntoElement {
        let kind = station.kind;
        let color = ink(colors, station.health);
        div()
            .id(SharedString::from(format!(
                "station-line-{}",
                kind.short_name()
            )))
            .min_h(px(28.))
            .py_1()
            .px_3()
            .rounded(px(INNER_RADIUS))
            .flex()
            .items_center()
            .gap_2()
            .text_xs()
            .cursor_pointer()
            .hover(|line| line.bg(colors.hover))
            .child(
                div()
                    .flex_none()
                    .w(px(TILE_SIZE))
                    .flex()
                    .justify_center()
                    .child(sized_icon(icon_of(kind), color, px(12.))),
            )
            .child(
                div()
                    .flex_none()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(kind.name()),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_color(color)
                    .child(SharedString::from(station.state.clone())),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.picked_station = Some(kind);
                cx.notify();
            }))
    }
}
