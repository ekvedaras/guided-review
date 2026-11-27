use std::borrow::Cow;
use std::rc::Rc;

use gpui::prelude::FluentBuilder;
use gpui::{
    App, FocusHandle, InteractiveElement, IntoElement, ParentElement, RenderOnce, SharedString,
    StatefulInteractiveElement, Styled, Window, div, px,
};

use crate::viewer::{
    BoxState, CommentDraft, CommentPlace, CommentProblem, QueuedComment, SendAction, SendMode,
    SendRole, Sending,
};

use super::button::{
    ButtonKind, ButtonSize, button as styled_button, chord_keys, icon_label_button,
};
use super::card::card;
use super::chip::callout;
use super::finding_view::finding_chips;
use super::icon::Icon;
use super::motion::{BOX_RISE, Edge, STATE, busy_dot, slide_in};
use super::palette::{Colors, Tone};
use super::selectable_text::SelectableText;
use super::text_box::{Field, TextBox};
use super::text_input::Lines;
use super::tooltip::tooltip;

pub type OnCompose = Rc<dyn Fn(&mut Window, &mut App)>;
pub type OnType = Rc<dyn Fn(String, &mut Window, &mut App)>;

/// What the composer's text and buttons do; the layout owns the review they change.
pub struct ComposerActions {
    pub on_focus: OnCompose,
    pub on_type: OnType,
    /// Escape: the draft stays, the box stops taking keys.
    pub on_stop: OnCompose,
    pub on_add_to_review: OnCompose,
    pub on_comment_now: OnCompose,
    pub on_ask_agent: OnCompose,
    pub on_ask_agent_now: OnCompose,
    pub on_suggest_change: OnCompose,
    pub on_discard: OnCompose,
    /// One of a local review's task send actions.
    pub on_send_task: OnSendTask,
}

pub type OnSendTask = Rc<dyn Fn(SendMode, &mut Window, &mut App)>;

/// Which ways out the box offers; they are independent facts, not modes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CommentCapabilities {
    pub comment_now: bool,
    pub suggest_change: bool,
    /// The agent link is on, so the comment can also go to the tour agent.
    pub ask_agent: bool,
    /// A local review's comment is a task, which replaces every send action of the host.
    pub sending: Sending,
}

/// What a task send action's button says.
pub fn task_button(mode: SendMode) -> (Icon, &'static str) {
    match mode {
        SendMode::SendNow => (Icon::Send, "Send now"),
        SendMode::AddToTasks => (Icon::ListChecks, "Add to tasks"),
        SendMode::AddToReview | SendMode::CommentNow => (Icon::Send, "Send"),
    }
}

/// The send actions a box has: the ones of the host, or a local review's tasks, and Suggest change
/// where the lines can take one.
fn offered_actions(capabilities: CommentCapabilities) -> Vec<SendAction> {
    let mut offered = vec![SendAction::AddToReview];
    let sending = capabilities.sending;
    if sending.to_host() {
        if capabilities.comment_now {
            offered.push(SendAction::CommentNow);
        }
        if capabilities.ask_agent {
            offered.push(SendAction::AskAgent);
            if capabilities.comment_now {
                offered.push(SendAction::AskAgentNow);
            }
        }
    } else if sending.tasks().len() > 1 {
        offered.push(SendAction::CommentNow);
    }
    if capabilities.suggest_change {
        offered.push(SendAction::SuggestChange);
    }
    offered
}

/// A send button's id, icon and words. A local review's two task buttons stand for Add to review
/// and Comment now.
fn send_button_look(
    action: SendAction,
    sending: Sending,
) -> (&'static str, Option<Icon>, &'static str) {
    let task = |index: usize| sending.tasks().get(index).copied().map(task_button);
    match (action, sending.to_host()) {
        (SendAction::AddToReview, false) => task(0)
            .map_or(("send-task-0", None, "Add"), |(icon, label)| {
                ("send-task-0", Some(icon), label)
            }),
        (SendAction::CommentNow, false) => task(1)
            .map_or(("send-task-1", None, "Send"), |(icon, label)| {
                ("send-task-1", Some(icon), label)
            }),
        (SendAction::AddToReview, true) => ("add-to-review", None, "Add to review"),
        (SendAction::CommentNow, true) => ("comment-now", Some(Icon::Send), "Comment now"),
        (SendAction::SuggestChange, _) => ("suggest-change", Some(Icon::Diff), "Suggest change"),
        (SendAction::AskAgent, _) => ("ask-agent", Some(Icon::Sparkles), "Ask agent"),
        (SendAction::AskAgentNow, _) => ("ask-agent-now", Some(Icon::Zap), "Ask agent now"),
    }
}

