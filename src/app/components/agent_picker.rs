//! Picking an agent by hand, for when the one the app found is not the right one: every Claude
//! terminal with where it works and which agent it is now, a button that shows it and one that makes
//! it the tour agent or the review agent.

use std::time::Instant;

use gpui::{
    AnyElement, Context, EventEmitter, FocusHandle, FontWeight, InteractiveElement, IntoElement,
    KeyDownEvent, MouseButton, ParentElement, Render, Rgba, SharedString,
    StatefulInteractiveElement, Styled, Window, div, px,
};

use crate::viewer::{AgentChoice, AgentRole, AgentStatus, Presence, TerminalId};

use super::button::{ButtonKind, ButtonSize, icon_button, icon_label_button};
use super::chip::chip;
use super::icon::{Icon, sized_icon};
use super::key_cap::{KeyLabel, key_hint};
use super::motion::OVERLAY;
use super::overlay::{close_presence, modal_layer_for, overlay_panel, picker_footer, rise_for};
use super::palette::{CODE_FONT, Colors, Tone};
use super::search_row::search_row;
use super::selectable_text::SelectableText;
use super::text_box::TextBox;
use super::text_input::Lines;

pub enum AgentPickerEvent {
    Use {
        terminal: TerminalId,
        role: AgentRole,
    },
    Show(TerminalId),
}

/// The terminals as herdr listed them when the picker opened.
enum Listed {
    Looking,
    Found(Vec<AgentChoice>),
    Failed(String),
}

pub struct AgentPicker {
    presence: Presence,
    focus: FocusHandle,
    owner_focus: FocusHandle,
    listed: Listed,
    /// The agent the picked terminal becomes.
    role: AgentRole,
    /// Says what that agent is for: "this PR's tour agent", "this review's AI review agent".
    of_what: SharedString,
    query: String,
}

impl EventEmitter<AgentPickerEvent> for AgentPicker {}

impl AgentPicker {
    pub const KEY_CONTEXT: &str = "AgentPicker";

    pub fn new(owner_focus: FocusHandle, cx: &mut Context<Self>) -> Self {
        Self {
            presence: Presence::lasting(OVERLAY),
            focus: cx.focus_handle(),
            owner_focus,
            listed: Listed::Looking,
            role: AgentRole::Tour,
            of_what: SharedString::default(),
            query: String::new(),
        }
    }

    pub fn is_open(&self) -> bool {
        self.presence.is_open()
    }

    /// Opens on "looking", with the search box taking the keys; `listed` gives the terminals once
    /// herdr answers.
    pub fn open(
        &mut self,
        role: AgentRole,
        of_what: SharedString,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.role = role;
        self.of_what = of_what;
        self.listed = Listed::Looking;
        self.query.clear();
        self.presence.open(Instant::now());
        window.focus(&self.focus, cx);
        cx.notify();
    }

    pub fn listed(&mut self, choices: Result<Vec<AgentChoice>, String>, cx: &mut Context<Self>) {
        self.listed = match choices {
            Ok(choices) => Listed::Found(choices),
            Err(reason) => Listed::Failed(reason),
        };
        cx.notify();
    }

