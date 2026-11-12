use gpui::{
    ElementId, InteractiveElement, ParentElement, Rgba, Stateful, StatefulInteractiveElement,
    Styled, div,
};

use crate::app::code_links::open_app_page;
use crate::update::AppVersion;

use super::palette::Colors;

/// The version this app runs, in `at_rest` like the text around it; only under the mouse does it show
/// that it opens its page on GitHub.
pub fn version_link(
    colors: &Colors,
    id: impl Into<ElementId>,
    at_rest: Rgba,
) -> Stateful<gpui::Div> {
    let version = AppVersion::RUNNING;
    let accent = colors.accent;
    div()
        .id(id)
        .cursor_pointer()
        .text_color(at_rest)
        .hover(move |link| link.text_color(accent))
        .on_click(move |_, _, cx| open_app_page(&version.page(), cx))
        .child(version.as_str())
}