/// The box a comment is written in, under the last selected row: the text and the buttons.
#[derive(IntoElement)]
pub struct CommentComposer {
    draft: CommentDraft,
    focus: FocusHandle,
    state: BoxState,
    capabilities: CommentCapabilities,
    /// What stops the comment, or what went wrong with the last send.
    problem: Option<CommentProblem>,
    actions: ComposerActions,
}

impl CommentComposer {
    pub const KEY_CONTEXT: &str = "CommentInput";

    pub fn new(
        draft: CommentDraft,
        focus: FocusHandle,
        state: BoxState,
        capabilities: CommentCapabilities,
        problem: Option<CommentProblem>,
        actions: ComposerActions,
    ) -> Self {
        Self {
            draft,
            focus,
            state,
            capabilities,
            problem,
            actions,
        }
    }
}

fn button(
    colors: &Colors,
    id: &'static str,
    icon: Icon,
    label: &'static str,
    kind: ButtonKind,
    on_click: OnCompose,
) -> gpui::Stateful<gpui::Div> {
    icon_label_button(colors, id, kind, ButtonSize::Regular, icon, label)
        .on_click(move |_, window, cx| on_click(window, cx))
}

impl RenderOnce for CommentComposer {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let colors = Colors::active(cx);
        let keymap = crate::app::keymap::shared_keymap(cx);
        let place = &self.draft.place;
        let id = SharedString::from(match place {
            CommentPlace::File(path) => format!("composer-{path}-file"),
            CommentPlace::Lines { end, .. } => format!("composer-{}-{}", end.path, end.line),
        });
        let actions = self.actions;
        let on_focus = Rc::clone(&actions.on_focus);
        let sending = self.capabilities.sending;
        let offered = offered_actions(self.capabilities);
        let run = |action: SendAction| -> OnCompose {
            let on_send_task = Rc::clone(&actions.on_send_task);
            match action {
                SendAction::AddToReview => match sending.tasks().first() {
                    Some(&mode) => Rc::new(move |window, cx| on_send_task(mode, window, cx)),
                    None => Rc::clone(&actions.on_add_to_review),
                },
                SendAction::CommentNow => match sending.tasks().get(1) {
                    Some(&mode) => Rc::new(move |window, cx| on_send_task(mode, window, cx)),
                    None => Rc::clone(&actions.on_comment_now),
                },
                SendAction::SuggestChange => Rc::clone(&actions.on_suggest_change),
                SendAction::AskAgent => Rc::clone(&actions.on_ask_agent),
                SendAction::AskAgentNow => Rc::clone(&actions.on_ask_agent_now),
            }
        };
        let body = if self.state == BoxState::Composing {
            let (on_type, on_stop) = (Rc::clone(&actions.on_type), Rc::clone(&actions.on_stop));
            let typed = |pressed: SendAction| {
                let runs = keymap.send_key_runs(pressed, &offered);
                let on_key = run(runs.unwrap_or(pressed));
                (runs.is_some(), move |window: &mut Window, cx: &mut App| {
                    on_key(window, cx);
                })
            };
            let (submits, on_submit) = typed(SendAction::AddToReview);
            let (asks, on_ask) = typed(SendAction::AskAgent);
            let (posts, on_post) = typed(SendAction::CommentNow);
            let (asks_now, on_ask_now) = typed(SendAction::AskAgentNow);
            let (suggests, on_suggest) = typed(SendAction::SuggestChange);
            TextBox::new(
                SharedString::from(format!("{id}-text")),
                &self.focus,
                &self.draft.body,
                Lines::Multi,
            )
            .label("Comment")
            .on_edit(move |body, window, cx| on_type(body, window, cx))
            .on_submit(move |window, cx| {
                if submits {
                    on_submit(window, cx);
                }
            })
            .on_cancel(move |window, cx| on_stop(window, cx))
            .on_submit_to_agent(asks, on_ask)
            .on_submit_now(posts, on_post)
            .on_submit_to_agent_now(asks_now, on_ask_now)
            .on_suggest_change(suggests, on_suggest)
            .into_any_element()
        } else {
            div().child(self.draft.body.clone()).into_any_element()
        };
        let send_buttons = (self.state != BoxState::Sending).then(|| {
            let buttons = keymap.send_plan(&offered).into_iter().map(|planned| {
                let kind = match planned.role {
                    SendRole::Primary => ButtonKind::Primary,
                    SendRole::Secondary | SendRole::Other => ButtonKind::Secondary,
                };
                let on_click = run(planned.action);
                let (id, icon, label) = send_button_look(planned.action, sending);
                let base = match icon {
                    Some(icon) => {
                        icon_label_button(&colors, id, kind, ButtonSize::Regular, icon, label)
                    }
                    None => styled_button(&colors, id, kind, ButtonSize::Regular).child(label),
                };
                base.child(chord_keys(&colors, kind, planned.chord))
                    .when(planned.action == SendAction::AskAgentNow, |button| {
                        button.tooltip(tooltip(|_| {
                            "Ask agent now: posts the comment at once".into()
                        }))
                    })
                    .aria_label(label)
                    .on_click(move |_, window, cx| on_click(window, cx))
            });
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap_2()
                .children(buttons)
                .child(div().flex_1())
                .child(button(
                    &colors,
                    "discard-comment",
                    Icon::Trash,
                    "Discard",
                    ButtonKind::Danger,
                    Rc::clone(&actions.on_discard),
                ))
        });
        let rise_id = SharedString::from(format!("{id}-rise"));
        let composer = card(&colors)
            .id(id)
            .flex()
            .flex_col()
            .gap_2p5()
            .m_2()
            .p_3()
            .font_family(*super::palette::TEXT_FONT)
            .on_click(move |_, window, cx| on_focus(window, cx))
            .child(
                div().flex().items_center().justify_between().gap_2().child(
                    div()
                        .text_xs()
                        .text_color(colors.muted_text)
                        .child(place.describe()),
                ),
            )
            .child(
                Field::new(&self.focus)
                    .px_2()
                    .py_1p5()
                    .min_h(px(64.))
                    .child(body),
            )
            .children(self.problem.map(|problem| {
                div()
                    .text_sm()
                    .text_color(colors.danger)
                    .child(SelectableText::new(
                        format!("composer-problem-{problem}"),
                        shown(problem).to_string(),
                    ))
            }))
            .children((self.state == BoxState::Sending).then(|| {
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_sm()
                    .text_color(colors.muted_text)
                    .child(busy_dot(&colors, "composer-posting-dot"))
                    .child(if sending.to_host() {
                        "Posting…"
                    } else {
                        "Sending…"
                    })
            }))
            .children(send_buttons);
        slide_in(composer, rise_id, STATE, Edge::Bottom, BOX_RISE)
    }
}