    pub fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.presence.is_open() {
            return;
        }
        close_presence(self, cx, |this| &mut this.presence);
        window.focus(&self.owner_focus, cx);
        cx.notify();
    }

    /// Three terminals as herdr might list them while the reviewer's agent is in another clone.
    #[cfg(feature = "snapshots")]
    pub fn stage_sample(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        use crate::review_subject::BranchName;
        let choice =
            |terminal: &str, space: &str, branch: &str, status, marks: (bool, bool, bool)| {
                AgentChoice {
                    terminal: TerminalId::new(terminal),
                    space: space.to_owned(),
                    folder: format!("/Users/you/Code/{space}").into(),
                    repo: "ekvedaras/guided-review".parse().ok(),
                    branch: BranchName::new(branch).ok(),
                    status,
                    is_tour_agent: marks.0,
                    is_review_agent: marks.1,
                    works_on_the_code: marks.2,
                    started_by_app: false,
                }
            };
        self.open(AgentRole::Tour, "this PR's tour agent".into(), window, cx);
        self.listed(
            Ok(vec![
                choice(
                    "t3",
                    "guided-review",
                    "main",
                    AgentStatus::Working,
                    (true, false, false),
                ),
                choice(
                    "t7",
                    "claude-a",
                    "files-list",
                    AgentStatus::Idle,
                    (false, true, true),
                ),
                choice(
                    "t9",
                    "claude-b",
                    "settings-pages",
                    AgentStatus::WaitingForYou,
                    (false, false, false),
                ),
            ]),
            cx,
        );
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.key == "escape" {
            cx.stop_propagation();
            self.close(window, cx);
        }
    }

    fn query_box(&self, cx: &mut Context<Self>) -> TextBox {
        let (on_edit, on_cancel) = (cx.entity(), cx.entity());
        TextBox::new(
            "agent-picker-query",
            &self.focus,
            &self.query,
            Lines::Single,
        )
        .label("Search the agents")
        .on_edit(move |query, _, cx| {
            on_edit.update(cx, |this, cx| {
                this.query = query;
                cx.notify();
            });
        })
        .on_cancel(move |window, cx| on_cancel.update(cx, |this, cx| this.close(window, cx)))
    }

    fn choice_row(
        &self,
        colors: &Colors,
        choice: &AgentChoice,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let terminal = choice.terminal.clone();
        let where_it_works = match (&choice.repo, &choice.branch) {
            (Some(repo), Some(branch)) => format!("{} · {}", repo.repo, branch.as_str()),
            (None, Some(branch)) => format!("no origin · {}", branch.as_str()),
            _ => "not a git checkout".to_owned(),
        };
        let picked_role_tone = |role: AgentRole| {
            if role == self.role {
                Tone::Accent
            } else {
                Tone::Neutral
            }
        };
        let tags = [
            choice
                .is_tour_agent
                .then(|| ("tour agent", picked_role_tone(AgentRole::Tour))),
            choice
                .is_review_agent
                .then(|| ("review agent", picked_role_tone(AgentRole::Review))),
            choice
                .works_on_the_code
                .then_some(("on its code", Tone::Success)),
            choice
                .started_by_app
                .then_some(("started by the app", Tone::Neutral)),
        ]
        .into_iter()
        .flatten()
        .map(|(words, tone)| chip(colors, tone).text_xs().child(words));
        let show = {
            let terminal = terminal.clone();
            icon_label_button(
                colors,
                SharedString::from(format!("agent-picker-show-{terminal}")),
                ButtonKind::Ghost,
                ButtonSize::Small,
                Icon::Terminal,
                "Show",
            )
            .on_click(cx.listener(move |_, _, _, cx| {
                cx.emit(AgentPickerEvent::Show(terminal.clone()));
            }))
        };
        let role = self.role;
        let pick = (!choice.is_agent_as(role)).then(|| {
            let terminal = terminal.clone();
            icon_label_button(
                colors,
                SharedString::from(format!("agent-picker-use-{terminal}")),
                ButtonKind::Secondary,
                ButtonSize::Small,
                Icon::Check,
                format!("Use {}", choice.space),
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                cx.emit(AgentPickerEvent::Use {
                    terminal: terminal.clone(),
                    role,
                });
                this.close(window, cx);
            }))
        });
        div()
            .px_5()
            .py_3()
            .flex()
            .items_center()
            .gap_4()
            .border_b_1()
            .border_color(colors.subtle_border)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .items_baseline()
                            .gap_2()
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(colors.muted_text)
                                    .child(format!("{terminal} in")),
                            )
                            .child(
                                div()
                                    .text_lg()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(colors.text)
                                    .child(choice.space.clone()),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(status_color(colors, choice.status))
                                    .child(choice.status.to_string()),
                            )
                            .children(tags),
                    )
                    .child(
                        div()
                            .text_sm()
                            .font_family(*CODE_FONT)
                            .text_color(colors.muted_text)
                            .child(SelectableText::new(
                                format!("agent-picker-folder-{terminal}"),
                                choice.folder.display().to_string(),
                            )),
                    )
                    .child(div().text_xs().text_color(colors.muted_text).child(
                        SelectableText::new(
                            format!("agent-picker-code-{terminal}"),
                            where_it_works,
                        ),
                    )),
            )
            .child(div().flex().flex_none().gap_2().child(show).children(pick))
    }
}

