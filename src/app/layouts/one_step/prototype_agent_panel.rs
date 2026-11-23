//! PROTOTYPE (issues #174 and #157), throwaway: three designs of everything in the sidebar between the
//! progress bar and the steps: your review and Finish, the agents, GitHub sync, problems and the clone offer.
//! Start the app with GUIDED_REVIEW_PROTOTYPE=0|A|B|C and switch with the floating bar at the foot of the
//! window. 0 is the current design. Not for main.

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

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
use crate::app::components::palette::Tone;
use crate::app::components::palette::{CARD_RADIUS, Colors};
use crate::app::components::tooltip::tooltip;
use crate::app::review_session::ReviewSession;
use crate::app::sidebar_rows::{
    SidebarAiReview, SidebarGuidedTour, SidebarProblem, SidebarTourAgent, WriteOffer,
};
use crate::viewer::ProblemKind;
use crate::viewer::{AiReviewAction, Command, Intent};

const NAMES: [&str; 7] = [
    "Current",
    "A · Grouped by who",
    "B · Next step and status chips",
    "C · Pipeline: dots on a rail",
    "D · Pipeline: stepper and one panel",
    "E · Pipeline: quiet when fine",
    "F · Pipeline: icon rows",
];

static VARIANT: LazyLock<AtomicU8> = LazyLock::new(|| {
    let start = match std::env::var("GUIDED_REVIEW_PROTOTYPE").ok().as_deref() {
        Some("A" | "a") => 1,
        Some("B" | "b") => 2,
        Some("C" | "c") => 3,
        Some("D" | "d") => 4,
        Some("E" | "e") => 5,
        Some("F" | "f") => 6,
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
            VARIANT.store((variant() + by) % 7, Ordering::Relaxed);
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
                    .child(arrow("proto-prev", Icon::ArrowLeft, 6))
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

pub(super) type Draw = fn(&Colors, Inputs<'_>, &Context<OneStepLayout>) -> AnyElement;

pub(super) fn drawing() -> Option<Draw> {
    match variant() {
        1 => Some(variant_a as Draw),
        2 => Some(variant_b as Draw),
        3 => Some(variant_c as Draw),
        4 => Some(variant_d as Draw),
        5 => Some(variant_e as Draw),
        6 => Some(variant_f as Draw),
        _ => None,
    }
}

/// Everything the section draws, as the sidebar has it.
pub(super) struct Inputs<'a> {
    pub guided_tour: Option<&'a SidebarGuidedTour>,
    pub tour_agent: &'a SidebarTourAgent,
    pub ai_review: &'a SidebarAiReview,
    pub reviews: Option<AnyElement>,
    pub finish: Option<AnyElement>,
    pub clone_offer: Option<AnyElement>,
    pub problems: &'a [SidebarProblem],
    pub changes_not_shown: Option<SharedString>,
    pub sync: SyncFacts,
    pub your_review: Option<crate::app::sidebar_rows::SidebarYourReview>,
}

/// Staged by the prototype's snapshot scene: viewed marks that wait, as if GitHub had not taken them.
pub(super) static FAKE_SYNC_WAIT: AtomicBool = AtomicBool::new(false);
static WAITING_SINCE: Mutex<Option<Instant>> = Mutex::new(None);

/// GitHub's viewed marks, as the section says them: in sync, waiting since when, or off and why.
pub(super) struct SyncFacts {
    is_pull_request: bool,
    off_because: Option<SharedString>,
    waiting_files: usize,
    waiting_for: Duration,
}

impl SyncFacts {
    pub(super) fn of(
        is_pull_request: bool,
        off_because: Option<SharedString>,
        waiting_files: usize,
    ) -> Self {
        if FAKE_SYNC_WAIT.load(Ordering::Relaxed) {
            return Self {
                is_pull_request,
                off_because,
                waiting_files: 3,
                waiting_for: Duration::from_secs(4 * 60),
            };
        }
        let mut since = WAITING_SINCE
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let waiting_for = if waiting_files == 0 {
            *since = None;
            Duration::ZERO
        } else {
            since.get_or_insert_with(Instant::now).elapsed()
        };
        Self {
            is_pull_request,
            off_because,
            waiting_files,
            waiting_for,
        }
    }

    fn health(&self) -> Health {
        if self.off_because.is_some() {
            Health::Off
        } else if self.waiting_files > 0 {
            Health::Waiting
        } else {
            Health::Idle
        }
    }

    fn line(&self) -> String {
        if self.off_because.is_some() {
            return "viewed marks: sync off".to_owned();
        }
        match self.waiting_files {
            0 => "viewed marks in sync".to_owned(),
            files => format!(
                "{} viewed mark{} waiting · {}",
                files,
                if files == 1 { "" } else { "s" },
                how_long(self.waiting_for)
            ),
        }
    }
}

fn how_long(time: Duration) -> String {
    match time.as_secs() {
        0..60 => "under a minute".to_owned(),
        seconds @ 60..3600 => format!("{} min", seconds / 60),
        seconds => format!("{} h", seconds / 3600),
    }
}

/// Who a problem is about, so it sits with that part of the section.
#[derive(Clone, Copy, PartialEq)]
enum Owner {
    GitHub,
    Agents,
    ThisMac,
}

fn owner_of(problem: &SidebarProblem) -> Owner {
    match problem.problem.kind() {
        ProblemKind::ThreadsNotRead
        | ProblemKind::GitHubNotAnswering
        | ProblemKind::NotRead(_)
        | ProblemKind::ViewedNotSynced
        | ProblemKind::HandOffNotSent
        | ProblemKind::ReviewingMarkNotChanged
        | ProblemKind::ReadinessNotChanged
        | ProblemKind::MergingNotChanged
        | ProblemKind::CodeOnGitHub => Owner::GitHub,
        ProblemKind::TourNotRefreshed
        | ProblemKind::TourNotReloaded
        | ProblemKind::ShowAgentFailed
        | ProblemKind::UseAgentFailed => Owner::Agents,
        _ => Owner::ThisMac,
    }
}

fn problem_row(colors: &Colors, problem: &SidebarProblem) -> AnyElement {
    div()
        .flex()
        .gap_2()
        .child(
            sized_icon(Icon::CircleAlert, colors.danger, px(12.))
                .mt(px(2.))
                .flex_none(),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .child(
                    div()
                        .text_xs()
                        .text_color(colors.danger)
                        .child(problem.headline.clone()),
                )
                .children(problem.detail.clone().map(|detail| muted(colors, detail))),
        )
        .into_any_element()
}

fn problems_of<'a>(
    colors: &'a Colors,
    problems: &'a [SidebarProblem],
    owner: Owner,
) -> impl Iterator<Item = AnyElement> + 'a {
    problems
        .iter()
        .filter(move |problem| owner_of(problem) == owner)
        .map(move |problem| problem_row(colors, problem))
}

fn sync_row(colors: &Colors, sync: &SyncFacts) -> Div {
    let health = sync.health();
    div()
        .flex()
        .flex_col()
        .gap_0p5()
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(dot(colors, health))
                .child(div().flex_none().text_xs().font_weight(FontWeight::MEDIUM).child("GitHub"))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_xs()
                        .text_color(if health == Health::Waiting { colors.warning } else { colors.muted_text })
                        .child(sync.line()),
                ),
        )
        .children(sync.off_because.clone().map(|reason| muted(colors, reason).pl(px(15.))))
        .when(sync.waiting_files > 0, |row| {
            row.child(
                muted(colors, "Files you ticked show as unviewed on GitHub until this goes through. It tries again on its own.")
                    .pl(px(15.)),
            )
        })
}