fn shown(problem: CommentProblem) -> SharedString {
    match problem {
        CommentProblem::Refused(refusal) => match refusal.message() {
            Cow::Borrowed(text) => SharedString::new_static(text),
            Cow::Owned(text) => text.into(),
        },
        CommentProblem::Rejected(reason) | CommentProblem::Failed(reason) => reason.into(),
    }
}

/// A comment the host has not taken yet, shown where it will land, with the way out.
#[derive(IntoElement)]
pub struct QueuedCommentNote {
    queued: QueuedComment,
    on_discard: OnCompose,
}

impl QueuedCommentNote {
    pub fn new(queued: QueuedComment, on_discard: OnCompose) -> Self {
        Self { queued, on_discard }
    }
}

impl RenderOnce for QueuedCommentNote {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let colors = Colors::active(cx);
        let on_discard = self.on_discard;
        callout(&colors, Tone::Warning)
            .flex()
            .flex_col()
            .gap_1()
            .m_2()
            .text_sm()
            .children((!self.queued.draft.from_findings.is_empty()).then(|| {
                div().flex().items_center().gap_2().children(
                    self.queued
                        .draft
                        .from_findings
                        .iter()
                        .flat_map(|finding| finding_chips(&colors, finding.severity)),
                )
            }))
            .child(self.queued.draft.body.clone())
            .child(
                div()
                    .text_xs()
                    .text_color(colors.warning)
                    .child(SelectableText::new(
                        format!("queued-failure-{}", self.queued.failed_because),
                        format!(
                            "Not sent yet, will be tried again: {}",
                            self.queued.failed_because
                        ),
                    )),
            )
            .child(
                div().flex().ml(px(-8.)).child(
                    icon_label_button(
                        &colors,
                        "discard-queued",
                        ButtonKind::Danger,
                        ButtonSize::Small,
                        Icon::Trash,
                        "Discard",
                    )
                    .on_click(move |_, window, cx| on_discard(window, cx)),
                ),
            )
    }
}
