//! The sidebar's stepper: the review's stations as icons in a row, the card of one of them under it,
//! and one line for each other station that needs the reviewer.

use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, Context, Div, ElementId, FontWeight, InteractiveElement, IntoElement,
    ParentElement, Pixels, Rgba, SharedString, StatefulInteractiveElement, Styled, div, px,
};

use super::OneStepLayout;
use crate::app::components::button::{ButtonKind, ButtonSize, icon_button};
use crate::app::components::icon::{Icon, sized_icon};
use crate::app::components::motion::{pulse, shimmer_text};
use crate::app::components::palette::{CARD_RADIUS, Colors, INNER_RADIUS, Tone};
use crate::app::components::reviews_block::ReviewsBlock;
use crate::app::components::tooltip::tooltip;
use crate::app::review_session::ReviewSession;
use crate::app::sidebar_rows::{SidebarProblem, SidebarRows};
use crate::viewer::{AiReviewAction, Intent, Station, StationHealth, StationKind};

/// The round icon of a station in the stepper.
const NODE_SIZE: f32 = 32.;
/// The square behind the icon at the head of a card, and the room the icon of a line under it takes,
/// so the icons of both stand in one column.
const TILE_SIZE: f32 = 22.;
/// The space between the tile and the name.
const TILE_GAP: f32 = 8.;
/// One line of the small text the rows are set in, so a row's icon stands beside its first line.
const LINE_HEIGHT: f32 = 16.;

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
        StationHealth::NotStarted => colors.border,
        health => colors.tone(tone_of(health)).background,
    }
}

