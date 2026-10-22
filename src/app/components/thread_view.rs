use gpui::{
    App, ClickEvent, InteractiveElement, IntoElement, ParentElement, RenderOnce, SharedString,
    StatefulInteractiveElement, Styled, Window, div,
};

use crate::viewer::{Markdown, ThreadView};

use super::markdown_view::MarkdownView;
use super::palette::Palette;

type OnToggle = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

/// One review thread as the reviewer sees it under a line, or after the code of a step.
/// Folded threads (resolved, outdated, by a bot) show one line and a toggle.
#[derive(IntoElement)]
pub struct ThreadElement {
    view: ThreadView,
    on_toggle: OnToggle,
}

impl ThreadElement {
    pub fn new(
        view: ThreadView,
        on_toggle: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        Self {
            view,
            on_toggle: Box::new(on_toggle),
        }
    }

    fn kind(&self) -> String {
        let thread = &self.view.thread;
        let mut kinds = Vec::new();
        if thread.is_outdated {
            kinds.push("Outdated");
        }
        if thread.is_resolved {
            kinds.push("Resolved");
        }
        if thread
            .comments
            .first()
            .is_some_and(|comment| comment.is_bot)
        {
            kinds.push("Bot");
        }
        if kinds.is_empty() {
            "Thread".to_string()
        } else {
            kinds.join(", ")
        }
    }

    fn where_it_is(&self) -> String {
        let thread = &self.view.thread;
        let lines = match (thread.is_outdated, thread.start_line, thread.line) {
            (true, _, _) => thread
                .original_line
                .map(|end| match thread.original_start_line {
                    Some(start) if start != end => format!("lines {start}-{end}"),
                    _ => format!("line {end}"),
                }),
            (false, Some(start), Some(end)) if start != end => Some(format!("lines {start}-{end}")),
            (false, _, Some(line)) if !thread.is_file_comment => Some(format!("line {line}")),
            _ => None,
        };
        match (self.view.anchor.is_some(), lines) {
            (true, _) | (false, None) => thread.path.clone(),
            (false, Some(lines)) => format!("{}, {lines}", thread.path),
        }
    }
}

impl RenderOnce for ThreadElement {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let id = self.view.thread.id.clone();
        let comment_count = self.view.thread.comments.len();
        let first_author = self
            .view
            .thread
            .comments
            .first()
            .map(|comment| comment.author.clone())
            .unwrap_or_default();
        let summary = format!(
            "{} · {} · {first_author} · {comment_count} comment{}",
            self.kind(),
            self.where_it_is(),
            if comment_count == 1 { "" } else { "s" }
        );
        let toggle = div()
            .id(SharedString::from(format!("thread-toggle-{id}")))
            .flex_1()
            .cursor_pointer()
            .text_sm()
            .text_color(Palette::muted_text())
            .on_click(self.on_toggle)
            .child(format!(
                "{} {summary}",
                if self.view.is_folded { "▸" } else { "▾" }
            ));
        let conversation_link = self.view.thread.comments.first().map(|comment| {
            let url = comment.url.clone();
            div()
                .id(SharedString::from(format!("thread-link-{id}")))
                .flex_none()
                .cursor_pointer()
                .text_sm()
                .text_color(Palette::muted_text())
                .on_click(move |_, _, cx| cx.open_url(&url))
                .child("Open on GitHub")
        });
        let comments = (!self.view.is_folded).then(|| {
            div().flex().flex_col().gap_3().children(
                self.view
                    .thread
                    .comments
                    .iter()
                    .enumerate()
                    .map(|(index, comment)| {
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(div().text_sm().child(comment.author.clone()))
                            .child(MarkdownView::new(
                                format!("thread-{id}-{index}"),
                                Markdown::parse(&comment.body),
                            ))
                    }),
            )
        });
        div()
            .flex()
            .flex_col()
            .gap_2()
            .m_2()
            .px_3()
            .py_2()
            .rounded_md()
            .border_1()
            .border_color(Palette::border())
            .bg(Palette::background())
            .font_family(super::palette::TEXT_FONT)
            .child(
                div()
                    .flex()
                    .gap_3()
                    .child(toggle)
                    .children(conversation_link),
            )
            .children(comments)
    }
}
