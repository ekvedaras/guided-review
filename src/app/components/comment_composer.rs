use std::borrow::Cow;
use std::rc::Rc;

use gpui::{
    App, FocusHandle, InteractiveElement, IntoElement, ParentElement, RenderOnce, SharedString,
    StatefulInteractiveElement, Styled, Window, div, px,
};

use crate::viewer::{CommentDraft, CommentPlace, CommentProblem, QueuedComment};

use super::button::{ButtonKind, ButtonSize, button as styled_button, icon_label_button, joined};
use super::card::card;
use super::chip::callout;
use super::icon::Icon;
use super::motion::{BOX_RISE, Edge, STATE, busy_dot, slide_in};
use super::palette::{Colors, Tone};
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
}

/// What the box is doing: only one of these at a time.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ComposerState {
    /// Shown, but keys go elsewhere.
    Idle,
    Composing,
    Sending,
}

/// Which ways out the box offers; they are independent facts, not modes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CommentCapabilities {
    pub comment_now: bool,
    pub suggest_change: bool,
    /// The agent link is on, so the comment can also go to the tour agent.
    pub ask_agent: bool,
}

/// The box a comment is written in, under the last selected row: the text and the buttons.
#[derive(IntoElement)]
pub struct CommentComposer {
    draft: CommentDraft,
    focus: FocusHandle,
    state: ComposerState,
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
        state: ComposerState,
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
        let place = &self.draft.place;
        let id = SharedString::from(match place {
            CommentPlace::File(path) => format!("composer-{path}-file"),
            CommentPlace::Lines { end, .. } => format!("composer-{}-{}", end.path, end.line),
        });
        let actions = self.actions;
        let on_focus = Rc::clone(&actions.on_focus);
        let body = if self.state == ComposerState::Composing {
            let (on_type, on_submit, on_stop) = (
                Rc::clone(&actions.on_type),
                Rc::clone(&actions.on_add_to_review),
                Rc::clone(&actions.on_stop),
            );
            let on_ask = Rc::clone(&actions.on_ask_agent);
            let asks = self.capabilities.ask_agent;
            TextBox::new(
                SharedString::from(format!("{id}-text")),
                &self.focus,
                &self.draft.body,
                Lines::Multi,
            )
            .on_edit(move |body, window, cx| on_type(body, window, cx))
            .on_submit(move |window, cx| on_submit(window, cx))
            .on_cancel(move |window, cx| on_stop(window, cx))
            .on_submit_to_agent(asks, move |window, cx| on_ask(window, cx))
            .into_any_element()
        } else {
            div().child(self.draft.body.clone()).into_any_element()
        };
        let buttons = (self.state != ComposerState::Sending).then(|| {
            let capabilities = self.capabilities;
            let ask_agent = capabilities.ask_agent.then(|| {
                let mut parts = vec![button(
                    &colors,
                    "ask-agent",
                    Icon::Sparkles,
                    "Ask agent",
                    ButtonKind::Joined,
                    actions.on_ask_agent,
                )];
                if capabilities.comment_now {
                    let on_ask_now = actions.on_ask_agent_now;
                    parts.push(
                        styled_button(
                            &colors,
                            "ask-agent-now",
                            ButtonKind::Joined,
                            ButtonSize::Regular,
                        )
                        .child("now")
                        .tooltip(tooltip(|_| {
                            "Ask agent now: posts the comment at once".into()
                        }))
                        .on_click(move |_, window, cx| on_ask_now(window, cx)),
                    );
                }
                joined(&colors, parts)
            });
            let on_add = actions.on_add_to_review;
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap_2()
                .child(
                    styled_button(
                        &colors,
                        "add-to-review",
                        ButtonKind::Primary,
                        ButtonSize::Regular,
                    )
                    .child("Add to review")
                    .on_click(move |_, window, cx| on_add(window, cx)),
                )
                .children(capabilities.comment_now.then(|| {
                    button(
                        &colors,
                        "comment-now",
                        Icon::Send,
                        "Comment now",
                        ButtonKind::Secondary,
                        actions.on_comment_now,
                    )
                }))
                .children(capabilities.suggest_change.then(|| {
                    button(
                        &colors,
                        "suggest-change",
                        Icon::Diff,
                        "Suggest change",
                        ButtonKind::Secondary,
                        actions.on_suggest_change,
                    )
                }))
                .children(ask_agent)
                .child(div().flex_1())
                .child(button(
                    &colors,
                    "discard-comment",
                    Icon::Trash,
                    "Discard",
                    ButtonKind::Danger,
                    actions.on_discard,
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
            .font_family(super::palette::TEXT_FONT)
            .on_click(move |_, window, cx| on_focus(window, cx))
            .child(
                div()
                    .text_xs()
                    .text_color(colors.muted_text)
                    .child(place.describe()),
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
                    .child(shown(problem))
            }))
            .children((self.state == ComposerState::Sending).then(|| {
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_sm()
                    .text_color(colors.muted_text)
                    .child(busy_dot(&colors, "composer-posting-dot"))
                    .child("Posting…")
            }))
            .children(buttons);
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

/// A comment GitHub has not taken yet, shown where it will land, with the way out.
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
            .child(self.queued.draft.body.clone())
            .child(div().text_xs().text_color(colors.warning).child(format!(
                "Not sent yet, will be tried again: {}",
                self.queued.failed_because
            )))
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