fn changes_not_shown_row(colors: &Colors, notice: SharedString) -> AnyElement {
    div()
        .flex()
        .gap_2()
        .child(
            sized_icon(Icon::CircleAlert, colors.warning, px(12.))
                .mt(px(2.))
                .flex_none(),
        )
        .child(
            div()
                .flex_1()
                .text_xs()
                .text_color(colors.warning)
                .child(notice),
        )
        .into_any_element()
}

fn group(colors: &Colors, title: &'static str, parts: Vec<AnyElement>) -> Option<AnyElement> {
    (!parts.is_empty()).then(|| {
        div()
            .flex()
            .flex_col()
            .gap_1p5()
            .child(section_label(colors, title))
            .children(parts)
            .into_any_element()
    })
}

// A · Grouped by who: one group per party, each with a small heading and only when it has something:
// You (your review and Finish), Agents, GitHub (sync and what GitHub refused), This Mac (the clone,
// saving, the app). A problem sits in the group it is about.

pub(super) fn variant_a(
    colors: &Colors,
    inputs: Inputs<'_>,
    cx: &Context<OneStepLayout>,
) -> AnyElement {
    let you: Vec<AnyElement> = inputs
        .reviews
        .into_iter()
        .chain(inputs.finish)
        .chain(
            inputs
                .changes_not_shown
                .map(|notice| changes_not_shown_row(colors, notice)),
        )
        .collect();
    let agents: Vec<AnyElement> = std::iter::once(agent_rows(
        colors,
        inputs.guided_tour,
        inputs.tour_agent,
        inputs.ai_review,
        cx,
    ))
    .chain(problems_of(colors, inputs.problems, Owner::Agents))
    .collect();
    let github: Vec<AnyElement> = inputs
        .sync
        .is_pull_request
        .then(|| sync_row(colors, &inputs.sync).into_any_element())
        .into_iter()
        .chain(problems_of(colors, inputs.problems, Owner::GitHub))
        .collect();
    let this_mac: Vec<AnyElement> = problems_of(colors, inputs.problems, Owner::ThisMac)
        .chain(inputs.clone_offer)
        .collect();
    div()
        .flex()
        .flex_col()
        .gap_4()
        .children(group(colors, "You", you))
        .children(group(colors, "Agents", agents))
        .children(group(colors, "GitHub", github))
        .children(group(colors, "This Mac", this_mac))
        .into_any_element()
}

