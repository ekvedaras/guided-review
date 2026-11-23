//! PROTOTYPE (issue #174), throwaway: three designs of the agent section of the sidebar, under the
//! progress bar and above the summary row. Start the app with GUIDED_REVIEW_PROTOTYPE=0|A|B|C and switch
//! with the floating bar at the foot of the window. 0 is the current design. Not for main.

use std::sync::LazyLock;
use std::sync::atomic::{AtomicU8, Ordering};

use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, Context, Div, FontWeight, InteractiveElement, IntoElement, ParentElement, Rgba,
    SharedString, StatefulInteractiveElement, Styled, div, px,
};

use super::OneStepLayout;
use crate::app::components::button::{
    ButtonKind, ButtonSize, button_keys, icon_button, icon_label_button,
};
use crate::app::components::icon::{Icon, sized_icon};
use crate::app::components::palette::{CARD_RADIUS, Colors};
use crate::app::components::tooltip::tooltip;
use crate::app::review_session::ReviewSession;
use crate::app::sidebar_rows::{SidebarAiReview, SidebarGuidedTour, SidebarTourAgent, WriteOffer};
use crate::viewer::{AiReviewAction, Command, Intent};

const NAMES: [&str; 4] = [
    "Current",
    "A · Agent rows",
    "B · One next step",
    "C · Pipeline",
];

static VARIANT: LazyLock<AtomicU8> = LazyLock::new(|| {
    let start = match std::env::var("GUIDED_REVIEW_PROTOTYPE").ok().as_deref() {
        Some("A" | "a") => 1,
        Some("B" | "b") => 2,
        Some("C" | "c") => 3,
        _ => 0,
    };
    AtomicU8::new(start)
});

fn is_on() -> bool {
    std::env::var_os("GUIDED_REVIEW_PROTOTYPE").is_some()
}

pub(super) fn variant() -> u8 {
    if is_on() {
        VARIANT.load(Ordering::Relaxed)
    } else {
        0
    }
}

/// The floating switcher: hidden unless GUIDED_REVIEW_PROTOTYPE is set, and in snapshot pictures.
pub(super) fn switcher(colors: &Colors, cx: &Context<OneStepLayout>) -> Option<AnyElement> {
    if !is_on() || cfg!(feature = "snapshots") {
        return None;
    }
    let current = variant();
    let step = |by: u8| {
        cx.listener(move |_, _, _, cx| {
            VARIANT.store((variant() + by) % 4, Ordering::Relaxed);
            cx.notify();
        })
    };
    let arrow = |id: &'static str, icon: Icon, by: u8| {
        div()
            .id(id)
            .px_2()
            .py_1()
            .rounded_full()
            .cursor_pointer()
            .hover(|arrow| arrow.bg(gpui::rgba(0xffffff22)))
            .child(sized_icon(icon, gpui::rgb(0xffffff).into(), px(14.)))
            .on_click(step(by))
    };
    Some(
        div()
            .absolute()
            .bottom(px(16.))
            .left_0()
            .right_0()
            .flex()
            .justify_center()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .py_1()
                    .rounded_full()
                    .bg(gpui::rgb(0xd0217a))
                    .shadow(colors.overlay_shadow())
                    .text_color(gpui::rgb(0xffffff))
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(arrow("proto-prev", Icon::ArrowLeft, 3))
                    .child(format!("PROTOTYPE  {}", NAMES[current as usize]))
                    .child(arrow("proto-next", Icon::ArrowRight, 1)),
            )
            .into_any_element(),
    )
}

/// The AI review's line, cut into its parts.
struct ReviewFacts {
    headline: String,
    by_severity: Option<String>,
    dismissed: Option<String>,
    fix_count: usize,
    fix_commits: Vec<String>,
}