/// Working is the accent, idle a success, waiting on the reviewer a warning, failed red; a terminal
/// with no status is quiet.
fn status_color(colors: &Colors, status: AgentStatus) -> Rgba {
    match status {
        AgentStatus::Working => colors.accent,
        AgentStatus::Idle => colors.success,
        AgentStatus::WaitingForYou => colors.warning,
        AgentStatus::Failed => colors.danger,
        AgentStatus::Unreported => colors.muted_text,
    }
}

impl Render for AgentPicker {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.presence.shown().is_none() {
            return div().into_any_element();
        }
        let colors = Colors::active(cx);
        let title = match self.role {
            AgentRole::Tour => "Pick the tour agent",
            AgentRole::Review => "Pick the review agent",
        };
        let header = div()
            .px_5()
            .pt_4()
            .pb_3()
            .flex()
            .items_center()
            .gap_3()
            .border_b_1()
            .border_color(colors.subtle_border)
            .child(sized_icon(Icon::Terminal, colors.accent, px(18.)))
            .child(
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .text_base()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(title),
                    )
                    .child(div().text_xs().text_color(colors.muted_text).child(format!(
                        "Every Claude terminal in herdr. The one you use becomes {}.",
                        self.of_what
                    ))),
            )
            .child(
                icon_button(
                    &colors,
                    "agent-picker-close",
                    ButtonKind::Ghost,
                    ButtonSize::Small,
                    Icon::Close,
                    "Close",
                )
                .on_click(cx.listener(|this, _, window, cx| this.close(window, cx))),
            );
        let quiet = |text: String| {
            div()
                .px_5()
                .py_4()
                .text_sm()
                .text_color(colors.muted_text)
                .child(SelectableText::new("agent-picker-note", text))
                .into_any_element()
        };
        let body: Vec<AnyElement> = match &self.listed {
            Listed::Looking => vec![quiet("Looking at herdr's terminals...".to_owned())],
            Listed::Failed(reason) => {
                vec![quiet(format!("Could not list the terminals: {reason}"))]
            }
            Listed::Found(choices) if choices.is_empty() => {
                vec![quiet("No terminal in herdr runs Claude.".to_owned())]
            }
            Listed::Found(choices) => {
                let shown: Vec<AgentChoice> = choices
                    .iter()
                    .filter(|choice| choice.matches(&self.query))
                    .cloned()
                    .collect();
                if shown.is_empty() {
                    vec![quiet(
                        "No agent's space, folder or branch holds every word.".to_owned(),
                    )]
                } else {
                    shown
                        .iter()
                        .map(|choice| self.choice_row(&colors, choice, cx).into_any_element())
                        .collect()
                }
            }
        };
        modal_layer_for(&colors, "agent-picker-scrim", &self.presence)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| this.close(window, cx)),
            )
            .child(rise_for(
                overlay_panel(&colors)
                    .id("agent-picker")
                    .key_context(Self::KEY_CONTEXT)
                    .on_key_down(cx.listener(Self::on_key_down))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .mt(px(80.))
                    .w(px(820.))
                    .max_h(px(600.))
                    .flex()
                    .flex_col()
                    .child(header)
                    .child(search_row(
                        &colors,
                        self.query_box(cx),
                        "Search by space, folder, repo or branch",
                        self.query.is_empty(),
                        None,
                    ))
                    .child(
                        div()
                            .id("agent-picker-rows")
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .children(body),
                    )
                    .child(
                        picker_footer(&colors)
                            .child(key_hint(&colors, &[KeyLabel::Text("esc")], "close"))
                            .child(div().flex_1())
                            .child(SharedString::from("Show brings the terminal to the front")),
                    ),
                "agent-picker-rise",
                &self.presence,
            ))
            .into_any_element()
    }
}
