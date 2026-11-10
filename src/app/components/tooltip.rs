use gpui::{
    AnyView, App, AppContext, Context, IntoElement, ParentElement, Render, SharedString, Styled,
    Window, div, px,
};

use super::motion::{FEEDBACK, fade_in};
use super::palette::{CONTROL_RADIUS, Colors};

/// A one-line note shown while the pointer rests on an element.
struct Tooltip {
    text: SharedString,
}

impl Render for Tooltip {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = Colors::active(cx);
        fade_in(
            div()
                .px_2()
                .py_1()
                .rounded(px(CONTROL_RADIUS))
                .border_1()
                .border_color(colors.border)
                .bg(colors.overlay)
                .shadow(colors.overlay_shadow())
                .text_xs()
                .text_color(colors.text)
                .child(self.text.clone()),
            "tooltip",
            FEEDBACK,
        )
    }
}

/// For `.tooltip(..)`; the text is made only when the tooltip shows.
pub fn tooltip(
    text: impl Fn(&App) -> SharedString + 'static,
) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
    move |_, cx| {
        let text = text(cx);
        cx.new(|_| Tooltip { text }).into()
    }
}
