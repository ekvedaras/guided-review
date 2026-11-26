//! Picking the tour agent by hand, for when the one the app found is not the right one: every
//! Claude terminal with where it works, a button that shows it and one that makes it the agent.

use std::time::Instant;

use gpui::{
    Context, EventEmitter, FocusHandle, FontWeight, InteractiveElement, IntoElement, KeyDownEvent,
    MouseButton, ParentElement, Render, SharedString, StatefulInteractiveElement, Styled, Window,
    div, px,
};

use crate::viewer::{AgentChoice, Presence, TerminalId};

use super::button::{ButtonKind, ButtonSize, icon_button, icon_label_button};
use super::chip::chip;
use super::icon::{Icon, sized_icon};
use super::key_cap::{KeyLabel, key_hint};
use super::motion::OVERLAY;
use super::overlay::{close_presence, modal_layer_for, overlay_panel, picker_footer, rise_for};
use super::palette::{CODE_FONT, Colors, Tone};
use super::selectable_text::SelectableText;

pub enum AgentPickerEvent {
    Use(TerminalId),
    Show(TerminalId),
}

/// The terminals as herdr listed them when the picker opened; `None` while it looks.
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
    /// Says what the agent is for: "this PR's tour agent", "this folder's agent".
    of_what: SharedString,
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
            of_what: SharedString::default(),
        }
    }

    pub fn is_open(&self) -> bool {
        self.presence.is_open()
    }

    /// Opens on "looking"; `listed` gives the terminals once herdr answers.
    pub fn open(&mut self, of_what: SharedString, window: &mut Window, cx: &mut Context<Self>) {
        self.of_what = of_what;
        self.listed = Listed::Looking;
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
        use crate::viewer::AgentStatus;
        let choice =
            |terminal: &str, folder: &str, branch: &str, status, is_the_agent, works| AgentChoice {
                terminal: TerminalId::new(terminal),
                space: "work".to_owned(),
                folder: folder.into(),
                repo: "ekvedaras/guided-review".parse().ok(),
                branch: BranchName::new(branch).ok(),
                status,
                is_the_agent,
                works_on_the_code: works,
                started_by_app: false,
            };
        self.open("this PR's tour agent".into(), window, cx);
        self.listed(
            Ok(vec![
                choice(
                    "t3",
                    "/Users/you/Code/guided-review",
                    "main",
                    AgentStatus::Working,
                    true,
                    false,
                ),
                choice(
                    "t7",
                    "/Users/you/Code/claude-a",
                    "files-list",
                    AgentStatus::Idle,
                    false,
                    true,
                ),
                choice(
                    "t9",
                    "/Users/you/Code/claude-b",
                    "settings-pages",
                    AgentStatus::WaitingForYou,
                    false,
                    false,
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

    fn choice_row(
        &self,
        colors: &Colors,
        choice: &AgentChoice,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let terminal = choice.terminal.clone();
        let where_it_works = match (&choice.repo, &choice.branch) {
            (Some(repo), Some(branch)) => format!("{} · {}", repo.repo, branch.as_str()),
            (None, Some(branch)) => format!("no origin · {}", branch.as_str()),
            _ => "not a git checkout".to_owned(),
        };
        let tags = [
            choice
                .is_the_agent
                .then_some(("the agent now", Tone::Accent)),
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
        let pick = (!choice.is_the_agent).then(|| {
            let terminal = terminal.clone();
            icon_label_button(
                colors,
                SharedString::from(format!("agent-picker-use-{terminal}")),
                ButtonKind::Secondary,
                ButtonSize::Small,
                Icon::Check,
                format!("Use {terminal}"),
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                cx.emit(AgentPickerEvent::Use(terminal.clone()));
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
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(format!("{} in {}", terminal, choice.space)),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(colors.muted_text)
                                    .child(choice.status.to_string()),
                            )
                            .children(tags),
                    )
                    .child(
                        div()
                            .text_sm()
                            .font_family(*CODE_FONT)
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

impl Render for AgentPicker {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.presence.shown().is_none() {
            return div().into_any_element();
        }
        let colors = Colors::active(cx);
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
                            .child("Pick the agent"),
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
        let body: Vec<gpui::AnyElement> = match &self.listed {
            Listed::Looking => vec![quiet("Looking at herdr's terminals...".to_owned())],
            Listed::Failed(reason) => {
                vec![quiet(format!("Could not list the terminals: {reason}"))]
            }
            Listed::Found(choices) if choices.is_empty() => {
                vec![quiet("No terminal in herdr runs Claude.".to_owned())]
            }
            Listed::Found(choices) => {
                let choices = choices.clone();
                choices
                    .iter()
                    .map(|choice| self.choice_row(&colors, choice, cx).into_any_element())
                    .collect()
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
                    .track_focus(&self.focus)
                    .on_key_down(cx.listener(Self::on_key_down))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .mt(px(80.))
                    .w(px(760.))
                    .max_h(px(560.))
                    .flex()
                    .flex_col()
                    .child(header)
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