impl ReviewFacts {
    fn of(line: &str) -> Self {
        let mut facts = Self {
            headline: String::new(),
            by_severity: None,
            dismissed: None,
            fix_count: 0,
            fix_commits: Vec::new(),
        };
        for (at, part) in line.split(" · ").enumerate() {
            if at == 0 {
                match part.split_once(": ") {
                    Some((head, rest)) if part.contains("finding") => {
                        facts.headline = head.to_owned();
                        facts.by_severity = Some(rest.to_owned());
                    }
                    _ => facts.headline = part.to_owned(),
                }
            } else if part.ends_with("dismissed") {
                facts.dismissed = Some(part.to_owned());
            } else if let Some((count, commits)) = part.split_once(": ") {
                facts.fix_count = count
                    .split(' ')
                    .next()
                    .and_then(|n| n.parse().ok())
                    .unwrap_or(0);
                facts.fix_commits = commits
                    .split(", ")
                    .map(|sha| sha.chars().take(7).collect())
                    .collect();
            }
        }
        facts
    }

    fn fixes_line(&self) -> Option<String> {
        match self.fix_count {
            0 => None,
            1 => Some(format!("1 fix pushed · {}", self.fix_commits.join(" "))),
            n => Some(format!(
                "{n} fixes pushed · {}{}",
                self.fix_commits.first().cloned().unwrap_or_default(),
                if n > 1 {
                    format!(" +{}", n - 1)
                } else {
                    String::new()
                }
            )),
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Health {
    Busy,
    Waiting,
    Idle,
    Failed,
    Off,
}

impl Health {
    fn color(self, colors: &Colors) -> Rgba {
        match self {
            Self::Busy => colors.accent,
            Self::Waiting => colors.warning,
            Self::Idle => colors.success,
            Self::Failed => colors.danger,
            Self::Off => colors.muted_text,
        }
    }
}

fn tour_status(agent: &SidebarTourAgent) -> (String, Health) {
    let line = agent.line.to_string();
    let status = line
        .split_once(": ")
        .map_or(line.clone(), |(_, status)| status.to_owned());
    let health = if line.contains(" is off") {
        Health::Off
    } else if status.contains("wait") {
        Health::Waiting
    } else if status.contains("working")
        || status.contains("starting")
        || status.contains("looking")
    {
        Health::Busy
    } else if status.contains("none") {
        Health::Off
    } else {
        Health::Idle
    };
    (status, health)
}

fn review_health(review: &SidebarAiReview) -> Health {
    if review.is_failure {
        Health::Failed
    } else if review.line.contains("waits") {
        Health::Waiting
    } else if review.line.starts_with("Reviewing") {
        Health::Busy
    } else if review.line.starts_with("Not run") {
        Health::Off
    } else {
        Health::Idle
    }
}

fn stage_line(line: &str) -> String {
    line.strip_prefix("Guided tour: ")
        .unwrap_or(line)
        .to_owned()
}

fn dot(colors: &Colors, health: Health) -> Div {
    div()
        .flex_none()
        .size(px(7.))
        .rounded_full()
        .bg(health.color(colors))
}

fn muted(colors: &Colors, text: impl Into<SharedString>) -> Div {
    div()
        .text_xs()
        .text_color(colors.muted_text)
        .child(text.into())
}

// The clicks, shared by every design.

fn show_tour_agent(
    colors: &Colors,
    id: &'static str,
    label: bool,
    cx: &Context<OneStepLayout>,
) -> AnyElement {
    let button = if label {
        icon_label_button(
            colors,
            id,
            ButtonKind::Ghost,
            ButtonSize::Small,
            Icon::Terminal,
            "Show agent",
        )
    } else {
        icon_button(
            colors,
            id,
            ButtonKind::Ghost,
            ButtonSize::Small,
            Icon::Terminal,
            "Show tour agent",
        )
    };
    button
        .on_click(cx.listener(|this, _, _, cx| {
            this.session.update(cx, ReviewSession::show_tour_agent);
        }))
        .into_any_element()
}

fn show_review_agent(
    colors: &Colors,
    id: &'static str,
    label: bool,
    cx: &Context<OneStepLayout>,
) -> AnyElement {
    let button = if label {
        icon_label_button(
            colors,
            id,
            ButtonKind::Ghost,
            ButtonSize::Small,
            Icon::Terminal,
            "Show agent",
        )
    } else {
        icon_button(
            colors,
            id,
            ButtonKind::Ghost,
            ButtonSize::Small,
            Icon::Terminal,
            "Show review agent",
        )
    };
    button
        .on_click(cx.listener(|this, _, _, cx| {
            this.session.update(cx, ReviewSession::show_review_agent);
        }))
        .into_any_element()
}

fn findings_list(
    colors: &Colors,
    kind: ButtonKind,
    label: bool,
    cx: &Context<OneStepLayout>,
) -> AnyElement {
    let button = if label {
        icon_label_button(
            colors,
            "p-findings-list",
            kind,
            ButtonSize::Small,
            Icon::ListChecks,
            "Findings list",
        )
        .child(button_keys(colors, kind, Command::FindingsList, cx))
    } else {
        icon_button(
            colors,
            "p-findings-list",
            kind,
            ButtonSize::Small,
            Icon::ListChecks,
            "Findings list (F)",
        )
    };
    button
        .on_click(
            cx.listener(|this, _, window, cx| this.act(Intent::ToggleFindingsList, window, cx)),
        )
        .into_any_element()
}

fn next_finding(colors: &Colors, kind: ButtonKind, cx: &Context<OneStepLayout>) -> AnyElement {
    icon_label_button(
        colors,
        "p-next-finding",
        kind,
        ButtonSize::Small,
        Icon::ArrowRight,
        "Next finding",
    )
    .on_click(cx.listener(|this, _, window, cx| this.act(Intent::NextFinding, window, cx)))
    .into_any_element()
}

fn send_fixes(
    colors: &Colors,
    count: usize,
    kind: ButtonKind,
    cx: &Context<OneStepLayout>,
) -> AnyElement {
    icon_label_button(
        colors,
        "p-send-fixes",
        kind,
        ButtonSize::Small,
        Icon::Zap,
        format!("Send fixes ({count})"),
    )
    .on_click(cx.listener(|this, _, _, cx| {
        this.session.update(cx, ReviewSession::send_fixes);
    }))
    .into_any_element()
}

fn review_action(
    colors: &Colors,
    action: AiReviewAction,
    kind: ButtonKind,
    label: bool,
    cx: &Context<OneStepLayout>,
) -> AnyElement {
    let icon = super::ai_review_icon(action);
    let button = if label {
        icon_label_button(
            colors,
            "p-review-action",
            kind,
            ButtonSize::Small,
            icon,
            action.label(),
        )
    } else {
        icon_button(
            colors,
            "p-review-action",
            kind,
            ButtonSize::Small,
            icon,
            action.label(),
        )
    };
    button
        .on_click(cx.listener(|this, _, window, cx| this.act(Intent::RunAiReview, window, cx)))
        .into_any_element()
}

fn write_tour(
    colors: &Colors,
    label: &'static str,
    kind: ButtonKind,
    cx: &Context<OneStepLayout>,
) -> AnyElement {
    icon_label_button(
        colors,
        "p-write-tour",
        kind,
        ButtonSize::Small,
        Icon::Sparkles,
        label,
    )
    .on_click(cx.listener(|this, _, window, cx| this.act(Intent::WriteGuidedTour, window, cx)))
    .into_any_element()
}

fn section_label(colors: &Colors, text: &'static str) -> Div {
    div()
        .text_xs()
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(colors.muted_text)
        .child(text.to_uppercase())
}

// A · Agent rows: one row per agent, its health as a dot, Show agent as an icon at the row's end; what the
// review found under its row; the actions in one line below.

pub(super) fn variant_a(
    colors: &Colors,
    guided_tour: Option<&SidebarGuidedTour>,
    tour_agent: &SidebarTourAgent,
    review: &SidebarAiReview,
    cx: &Context<OneStepLayout>,
) -> AnyElement {
    let facts = ReviewFacts::of(&review.line);
    let agent_row = |id: &'static str,
                     name: &'static str,
                     health: Health,
                     status: String,
                     show: Option<AnyElement>| {
        div()
            .id(id)
            .flex()
            .items_center()
            .gap_2()
            .h(px(24.))
            .child(dot(colors, health))
            .child(
                div()
                    .flex_none()
                    .text_xs()
                    .font_weight(FontWeight::MEDIUM)
                    .child(name),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_xs()
                    .text_color(if health == Health::Failed {
                        colors.danger
                    } else {
                        colors.muted_text
                    })
                    .child(status.clone()),
            )
            .tooltip(tooltip(move |_| status.clone().into()))
            .children(show)
    };
    let writing = guided_tour.and_then(|tour| tour.stage.as_ref());
    let tour_row = if let Some(stage) = writing {
        Some(agent_row(
            "p-a-tour",
            "Tour agent",
            if stage.can_try_again {
                Health::Failed
            } else {
                Health::Busy
            },
            stage_line(&stage.line),
            guided_tour
                .filter(|tour| tour.can_show_agent)
                .map(|_| show_tour_agent(colors, "p-a-show-tour", false, cx)),
        ))
    } else if tour_agent.is_shown {
        let (status, health) = tour_status(tour_agent);
        Some(agent_row(
            "p-a-tour",
            "Tour agent",
            health,
            status,
            tour_agent
                .can_show
                .then(|| show_tour_agent(colors, "p-a-show-tour", false, cx)),
        ))
    } else {
        None
    };
    let review_status = if review.is_failure {
        "failed".to_owned()
    } else if review.line.starts_with("Reviewing") || review.line.contains("waits") {
        review.line.to_string()
    } else {
        facts.headline.clone()
    };
    let review_row = agent_row(
        "p-a-review",
        "Review agent",
        review_health(review),
        review_status,
        review
            .can_show_agent
            .then(|| show_review_agent(colors, "p-a-show-review", false, cx)),
    );
    let details = div()
        .pl(px(15.))
        .flex()
        .flex_col()
        .gap_0p5()
        .when(review.is_failure, |details| {
            details.child(muted(colors, review.line.clone()).text_color(colors.danger))
        })
        .children(
            facts
                .by_severity
                .clone()
                .filter(|_| !review.is_failure)
                .map(|line| muted(colors, line)),
        )
        .children(facts.dismissed.clone().map(|line| muted(colors, line)))
        .children(facts.fixes_line().map(|line| muted(colors, line)));
    let mut actions: Vec<AnyElement> = Vec::new();
    if review.fixes_to_send > 0 {
        actions.push(send_fixes(
            colors,
            review.fixes_to_send,
            ButtonKind::Primary,
            cx,
        ));
    }
    if review.has_findings {
        actions.push(next_finding(colors, ButtonKind::Secondary, cx));
    }
    if review.has_findings || review.has_dismissed_findings {
        actions.push(findings_list(colors, ButtonKind::Ghost, false, cx));
    }
    if let Some(action) = review.action {
        let label = action != AiReviewAction::RunAgain;
        actions.push(review_action(colors, action, ButtonKind::Ghost, label, cx));
    }
    let write = guided_tour
        .and_then(|tour| tour.write)
        .map(|offer| match offer {
            WriteOffer::Available => {
                write_tour(colors, "Write guided tour", ButtonKind::Secondary, cx)
            }
            WriteOffer::Refused(refusal) => muted(colors, refusal.reason()).into_any_element(),
        });
    let try_again = writing
        .filter(|stage| stage.can_try_again)
        .map(|_| write_tour(colors, "Try again", ButtonKind::Secondary, cx));
    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(section_label(colors, "Agents"))
        .children(tour_row)
        .children(
            writing
                .and_then(|stage| stage.activity.clone())
                .map(|activity| {
                    muted(colors, activity)
                        .pl(px(15.))
                        .truncate()
                        .font_family(*crate::app::components::palette::CODE_FONT)
                }),
        )
        .children(try_again.map(|button| div().pl(px(15.)).flex().child(button)))
        .child(review_row)
        .child(details)
        .child(
            div()
                .pt_1()
                .flex()
                .flex_wrap()
                .items_center()
                .gap_1()
                .children(actions),
        )
        .children(write)
        .into_any_element()
}

// B · One next step: a card that says in one sentence where the work stands, one button for what to do
// next, and every other action as a quiet icon in a row under it.

pub(super) fn variant_b(
    colors: &Colors,
    guided_tour: Option<&SidebarGuidedTour>,
    tour_agent: &SidebarTourAgent,
    review: &SidebarAiReview,
    cx: &Context<OneStepLayout>,
) -> AnyElement {
    let facts = ReviewFacts::of(&review.line);
    let writing = guided_tour.and_then(|tour| tour.stage.as_ref());
    let (headline, detail, tone): (String, Option<String>, Health) = if let Some(stage) = writing {
        (
            if stage.can_try_again {
                "The guided tour failed".to_owned()
            } else {
                "Writing the guided tour".to_owned()
            },
            Some(
                stage
                    .activity
                    .clone()
                    .unwrap_or_else(|| stage_line(&stage.line)),
            ),
            if stage.can_try_again {
                Health::Failed
            } else {
                Health::Busy
            },
        )
    } else if review.is_failure {
        (
            "AI review failed".to_owned(),
            Some(review.line.to_string()),
            Health::Failed,
        )
    } else if review.fixes_to_send > 0 {
        (
            match review.fixes_to_send {
                1 => "1 fix ready to send".to_owned(),
                n => format!("{n} fixes ready to send"),
            },
            Some("The tour agent makes them and pushes".to_owned()),
            Health::Waiting,
        )
    } else if review.line.starts_with("Reviewing") || review.line.contains("waits") {
        (
            "AI review running".to_owned(),
            Some(review.line.to_string()),
            Health::Busy,
        )
    } else if review.has_findings {
        (
            format!("AI review: {}", facts.headline),
            facts.by_severity.clone(),
            Health::Idle,
        )
    } else {
        (
            format!("AI review: {}", facts.headline.to_lowercase()),
            None,
            Health::Off,
        )
    };
    let primary: Option<AnyElement> = if writing.is_some_and(|stage| stage.can_try_again) {
        Some(write_tour(colors, "Try again", ButtonKind::Primary, cx))
    } else if review.fixes_to_send > 0 {
        Some(send_fixes(
            colors,
            review.fixes_to_send,
            ButtonKind::Primary,
            cx,
        ))
    } else if review.action == Some(AiReviewAction::TryAgain) {
        Some(review_action(
            colors,
            AiReviewAction::TryAgain,
            ButtonKind::Primary,
            true,
            cx,
        ))
    } else if review.has_findings {
        Some(next_finding(colors, ButtonKind::Secondary, cx))
    } else if review.action == Some(AiReviewAction::Run) {
        Some(review_action(
            colors,
            AiReviewAction::Run,
            ButtonKind::Secondary,
            true,
            cx,
        ))
    } else if guided_tour
        .and_then(|tour| tour.write)
        .is_some_and(|offer| matches!(offer, WriteOffer::Available))
    {
        Some(write_tour(
            colors,
            "Write guided tour",
            ButtonKind::Secondary,
            cx,
        ))
    } else {
        None
    };
    let mut quiet: Vec<AnyElement> = Vec::new();
    if guided_tour.is_some_and(|tour| tour.can_show_agent) || tour_agent.can_show {
        quiet.push(show_tour_agent(colors, "p-b-show-tour", false, cx));
    }
    if review.can_show_agent {
        quiet.push(show_review_agent(colors, "p-b-show-review", false, cx));
    }
    if review.has_findings || review.has_dismissed_findings {
        quiet.push(findings_list(colors, ButtonKind::Ghost, false, cx));
    }
    if review.action == Some(AiReviewAction::RunAgain) {
        quiet.push(review_action(
            colors,
            AiReviewAction::RunAgain,
            ButtonKind::Ghost,
            false,
            cx,
        ));
    }
    let (tour_status_line, tour_health) = writing.map_or_else(
        || tour_status(tour_agent),
        |stage| {
            (
                stage_line(&stage.line),
                if stage.can_try_again {
                    Health::Failed
                } else {
                    Health::Busy
                },
            )
        },
    );
    let footnotes = [
        (tour_agent.is_shown || writing.is_some())
            .then(|| format!("Tour agent: {tour_status_line}")),
        facts.dismissed.clone(),
        facts.fixes_line(),
    ];
    div()
        .p_3()
        .rounded(px(CARD_RADIUS))
        .border_1()
        .border_color(if tone == Health::Failed {
            colors.danger_border
        } else {
            colors.subtle_border
        })
        .bg(colors.surface)
        .flex()
        .flex_col()
        .gap_2()
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(dot(colors, tone))
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(headline),
                ),
        )
        .children(detail.map(|detail| muted(colors, detail).line_clamp(2)))
        .children(primary.map(|primary| div().flex().child(primary)))
        .child(
            div()
                .flex()
                .items_center()
                .gap_1()
                .pt_1()
                .border_t_1()
                .border_color(colors.subtle_border)
                .when(tour_agent.is_shown || writing.is_some(), |row| {
                    row.child(dot(colors, tour_health).mr_1())
                })
                .child(
                    div().flex_1().min_w_0().flex().flex_col().children(
                        footnotes
                            .into_iter()
                            .flatten()
                            .map(|note| muted(colors, note).truncate()),
                    ),
                )
                .children(quiet),
        )
        .into_any_element()
}

// C · Pipeline: the work as three stops on a line, tour, review and fixes, each with its state; the stop the
// work is at is open and holds its actions, the others are one quiet line.

pub(super) fn variant_c(
    colors: &Colors,
    guided_tour: Option<&SidebarGuidedTour>,
    tour_agent: &SidebarTourAgent,
    review: &SidebarAiReview,
    cx: &Context<OneStepLayout>,
) -> AnyElement {
    let facts = ReviewFacts::of(&review.line);
    let writing = guided_tour.and_then(|tour| tour.stage.as_ref());
    let stop = |name: &'static str,
                health: Health,
                state: String,
                is_open: bool,
                body: Vec<AnyElement>,
                show: Option<AnyElement>,
                is_last: bool| {
        div()
            .flex()
            .gap_2()
            .child(
                div()
                    .flex_none()
                    .w(px(12.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .child(
                        div()
                            .mt(px(5.))
                            .size(px(if is_open { 10. } else { 8. }))
                            .rounded_full()
                            .when(is_open, |node| node.bg(health.color(colors)))
                            .when(!is_open, |node| {
                                node.border_2().border_color(health.color(colors))
                            }),
                    )
                    .when(!is_last, |rail| {
                        rail.child(div().flex_1().w(px(1.)).mt_1().bg(colors.border))
                    }),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .pb_2()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .text_xs()
                                    .font_weight(if is_open {
                                        FontWeight::SEMIBOLD
                                    } else {
                                        FontWeight::NORMAL
                                    })
                                    .text_color(if is_open {
                                        colors.text
                                    } else {
                                        colors.muted_text
                                    })
                                    .child(name),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_xs()
                                    .text_color(if health == Health::Failed {
                                        colors.danger
                                    } else {
                                        colors.muted_text
                                    })
                                    .child(state),
                            )
                            .children(show),
                    )
                    .when(is_open, |stop| stop.children(body)),
            )
    };
    let tour_open = writing.is_some() || guided_tour.and_then(|tour| tour.write).is_some();
    let (tour_state, tour_health) = if let Some(stage) = writing {
        (
            stage_line(&stage.line),
            if stage.can_try_again {
                Health::Failed
            } else {
                Health::Busy
            },
        )
    } else if guided_tour.and_then(|tour| tour.write).is_some() {
        ("not written".to_owned(), Health::Off)
    } else {
        let (status, health) = tour_status(tour_agent);
        (
            if health == Health::Off {
                "written".to_owned()
            } else {
                format!("written · agent {status}")
            },
            health,
        )
    };
    let mut tour_body: Vec<AnyElement> = Vec::new();
    if let Some(activity) = writing.and_then(|stage| stage.activity.clone()) {
        tour_body.push(
            muted(colors, activity)
                .truncate()
                .font_family(*crate::app::components::palette::CODE_FONT)
                .into_any_element(),
        );
    }
    if writing.is_some_and(|stage| stage.can_try_again) {
        tour_body.push(
            div()
                .flex()
                .child(write_tour(colors, "Try again", ButtonKind::Secondary, cx))
                .into_any_element(),
        );
    }
    match guided_tour.and_then(|tour| tour.write) {
        Some(WriteOffer::Available) => tour_body.push(
            div()
                .flex()
                .child(write_tour(
                    colors,
                    "Write guided tour",
                    ButtonKind::Secondary,
                    cx,
                ))
                .into_any_element(),
        ),
        Some(WriteOffer::Refused(refusal)) => {
            tour_body.push(muted(colors, refusal.reason()).into_any_element())
        }
        None => {}
    }
    let review_health = review_health(review);
    let review_open = review.fixes_to_send == 0;
    let review_state = if review.is_failure {
        "failed".to_owned()
    } else if review_health == Health::Busy || review_health == Health::Waiting {
        review.line.to_lowercase()
    } else {
        facts.headline.to_lowercase()
    };
    let mut review_body: Vec<AnyElement> = Vec::new();
    if review.is_failure {
        review_body.push(
            muted(colors, review.line.clone())
                .text_color(colors.danger)
                .into_any_element(),
        );
    }
    if let Some(by_severity) = facts.by_severity.clone() {
        let line = match &facts.dismissed {
            Some(dismissed) => format!("{by_severity} · {dismissed}"),
            None => by_severity,
        };
        review_body.push(muted(colors, line).into_any_element());
    }
    let mut buttons: Vec<AnyElement> = Vec::new();
    if review.has_findings {
        buttons.push(next_finding(colors, ButtonKind::Secondary, cx));
    }
    if review.has_findings || review.has_dismissed_findings {
        buttons.push(findings_list(colors, ButtonKind::Ghost, false, cx));
    }
    if let Some(action) = review.action {
        let label = action != AiReviewAction::RunAgain;
        let kind = if action == AiReviewAction::RunAgain {
            ButtonKind::Ghost
        } else {
            ButtonKind::Secondary
        };
        buttons.push(review_action(colors, action, kind, label, cx));
    }
    if !buttons.is_empty() {
        review_body.push(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap_1()
                .children(buttons)
                .into_any_element(),
        );
    }
    let fixes_open = !tour_open && review.fixes_to_send > 0;
    let (fixes_state, fixes_health) = match (review.fixes_to_send, facts.fix_count) {
        (0, 0) => ("none asked".to_owned(), Health::Off),
        (0, done) => (
            format!(
                "{done} pushed · {}",
                facts.fix_commits.first().cloned().unwrap_or_default()
            ),
            Health::Idle,
        ),
        (waiting, _) => (format!("{waiting} to send"), Health::Waiting),
    };
    let mut fixes_body: Vec<AnyElement> = Vec::new();
    if review.fixes_to_send > 0 {
        fixes_body.push(
            div()
                .flex()
                .child(send_fixes(
                    colors,
                    review.fixes_to_send,
                    ButtonKind::Primary,
                    cx,
                ))
                .into_any_element(),
        );
    }
    let tour_show = (guided_tour.is_some_and(|tour| tour.can_show_agent) || tour_agent.can_show)
        .then(|| show_tour_agent(colors, "p-c-show-tour", false, cx));
    div()
        .flex()
        .flex_col()
        .child(section_label(colors, "Agent work").mb_2())
        .child(stop(
            "Tour",
            tour_health,
            tour_state,
            tour_open,
            tour_body,
            tour_show,
            false,
        ))
        .child(stop(
            "AI review",
            review_health,
            review_state,
            review_open,
            review_body,
            review
                .can_show_agent
                .then(|| show_review_agent(colors, "p-c-show-review", false, cx)),
            false,
        ))
        .child(stop(
            "Fixes",
            fixes_health,
            fixes_state,
            fixes_open,
            fixes_body,
            None,
            true,
        ))
        .into_any_element()
}