// B · Next step and status chips: your review and Finish on top, then one card with the next thing to do,
// then one line of chips, one per party, coloured by its health. A chip opens its details under the line;
// a chip with a problem opens on its own.

static OPEN_CHIP: AtomicU8 = AtomicU8::new(u8::MAX);

pub(super) fn variant_b(
    colors: &Colors,
    inputs: Inputs<'_>,
    cx: &Context<OneStepLayout>,
) -> AnyElement {
    let agent_problems = inputs
        .problems
        .iter()
        .filter(|problem| owner_of(problem) == Owner::Agents)
        .count();
    let github_problems = inputs
        .problems
        .iter()
        .filter(|problem| owner_of(problem) == Owner::GitHub)
        .count();
    let mac_problems = inputs
        .problems
        .iter()
        .filter(|problem| owner_of(problem) == Owner::ThisMac)
        .count();
    let review_health = review_health(inputs.ai_review);
    let (tour_line, tour_health) = tour_status(inputs.tour_agent);
    let agents_health =
        if agent_problems > 0 || review_health == Health::Failed || tour_health == Health::Failed {
            Health::Failed
        } else if review_health == Health::Waiting || tour_health == Health::Waiting {
            Health::Waiting
        } else if review_health == Health::Busy || tour_health == Health::Busy {
            Health::Busy
        } else {
            Health::Idle
        };
    let github_health = if github_problems > 0 {
        Health::Failed
    } else {
        inputs.sync.health()
    };
    let mac_health = if mac_problems > 0 {
        Health::Failed
    } else {
        Health::Idle
    };
    let chips: Vec<(u8, &'static str, Health, Option<String>)> = vec![
        (0, "Agents", agents_health, None),
        (
            1,
            "GitHub",
            github_health,
            match (github_problems, inputs.sync.waiting_files) {
                (0, 0) => None,
                (0, files) => Some(format!("{files} waiting")),
                (problems, _) => Some(problems.to_string()),
            },
        ),
        (
            2,
            "This Mac",
            mac_health,
            (mac_problems > 0).then(|| mac_problems.to_string()),
        ),
    ];
    let chosen = OPEN_CHIP.load(Ordering::Relaxed);
    let open = if chosen == u8::MAX {
        chips
            .iter()
            .find(|(_, _, health, _)| *health == Health::Failed)
            .map(|(key, ..)| *key)
    } else if chosen == u8::MAX - 1 {
        None
    } else {
        Some(chosen)
    };
    let strip =
        div()
            .flex()
            .flex_wrap()
            .gap_1()
            .children(chips.iter().map(|(key, name, health, note)| {
                let key = *key;
                let is_open = open == Some(key);
                let tone = match health {
                    Health::Failed => Tone::Danger,
                    Health::Waiting => Tone::Warning,
                    _ => Tone::Neutral,
                };
                div()
                    .id(SharedString::from(format!("proto-chip-{key}")))
                    .cursor_pointer()
                    .child(
                        crate::app::components::chip::chip(colors, tone)
                            .py_0p5()
                            .when(is_open, |chip| chip.border_color(colors.accent))
                            .child(dot(colors, *health))
                            .child(*name)
                            .children(note.clone()),
                    )
                    .on_click(cx.listener(move |_, _, _, cx| {
                        OPEN_CHIP.store(if is_open { u8::MAX - 1 } else { key }, Ordering::Relaxed);
                        cx.notify();
                    }))
            }));
    let detail: Option<AnyElement> =
        match open {
            Some(0) => Some(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .when(inputs.tour_agent.is_shown, |detail| {
                        detail.child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(dot(colors, tour_health))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .truncate()
                                        .text_xs()
                                        .child(format!("Tour agent: {tour_line}")),
                                )
                                .children(inputs.tour_agent.can_show.then(|| {
                                    show_tour_agent(colors, "p-b-tour-detail", false, cx)
                                })),
                        )
                    })
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(dot(colors, review_health))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_xs()
                                    .child(format!("Review agent: {}", inputs.ai_review.line)),
                            )
                            .children(inputs.ai_review.can_show_agent.then(|| {
                                show_review_agent(colors, "p-b-review-detail", false, cx)
                            })),
                    )
                    .children(problems_of(colors, inputs.problems, Owner::Agents))
                    .into_any_element(),
            ),
            Some(1) => Some(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(sync_row(colors, &inputs.sync))
                    .children(problems_of(colors, inputs.problems, Owner::GitHub))
                    .into_any_element(),
            ),
            Some(2) => Some(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .children(problems_of(colors, inputs.problems, Owner::ThisMac))
                    .when(mac_problems == 0, |detail| {
                        detail.child(muted(colors, "Nothing wrong on this Mac"))
                    })
                    .into_any_element(),
            ),
            _ => None,
        };
    div()
        .flex()
        .flex_col()
        .gap_3()
        .children(inputs.reviews)
        .children(inputs.finish)
        .children(
            inputs
                .changes_not_shown
                .map(|notice| changes_not_shown_row(colors, notice)),
        )
        .child(next_step_card(
            colors,
            inputs.guided_tour,
            inputs.tour_agent,
            inputs.ai_review,
            cx,
        ))
        .child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(strip)
                .children(detail.map(|detail| {
                    div()
                        .p_2()
                        .rounded(px(CARD_RADIUS))
                        .bg(colors.inset)
                        .child(detail)
                })),
        )
        .children(inputs.clone_offer)
        .into_any_element()
}

