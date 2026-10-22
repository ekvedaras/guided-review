use gpui::{App, IntoElement, ParentElement, RenderOnce, Styled, Window, div, px};

use crate::viewer::{DiffLine, PieceDiff, Side};

use super::palette::{CODE_FONT, Palette};

/// Our own diff view (Zed's is GPL). One row per line, and each row keeps its `LineLocation`
/// so comments and links can hang off any row later.
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
            .children(self.piece.lines.into_iter().map(DiffRow::new))
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
            .child(line_number(base_number))
            .child(line_number(head_number))
            .child(div().w(px(16.)).flex_none().child(sign))
            .child(div().flex_1().whitespace_nowrap().child(self.line.text))
    }
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
