use std::borrow::Cow;
use std::rc::Rc;

use gpui::{
    App, FocusHandle, InteractiveElement, IntoElement, ParentElement, RenderOnce, SharedString,
    StatefulInteractiveElement, Styled, Window, div, red,
};

use crate::viewer::{CommentDraft, CommentPlace, CommentProblem, QueuedComment};

use super::palette::Colors;
use super::text_box::TextBox;
use super::text_input::Lines;

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
    label: &'static str,
    on_click: OnCompose,
) -> impl IntoElement {
    div()
        .id(id)
        .px_3()
        .py_1()
        .rounded_md()
        .border_1()
        .border_color(colors.border)
        .cursor_pointer()
        .text_sm()
        .on_click(move |_, window, cx| on_click(window, cx))
        .child(label)
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
            div()
                .flex()
                .gap_2()
                .child(button(
                    &colors,
                    "add-to-review",
                    "Add to review",
                    actions.on_add_to_review,
                ))
                .children(self.capabilities.comment_now.then(|| {
                    button(
                        &colors,
                        "comment-now",
                        "Comment now",
                        actions.on_comment_now,
                    )
                }))
                .children(
                    self.capabilities
                        .ask_agent
                        .then(|| button(&colors, "ask-agent", "Ask agent", actions.on_ask_agent)),
                )
                .children(
                    (self.capabilities.ask_agent && self.capabilities.comment_now).then(|| {
                        button(
                            &colors,
                            "ask-agent-now",
                            "Ask agent now",
                            actions.on_ask_agent_now,
                        )
                    }),
                )
                .children(self.capabilities.suggest_change.then(|| {
                    button(
                        &colors,
                        "suggest-change",
                        "Suggest change",
                        actions.on_suggest_change,
                    )
                }))
                .child(button(
                    &colors,
                    "discard-comment",
                    "Discard",
                    actions.on_discard,
                ))
        });
        div()
            .id(id)
            .flex()
            .flex_col()
            .gap_2()
            .m_2()
            .p_2()
            .rounded_md()
            .border_1()
            .border_color(colors.border)
            .bg(colors.surface)
            .font_family(super::palette::TEXT_FONT)
            .on_click(move |_, window, cx| on_focus(window, cx))
            .child(
                div()
                    .text_xs()
                    .text_color(colors.muted_text)
                    .child(place.describe()),
            )
            .child(div().min_h(gpui::px(48.)).child(body))
            .children(
                self.problem
                    .map(|problem| div().text_sm().text_color(red()).child(shown(problem))),
            )
            .children(
                (self.state == ComposerState::Sending).then(|| div().text_sm().child("Posting…")),
            )
            .children(buttons)
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
        div()
            .flex()
            .flex_col()
            .gap_1()
            .m_2()
            .p_2()
            .rounded_md()
            .border_1()
            .border_color(colors.border)
            .text_sm()
            .child(self.queued.draft.body.clone())
            .child(div().text_xs().text_color(colors.muted_text).child(format!(
                "Not sent yet, will be tried again: {}",
                self.queued.failed_because
            )))
            .child(
                div()
                    .id("discard-queued")
                    .text_xs()
                    .cursor_pointer()
                    .text_color(colors.muted_text)
                    .on_click(move |_, window, cx| on_discard(window, cx))
                    .child("Discard"),
            )
    }
}