// C · One pipeline: the review as stops on one line, from the tour to GitHub: Tour, AI review, Fixes,
// Your review, GitHub. Each stop says its state in one line; a stop that needs you is open and holds
// its buttons and its problems. Problems about this Mac sit in a red box above the line.

#[allow(clippy::too_many_arguments)]
fn stop(
    colors: &Colors,
    name: &'static str,
    health: Health,
    state: String,
    is_open: bool,
    body: Vec<AnyElement>,
    show: Option<AnyElement>,
    is_last: bool,
) -> Div {
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
                                .flex_none()
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
                                .text_color(match health {
                                    Health::Failed => colors.danger,
                                    Health::Waiting => colors.warning,
                                    _ => colors.muted_text,
                                })
                                .child(state),
                        )
                        .children(show),
                )
                .when(is_open, |stop| stop.children(body)),
        )
}

fn pipeline_of(colors: &Colors, inputs: Inputs<'_>, cx: &Context<OneStepLayout>) -> Pipeline {
    let review = inputs.ai_review;
    let tour_agent = inputs.tour_agent;
    let guided_tour = inputs.guided_tour;
    let facts = ReviewFacts::of(&review.line);
    let writing = guided_tour.and_then(|tour| tour.stage.as_ref());
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
    if tour_agent.line.contains(" is off") {
        tour_body.push(muted(colors, tour_agent.line.to_string()).into_any_element());
    }
    let agent_problems: Vec<AnyElement> =
        problems_of(colors, inputs.problems, Owner::Agents).collect();
    let tour_open = tour_open
        || !agent_problems.is_empty()
        || tour_health == Health::Off && tour_agent.line.contains(" is off");
    tour_body.extend(agent_problems);
    let review_health = review_health(review);
    let review_open = review.fixes_to_send == 0 && review_health != Health::Off
        || review.action == Some(AiReviewAction::Run);
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
    let fixes_open = review.fixes_to_send > 0;
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
    let fixes_body: Vec<AnyElement> = (review.fixes_to_send > 0)
        .then(|| {
            div()
                .flex()
                .child(send_fixes(
                    colors,
                    review.fixes_to_send,
                    ButtonKind::Primary,
                    cx,
                ))
                .into_any_element()
        })
        .into_iter()
        .collect();
    let yours: Vec<AnyElement> = inputs
        .changes_not_shown
        .map(|notice| changes_not_shown_row(colors, notice))
        .into_iter()
        .chain(inputs.reviews)
        .chain(inputs.finish)
        .collect();
    let github_problems: Vec<AnyElement> =
        problems_of(colors, inputs.problems, Owner::GitHub).collect();
    let github_health = if github_problems.is_empty() {
        inputs.sync.health()
    } else {
        Health::Failed
    };
    let github_state = if github_problems.is_empty() {
        inputs.sync.line()
    } else {
        format!(
            "{} problem{}",
            github_problems.len(),
            if github_problems.len() == 1 { "" } else { "s" }
        )
    };
    let github_open = github_health != Health::Idle;
    let mut github_body: Vec<AnyElement> = Vec::new();
    if !github_problems.is_empty()
        && (inputs.sync.waiting_files > 0 || inputs.sync.off_because.is_some())
    {
        github_body.push(muted(colors, inputs.sync.line()).into_any_element());
    }
    if let Some(reason) = inputs.sync.off_because.clone() {
        github_body.push(muted(colors, reason).into_any_element());
    }
    if inputs.sync.waiting_files > 0 {
        github_body.push(muted(colors, "Files you ticked show as unviewed on GitHub until this goes through. It tries again on its own.").into_any_element());
    }
    github_body.extend(github_problems);
    let mac_problems: Vec<AnyElement> =
        problems_of(colors, inputs.problems, Owner::ThisMac).collect();
    let tour_show = (guided_tour.is_some_and(|tour| tour.can_show_agent) || tour_agent.can_show)
        .then(|| show_tour_agent(colors, "p-c-show-tour", false, cx));
    let (yours_state, yours_health) = match &inputs.your_review {
        Some(stance) if stance.is_marked_done => {
            (format!("{} · marked done", stance.title), Health::Idle)
        }
        Some(stance) if stance.age == crate::viewer::StanceAge::BeforeNewCommits => {
            ("new commits since".to_owned(), Health::Waiting)
        }
        Some(stance) => (stance.title.to_string(), Health::Idle),
        None if yours.is_empty() => ("nothing to send".to_owned(), Health::Off),
        None => ("not sent yet".to_owned(), Health::Off),
    };
    let yours_open = !yours.is_empty();
    let mut stops = vec![
        Stop {
            key: 0,
            name: "Tour",
            icon: Icon::Sparkles,
            health: tour_health,
            state: tour_state,
            open: tour_open,
            body: tour_body,
            show: tour_show,
        },
        Stop {
            key: 1,
            name: "AI review",
            icon: Icon::ListChecks,
            health: review_health,
            state: review_state,
            open: review_open,
            body: review_body,
            show: review
                .can_show_agent
                .then(|| show_review_agent(colors, "p-c-show-review", false, cx)),
        },
        Stop {
            key: 2,
            name: "Fixes",
            icon: Icon::Zap,
            health: fixes_health,
            state: fixes_state,
            open: fixes_open,
            body: fixes_body,
            show: None,
        },
        Stop {
            key: 3,
            name: "Your review",
            icon: Icon::Send,
            health: yours_health,
            state: yours_state,
            open: yours_open,
            body: yours,
            show: None,
        },
    ];
    if inputs.sync.is_pull_request {
        stops.push(Stop {
            key: 4,
            name: "GitHub",
            icon: Icon::Globe,
            health: github_health,
            state: github_state,
            open: github_open,
            body: github_body,
            show: None,
        });
    }
    Pipeline {
        stops,
        mac_problems,
        clone_offer: inputs.clone_offer,
    }
}