/// A problem's headline and its detail under it. In a station's card the head already shows the
/// station failed in red, so only a problem above the stepper has the alert icon and a red headline.
fn problem_line(colors: &Colors, problem: &SidebarProblem, is_above_stepper: bool) -> Div {
    div()
        .flex()
        .gap_2()
        .text_xs()
        .when(is_above_stepper, |line| {
            line.child(
                div()
                    .flex_none()
                    .h(px(16.))
                    .flex()
                    .items_center()
                    .child(sized_icon(Icon::CircleAlert, colors.danger, px(12.))),
            )
        })
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap_0p5()
                .child(
                    div()
                        .text_color(if is_above_stepper {
                            colors.danger
                        } else {
                            colors.text
                        })
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

/// The ground of a station at work, under its icon: it breathes, so the stepper shows something
/// is moving even when that station's card is not the one open.
fn busy_ground(colors: &Colors, radius: Pixels, id: impl Into<ElementId>) -> impl IntoElement {
    pulse(
        div()
            .absolute()
            .inset_0()
            .rounded(radius)
            .bg(ground(colors, StationHealth::Working)),
        id,
    )
}

/// A station's state in its tone; a station at work has a shine running over it.
fn state_text(colors: &Colors, station: &Station, id: impl Into<ElementId>) -> AnyElement {
    let state = SharedString::from(station.state.clone());
    let color = ink(colors, station.health);
    if station.health == StationHealth::Working {
        shimmer_text(state, color, colors.text, id).into_any_element()
    } else {
        div().text_color(color).child(state).into_any_element()
    }
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
                        .mb_1()
                        .p_3()
                        .rounded(px(CARD_RADIUS))
                        .bg(colors.danger_background)
                        .flex()
                        .flex_col()
                        .gap_2()
                        .children(
                            mac_problems
                                .into_iter()
                                .map(|problem| problem_line(colors, problem, true)),
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
                                    .mx(px(8.))
                                    .size(px(NODE_SIZE))
                                    .rounded_full()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .relative()
                                    .map(|node| {
                                        if station.health == StationHealth::Working {
                                            node.child(busy_ground(
                                                colors,
                                                px(NODE_SIZE / 2.),
                                                format!("station-node-busy-{}", kind.short_name()),
                                            ))
                                        } else if is_open {
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

    /// The open station: its icon and name, its state and its agent's buttons at the right, then what
    /// it holds, in line with the name.
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
            .map(|problem| problem_line(colors, problem, false).into_any_element());
        let body: Vec<AnyElement> = self
            .station_body(colors, sidebar, station, is_everything_read, cx)
            .into_iter()
            .chain(problems)
            .collect();
        let head_buttons = Self::head_buttons(colors, sidebar, station.kind, cx);
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
                    .min_h(px(TILE_SIZE))
                    .flex()
                    .items_center()
                    .gap(px(TILE_GAP))
                    .text_xs()
                    .child(
                        div()
                            .flex_none()
                            .size(px(TILE_SIZE))
                            .rounded(px(INNER_RADIUS))
                            .relative()
                            .map(|tile| {
                                if station.health == StationHealth::Working {
                                    tile.child(busy_ground(
                                        colors,
                                        px(INNER_RADIUS),
                                        "station-tile-busy",
                                    ))
                                } else {
                                    tile.bg(ground(colors, station.health))
                                }
                            })
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
                            .flex()
                            .justify_end()
                            .text_right()
                            .child(state_text(colors, station, "station-card-state")),
                    )
                    .when(!head_buttons.is_empty(), |head| {
                        head.child(
                            div()
                                .flex_none()
                                .flex()
                                .items_center()
                                .my(px(-2.))
                                .mr(px(-6.))
                                .children(head_buttons),
                        )
                    }),
            )
            .when(!body.is_empty(), |card| {
                card.child(
                    div()
                        .pl(px(TILE_SIZE + TILE_GAP))
                        .flex()
                        .flex_col()
                        .gap_2()
                        .children(body),
                )
            })
    }

    /// Run again and Show agent: quiet icons, as they act on the station's agent, not on the review.
    fn head_buttons(
        colors: &Colors,
        sidebar: &SidebarRows,
        kind: StationKind,
        cx: &Context<Self>,
    ) -> Vec<AnyElement> {
        let run_again = (kind == StationKind::AiReview
            && sidebar.ai_review.action == Some(AiReviewAction::RunAgain))
        .then(|| {
            icon_button(
                colors,
                "sidebar-run-ai-review-again",
                ButtonKind::Ghost,
                ButtonSize::Small,
                Icon::Refresh,
                "Run again",
            )
            .on_click(cx.listener(|this, _, window, cx| {
                this.act(Intent::RunAiReview, window, cx);
            }))
            .into_any_element()
        });
        run_again
            .into_iter()
            .chain(Self::show_agent(colors, sidebar, kind, cx))
            .collect()
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
            StationKind::AiReview
                if sidebar.ai_review.can_show_agent && !sidebar.ai_review.is_failure =>
            {
                Some(
                    button("show-review-agent")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.session.update(cx, ReviewSession::show_review_agent);
                        }))
                        .into_any_element(),
                )
            }
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
                let ai_review = &sidebar.ai_review;
                let has_body = ai_review.is_failure
                    || ai_review.has_findings
                    || ai_review.has_dismissed_findings
                    || ai_review
                        .action
                        .is_some_and(|action| action != AiReviewAction::RunAgain);
                has_body
                    .then(|| Self::ai_review_block(colors, ai_review, cx).into_any_element())
                    .into_iter()
                    .collect()
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
            .py(px(6.))
            .px_3()
            .rounded(px(INNER_RADIUS))
            .flex()
            .items_start()
            .gap(px(TILE_GAP))
            .text_xs()
            .cursor_pointer()
            .hover(|line| line.bg(colors.hover))
            .child(
                div()
                    .flex_none()
                    .w(px(TILE_SIZE))
                    .h(px(LINE_HEIGHT))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(sized_icon(icon_of(kind), color, px(12.))),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_wrap()
                    .justify_between()
                    .gap_x_3()
                    .child(
                        div()
                            .flex_none()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(kind.name()),
                    )
                    .child(div().min_w_0().child(state_text(
                        colors,
                        station,
                        format!("station-line-state-{}", kind.short_name()),
                    ))),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.picked_station = Some(kind);
                cx.notify();
            }))
    }
}
