use gpui::{
    App, HighlightStyle, InteractiveElement, IntoElement, ParentElement, RenderOnce, SharedString,
    StatefulInteractiveElement, Styled, StyledText, Window, div, px,
};

use crate::viewer::{DiffLine, GitHubLink, PieceDiff, Side};

use super::palette::{CODE_FONT, Palette};

/// Our own diff view (Zed's is GPL). One row per line, and each row keeps its `LineLocation`
/// so comments can hang off any row later. A click on a line number opens that line on GitHub.
#[derive(IntoElement)]
pub struct DiffView {
    piece: PieceDiff,
}

impl DiffView {
    pub fn new(piece: PieceDiff) -> Self {
        Self { piece }
    }
}

impl RenderOnce for DiffView {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .border_1()
            .border_color(Palette::border())
            .rounded_md()
            .overflow_hidden()
            .child(
                div()
                    .px_2()
                    .py_1()
                    .bg(Palette::sidebar())
                    .border_b_1()
                    .border_color(Palette::border())
                    .text_sm()
                    .font_family(CODE_FONT)
                    .child(self.piece.id.to_string()),
            )
            .children(self.piece.binary_file.map(BinaryFileRow::new))
            .children(self.piece.lines.into_iter().map(DiffRow::new))
    }
}

/// A binary file has no lines to show, so it gets one row that sends the reviewer to GitHub.
#[derive(IntoElement)]
struct BinaryFileRow {
    link: GitHubLink,
}

impl BinaryFileRow {
    fn new(link: GitHubLink) -> Self {
        Self { link }
    }
}

impl RenderOnce for BinaryFileRow {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let link = self.link;
        div()
            .id(SharedString::from(link.to_string()))
            .px_2()
            .py_1()
            .text_sm()
            .cursor_pointer()
            .text_color(Palette::muted_text())
            .on_click(move |_, _, cx| cx.open_url(link.as_str()))
            .child("Binary file, open it on GitHub")
    }
}

#[derive(IntoElement)]
struct DiffRow {
    line: DiffLine,
}

impl DiffRow {
    fn new(line: DiffLine) -> Self {
        Self { line }
    }
}

impl RenderOnce for DiffRow {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let location = &self.line.location;
        let link = self.line.github_link.clone();
        let (base_number, head_number, sign, background) = match location.side {
            Side::Base => (
                location.line.to_string(),
                String::new(),
                "-",
                Palette::removed_line(),
            ),
            Side::Head => (
                String::new(),
                location.line.to_string(),
                "+",
                Palette::added_line(),
            ),
        };
        div()
            .flex()
            .bg(background)
            .font_family(CODE_FONT)
            .text_sm()
            .child(
                div()
                    .id(SharedString::from(link.to_string()))
                    .flex()
                    .flex_none()
                    .cursor_pointer()
                    .on_click(move |_, _, cx| cx.open_url(link.as_str()))
                    .child(line_number(base_number))
                    .child(line_number(head_number)),
            )
            .child(div().w(px(16.)).flex_none().child(sign))
            .child(
                div()
                    .flex_1()
                    .whitespace_nowrap()
                    .child(highlighted(self.line)),
            )
    }
}

fn highlighted(line: DiffLine) -> StyledText {
    let highlights: Vec<_> = line
        .spans
        .iter()
        .filter(|span| {
            line.text.is_char_boundary(span.range.start)
                && line.text.is_char_boundary(span.range.end)
        })
        .map(|span| {
            let style = HighlightStyle {
                color: Some(Palette::code(span.capture).into()),
                ..Default::default()
            };
            (span.range.clone(), style)
        })
        .collect();
    StyledText::new(line.text).with_highlights(highlights)
}

fn line_number(number: String) -> impl IntoElement {
    div()
        .w(px(48.))
        .flex_none()
        .pr_2()
        .text_right()
        .text_color(Palette::muted_text())
        .child(number)
}
