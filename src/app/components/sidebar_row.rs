use gpui::prelude::FluentBuilder;
use gpui::{Div, ElementId, InteractiveElement, Pixels, Stateful, Styled, div, px};

use super::palette::Colors;

const FLAT_INDENT: f32 = 12.;
const INDENT_PER_DEPTH: f32 = 16.;

/// The look every clickable row of the sidebar shares.
pub fn sidebar_row(
    colors: Colors,
    id: impl Into<ElementId>,
    depth: usize,
    is_current: bool,
) -> Stateful<Div> {
    div()
        .id(id)
        .pl(indent(depth))
        .pr(px(FLAT_INDENT))
        .py_1()
        .cursor_pointer()
        .when(is_current, |row| row.bg(colors.current))
}

fn indent(depth: usize) -> Pixels {
    px(FLAT_INDENT + INDENT_PER_DEPTH * depth as f32)
}