struct Stop {
    key: u8,
    name: &'static str,
    icon: Icon,
    health: Health,
    state: String,
    open: bool,
    body: Vec<AnyElement>,
    show: Option<AnyElement>,
}

impl Stop {
    fn needs_you(&self) -> bool {
        matches!(self.health, Health::Failed | Health::Waiting)
            || self.open && !self.body.is_empty()
    }
}

struct Pipeline {
    stops: Vec<Stop>,
    mac_problems: Vec<AnyElement>,
    clone_offer: Option<AnyElement>,
}

fn mac_box(colors: &Colors, problems: Vec<AnyElement>) -> Option<AnyElement> {
    (!problems.is_empty()).then(|| {
        div()
            .p_2()
            .rounded(px(CARD_RADIUS))
            .border_1()
            .border_color(colors.danger_border)
            .bg(colors.danger_background)
            .flex()
            .flex_col()
            .gap_1()
            .children(problems)
            .into_any_element()
    })
}

fn state_color(colors: &Colors, health: Health) -> Rgba {
    match health {
        Health::Failed => colors.danger,
        Health::Waiting => colors.warning,
        _ => colors.muted_text,
    }
}

// C · One pipeline, as first drawn: dots on a rail.

pub(super) fn variant_c(
    colors: &Colors,
    inputs: Inputs<'_>,
    cx: &Context<OneStepLayout>,
) -> AnyElement {
    let pipeline = pipeline_of(colors, inputs, cx);
    let last = pipeline.stops.len().saturating_sub(1);
    div()
        .flex()
        .flex_col()
        .gap_2()
        .children(mac_box(colors, pipeline.mac_problems))
        .child(
            div()
                .flex()
                .flex_col()
                .children(pipeline.stops.into_iter().enumerate().map(|(at, s)| {
                    stop(
                        colors,
                        s.name,
                        s.health,
                        s.state,
                        s.open,
                        s.body,
                        s.show,
                        at == last,
                    )
                })),
        )
        .children(pipeline.clone_offer)
        .into_any_element()
}

