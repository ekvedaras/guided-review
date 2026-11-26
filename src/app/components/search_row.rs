//! The search row of a list over the page: the search icon and the query box on a row of their own
//! under the list's head, as the jump palette has it, so every list searches the same way.

use gpui::prelude::FluentBuilder;
use gpui::{Div, ParentElement, Styled, div, px};

use super::icon::{Icon, sized_icon};
use super::key_cap::{KeyLabel, key_cap};
use super::palette::Colors;
use super::text_box::TextBox;

/// `placeholder` shows while `shows_placeholder`; `key` is the cap of the key that gives the box
/// the keys, for a list whose own keys work while the box does not have them.
pub fn search_row(
    colors: &Colors,
    query_box: TextBox,
    placeholder: &'static str,
    shows_placeholder: bool,
    key: Option<KeyLabel>,
) -> Div {
    div()
        .px_4()
        .py_3()
        .flex()
        .items_center()
        .gap_3()
        .text_base()
        .border_b_1()
        .border_color(colors.subtle_border)
        .child(sized_icon(Icon::Search, colors.muted_text, px(16.)))
        .child(div().relative().flex_1().min_w_0().child(query_box).when(
            shows_placeholder,
            |field| {
                field.child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .text_color(colors.muted_text)
                        .child(placeholder),
                )
            },
        ))
        .children(key.map(|key| key_cap(colors, key)))
}
