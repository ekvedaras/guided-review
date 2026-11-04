use std::rc::Rc;

use gpui::{
    App, FocusHandle, InteractiveElement, IntoElement, ParentElement, RenderOnce, SharedString,
    StatefulInteractiveElement, Styled, Window, div, red,
};

use crate::viewer::{CommentDraft, CommentPlace, QueuedComment};

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
    pub on_suggest_change: OnCompose,
    pub on_discard: OnCompose,
}

/// The box a comment is written in, under the last selected row: the text and the buttons.
#[derive(IntoElement)]
pub struct CommentComposer {
    draft: CommentDraft,
    focus: FocusHandle,
    is_composing: bool,
    is_sending: bool,
    can_comment_now: bool,
    can_suggest_change: bool,
    /// What stops the comment, or what went wrong with the last send.
    problem: Option<String>,
    actions: ComposerActions,
}

impl CommentComposer {
    pub const KEY_CONTEXT: &str = "CommentInput";

    #[expect(
        clippy::too_many_arguments,
        reason = "the view is built from its collaborators, which the caller owns"
    )]
    pub fn new(
        draft: CommentDraft,
        focus: FocusHandle,
        is_composing: bool,
        is_sending: bool,
        can_comment_now: bool,
        can_suggest_change: bool,
        problem: Option<String>,
        actions: ComposerActions,
    ) -> Self {
        Self {
            draft,
            focus,
            is_composing,
            is_sending,
            can_comment_now,
            can_suggest_change,
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
        let id = SharedString::from(format!("composer-{}-{}", place.path(), place.end.line));
        let actions = self.actions;
        let on_focus = Rc::clone(&actions.on_focus);
        let body = if self.is_composing && !self.is_sending {
            let (on_type, on_submit, on_stop) = (
                Rc::clone(&actions.on_type),
                Rc::clone(&actions.on_add_to_review),
                Rc::clone(&actions.on_stop),
            );
            TextBox::new(
                SharedString::from(format!("{id}-text")),
                &self.focus,
                &self.draft.body,
                Lines::Multi,
            )
            .on_edit(move |body, window, cx| on_type(body, window, cx))
            .on_submit(move |window, cx| on_submit(window, cx))
            .on_cancel(move |window, cx| on_stop(window, cx))
            .into_any_element()
        } else {
            div().child(self.draft.body.clone()).into_any_element()
        };
        let buttons = (!self.is_sending).then(|| {
            div()
                .flex()
                .gap_2()
                .child(button(
                    &colors,
                    "add-to-review",
                    "Add to review",
                    actions.on_add_to_review,
                ))
                .children(self.can_comment_now.then(|| {
                    button(
                        &colors,
                        "comment-now",
                        "Comment now",
                        actions.on_comment_now,
                    )
                }))
                .children(self.can_suggest_change.then(|| {
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
                    .child(describe(place)),
            )
            .child(div().min_h(gpui::px(48.)).child(body))
            .children(
                self.problem
                    .map(|problem| div().text_sm().text_color(red()).child(problem)),
            )
            .children(self.is_sending.then(|| div().text_sm().child("Sending...")))
            .children(buttons)
    }
}

fn describe(place: &CommentPlace) -> String {
    if place.is_file_comment() {
        format!("Comment on {}", place.path())
    } else if place.is_single_line() {
        format!("Comment on {} line {}", place.path(), place.end.line)
    } else {
        format!(
            "Comment on {} lines {} to {}",
            place.path(),
            place.start.line,
            place.end.line
        )
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