// D · Stepper: the stops as round icons in one row, tinted by their health, no edges. Under the row, one
// card for one stop: the one that needs you, or the one clicked. The card is F's raised card.

static PICKED_STOP: AtomicU8 = AtomicU8::new(u8::MAX);

fn tone_of(health: Health) -> Tone {
    match health {
        Health::Failed => Tone::Danger,
        Health::Waiting => Tone::Warning,
        Health::Idle => Tone::Success,
        Health::Busy => Tone::Accent,
        Health::Off => Tone::Neutral,
    }
}

fn health_text(colors: &Colors, health: Health) -> Rgba {
    match health {
        Health::Off => colors.muted_text,
        health => health.color(colors),
    }
}

/// F's card: an icon tile, the stop's name, its state in the colour of its health, then its buttons.
fn stop_card(colors: &Colors, s: Stop) -> Div {
    div()
        .p_2()
        .rounded(px(CARD_RADIUS))
        .bg(colors.surface)
        .shadow(colors.card_shadow())
        .flex()
        .flex_col()
        .gap_1p5()
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .min_h(px(24.))
                .child(
                    div()
                        .flex_none()
                        .size(px(20.))
                        .rounded(px(6.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .bg(colors.tone(tone_of(s.health)).background)
                        .child(sized_icon(s.icon, health_text(colors, s.health), px(12.))),
                )
                .child(
                    div()
                        .flex_none()
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(s.name),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_xs()
                        .text_color(health_text(colors, s.health))
                        .child(s.state),
                )
                .children(s.show),
        )
        .children(s.body)
}

fn pick_shown(stops: &[Stop]) -> u8 {
    let picked = PICKED_STOP.load(Ordering::Relaxed);
    if stops.iter().any(|s| s.key == picked) {
        return picked;
    }
    stops
        .iter()
        .find(|s| s.health == Health::Failed)
        .or_else(|| stops.iter().find(|s| s.health == Health::Waiting))
        .or_else(|| stops.iter().find(|s| s.open && !s.body.is_empty()))
        .map_or(1, |s| s.key)
}

pub(super) fn variant_d(
    colors: &Colors,
    inputs: Inputs<'_>,
    cx: &Context<OneStepLayout>,
) -> AnyElement {
    let pipeline = pipeline_of(colors, inputs, cx);
    let shown_key = pick_shown(&pipeline.stops);
    let count = pipeline.stops.len();
    let mut nodes: Vec<AnyElement> = Vec::new();
    let mut card: Option<Stop> = None;
    for (at, s) in pipeline.stops.into_iter().enumerate() {
        let key = s.key;
        let is_shown = key == shown_key;
        let tone = colors.tone(tone_of(s.health));
        let (ground, ink) = match (is_shown, s.health) {
            (true, Health::Off) => (colors.current, colors.accent),
            (true, _) => (tone.background, tone.text),
            (false, Health::Off) => (gpui::transparent_black().into(), colors.muted_text),
            (false, _) => (gpui::transparent_black().into(), tone.text),
        };
        nodes.push(
            div()
                .id(SharedString::from(format!("p-d-node-{key}")))
                .flex_none()
                .w(px(44.))
                .flex()
                .flex_col()
                .items_center()
                .gap_1()
                .cursor_pointer()
                .child(
                    div()
                        .size(px(if is_shown { 32. } else { 28. }))
                        .mt(px(if is_shown { 0. } else { 2. }))
                        .rounded_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .bg(ground)
                        .hover(|node| node.opacity(0.85))
                        .child(sized_icon(s.icon, ink, px(14.))),
                )
                .child(
                    div()
                        .text_xs()
                        .font_weight(if is_shown {
                            FontWeight::SEMIBOLD
                        } else {
                            FontWeight::NORMAL
                        })
                        .text_color(if is_shown {
                            colors.text
                        } else {
                            colors.muted_text
                        })
                        .whitespace_nowrap()
                        .child(match s.name {
                            "AI review" => "Review",
                            "Your review" => "You",
                            name => name,
                        }),
                )
                .tooltip({
                    let tip = format!("{}: {}", s.name, s.state);
                    tooltip(move |_| tip.clone().into())
                })
                .on_click(cx.listener(move |_, _, _, cx| {
                    PICKED_STOP.store(key, Ordering::Relaxed);
                    cx.notify();
                }))
                .into_any_element(),
        );
        if at + 1 < count {
            nodes.push(
                div()
                    .flex_1()
                    .h(px(2.))
                    .mt(px(14.))
                    .rounded_full()
                    .bg(colors.hover)
                    .into_any_element(),
            );
        }
        if is_shown {
            card = Some(s);
        }
    }
    div()
        .flex()
        .flex_col()
        .gap_2()
        .children(mac_box(colors, pipeline.mac_problems))
        .child(div().flex().items_start().children(nodes))
        .children(card.map(|s| stop_card(colors, s)))
        .children(pipeline.clone_offer)
        .into_any_element()
}

// E · Quiet when fine: the stops that need nothing fold into one line of small ticks ("Tour ✓ · Fixes ·
// GitHub ✓"); only the stops that need you are drawn, each as a card with its buttons.

pub(super) fn variant_e(
    colors: &Colors,
    inputs: Inputs<'_>,
    cx: &Context<OneStepLayout>,
) -> AnyElement {
    let pipeline = pipeline_of(colors, inputs, cx);
    let (loud, quiet): (Vec<Stop>, Vec<Stop>) =
        pipeline.stops.into_iter().partition(Stop::needs_you);
    let quiet_line = (!quiet.is_empty()).then(|| {
        div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_x_3()
            .gap_y_1()
            .children(quiet.into_iter().map(|s| {
                let tip = format!("{}: {}", s.name, s.state);
                div()
                    .id(SharedString::from(format!("p-e-quiet-{}", s.key)))
                    .flex()
                    .items_center()
                    .gap_1()
                    .text_xs()
                    .text_color(colors.muted_text)
                    .child(match s.health {
                        Health::Idle => {
                            sized_icon(Icon::Check, colors.success, px(12.)).into_any_element()
                        }
                        Health::Busy => dot(colors, Health::Busy).into_any_element(),
                        _ => sized_icon(Icon::Minus, colors.muted_text, px(12.)).into_any_element(),
                    })
                    .child(s.name)
                    .children(s.show)
                    .tooltip(tooltip(move |_| tip.clone().into()))
            }))
    });
    div()
        .flex()
        .flex_col()
        .gap_2()
        .children(mac_box(colors, pipeline.mac_problems))
        .children(loud.into_iter().map(|s| {
            div()
                .p_2()
                .rounded(px(CARD_RADIUS))
                .border_1()
                .border_color(match s.health {
                    Health::Failed => colors.danger_border,
                    Health::Waiting => colors.warning_border,
                    _ => colors.subtle_border,
                })
                .bg(colors.surface)
                .flex()
                .flex_col()
                .gap_1p5()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(sized_icon(s.icon, s.health.color(colors), px(13.)))
                        .child(
                            div()
                                .flex_none()
                                .text_xs()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(s.name),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_xs()
                                .text_color(state_color(colors, s.health))
                                .child(s.state),
                        )
                        .children(s.show),
                )
                .children(s.body)
        }))
        .children(quiet_line)
        .children(pipeline.clone_offer)
        .into_any_element()
}

