use std::rc::Rc;

use gpui::{
    App, InteractiveElement, IntoElement, ParentElement, RenderOnce, SharedString,
    StatefulInteractiveElement, Styled, Window, div, red,
};

use crate::viewer::{CommentDraft, CommentPlace, QueuedComment};

use super::palette::Palette;

pub type OnCompose = Rc<dyn Fn(&mut Window, &mut App)>;

/// The box a comment is written in, under the last selected row. It shows the text and buttons;
/// the layout feeds it the keys, so one place decides what typing does.
#[derive(IntoElement)]
pub struct CommentComposer {
    draft: CommentDraft,
    is_focused: bool,
    is_sending: bool,
    can_comment_now: bool,
    /// What stops the comment, or what went wrong with the last send.
    problem: Option<String>,
    on_focus: OnCompose,
    on_add_to_review: OnCompose,
    on_comment_now: OnCompose,
    on_discard: OnCompose,
}

impl CommentComposer {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        draft: CommentDraft,
        is_focused: bool,
        is_sending: bool,
        can_comment_now: bool,
        problem: Option<String>,
        on_focus: OnCompose,
        on_add_to_review: OnCompose,
        on_comment_now: OnCompose,
        on_discard: OnCompose,
    ) -> Self {
        Self {
            draft,
            is_focused,
            is_sending,
            can_comment_now,
            problem,
            on_focus,
            on_add_to_review,
            on_comment_now,
            on_discard,
        }
    }
}

fn button(id: &'static str, label: &'static str, on_click: OnCompose) -> impl IntoElement {
    div()
        .id(id)
        .px_3()
        .py_1()
        .rounded_md()
        .border_1()
        .border_color(Palette::border())
        .cursor_pointer()
        .text_sm()
        .on_click(move |_, window, cx| on_click(window, cx))
        .child(label)
}

impl RenderOnce for CommentComposer {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let place = &self.draft.place;
        let id = SharedString::from(format!("composer-{}-{}", place.path(), place.end.line));
        let on_focus = self.on_focus.clone();
        let cursor = if self.is_focused { "\u{258f}" } else { "" };
        let buttons = (!self.is_sending).then(|| {
            div()
                .flex()
                .gap_2()
                .child(button(
                    "add-to-review",
                    "Add to review",
                    self.on_add_to_review,
                ))
                .children(
                    self.can_comment_now
                        .then(|| button("comment-now", "Comment now", self.on_comment_now)),
                )
                .child(button("discard-comment", "Discard", self.on_discard))
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
            .border_color(Palette::border())
            .bg(Palette::sidebar())
            .font_family(super::palette::TEXT_FONT)
            .on_click(move |_, window, cx| on_focus(window, cx))
            .child(
                div()
                    .text_xs()
                    .text_color(Palette::muted_text())
                    .child(describe(place)),
            )
            .child(
                div()
                    .min_h(gpui::px(48.))
                    .child(format!("{}{cursor}", self.draft.body)),
            )
            .children(
                self.problem
                    .map(|problem| div().text_sm().text_color(red()).child(problem)),
            )
            .children(self.is_sending.then(|| div().text_sm().child("Sending...")))
            .children(buttons)
    }
}

fn describe(place: &CommentPlace) -> String {
    if place.is_single_line() {
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
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let on_discard = self.on_discard;
        div()
            .flex()
            .flex_col()
            .gap_1()
            .m_2()
            .p_2()
            .rounded_md()
            .border_1()
            .border_color(Palette::border())
            .text_sm()
            .child(self.queued.draft.body.clone())
            .child(
                div()
                    .text_xs()
                    .text_color(Palette::muted_text())
                    .child(format!(
                        "Not sent yet, will be tried again: {}",
                        self.queued.failed_because
                    )),
            )
            .child(
                div()
                    .id("discard-queued")
                    .text_xs()
                    .cursor_pointer()
                    .text_color(Palette::muted_text())
                    .on_click(move |_, window, cx| on_discard(window, cx))
                    .child("Discard"),
            )
    }
}
