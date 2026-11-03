use gpui::{
    AnyView, App, AppContext, Context, IntoElement, ParentElement, Render, SharedString, Styled,
    Window, div,
};

use super::palette::Colors;

/// A one-line note shown while the pointer rests on an element.
struct Tooltip {
    text: SharedString,
}

impl Render for Tooltip {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = Colors::active(cx);
        div()
            .px_2()
            .py_1()
            .rounded_md()
            .border_1()
            .border_color(colors.border)
            .bg(colors.surface)
            .text_xs()
            .text_color(colors.text)
            .child(self.text.clone())
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