// F · Icon rows: every stop is one row with its own icon on the rail in place of a dot, the state as a
// small chip at the row's end. The stop that needs you is raised as a card around its row and buttons.

pub(super) fn variant_f(
    colors: &Colors,
    inputs: Inputs<'_>,
    cx: &Context<OneStepLayout>,
) -> AnyElement {
    let pipeline = pipeline_of(colors, inputs, cx);
    let rows = pipeline.stops.into_iter().map(|s| {
        let is_raised = s.needs_you();
        let tone = match s.health {
            Health::Failed => Tone::Danger,
            Health::Waiting => Tone::Warning,
            Health::Idle => Tone::Success,
            Health::Busy => Tone::Accent,
            Health::Off => Tone::Neutral,
        };
        let head = div()
            .flex()
            .items_center()
            .gap_2()
            .h(px(26.))
            .child(
                div()
                    .flex_none()
                    .size(px(20.))
                    .rounded(px(6.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(colors.inset)
                    .child(sized_icon(
                        s.icon,
                        if s.health == Health::Off {
                            colors.muted_text
                        } else {
                            s.health.color(colors)
                        },
                        px(12.),
                    )),
            )
            .child(
                div()
                    .flex_1()
                    .text_xs()
                    .font_weight(if is_raised {
                        FontWeight::SEMIBOLD
                    } else {
                        FontWeight::NORMAL
                    })
                    .text_color(if is_raised {
                        colors.text
                    } else {
                        colors.muted_text
                    })
                    .child(s.name),
            )
            .children(s.show)
            .child(if is_raised {
                div()
                    .text_xs()
                    .text_color(health_text(colors, s.health))
                    .child(s.state)
                    .into_any_element()
            } else {
                crate::app::components::chip::chip(colors, tone)
                    .max_w(px(150.))
                    .child(div().truncate().child(s.state))
                    .into_any_element()
            });
        div()
            .when(is_raised, |row| {
                row.p_2()
                    .rounded(px(CARD_RADIUS))
                    .border_1()
                    .border_color(colors.subtle_border)
                    .bg(colors.surface)
            })
            .when(!is_raised, |row| row.px_2())
            .flex()
            .flex_col()
            .gap_1p5()
            .child(head)
            .when(is_raised, |row| row.children(s.body))
    });
    div()
        .flex()
        .flex_col()
        .gap_1()
        .children(mac_box(colors, pipeline.mac_problems))
        .children(rows)
        .children(pipeline.clone_offer)
        .into_any_element()
}

fn agent_rows(
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

fn next_step_card(
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
