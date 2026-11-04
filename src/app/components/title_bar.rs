use gpui::{
    AnyElement, App, ElementId, InteractiveElement, IntoElement, MouseButton, ParentElement,
    RenderOnce, Rgba, StatefulInteractiveElement, Styled, Window, WindowControlArea, div, px,
};

use super::palette::Colors;

/// Tall enough for the macOS traffic lights, which the window puts at 14 px from the corner.
pub const TITLE_BAR_HEIGHT: f32 = 40.;
/// Room left of the traffic lights' right edge, so nothing is drawn under them.
pub const TRAFFIC_LIGHTS_WIDTH: f32 = 80.;

/// A strip of the window's top edge: it drags the window and a double click zooms it, like a
/// native title bar, while its buttons still take their own clicks.
#[derive(IntoElement)]
pub struct TitleBarArea {
    id: ElementId,
    background: Rgba,
    children: Vec<AnyElement>,
}

impl TitleBarArea {
    pub fn new(id: impl Into<ElementId>, background: Rgba) -> Self {
        Self {
            id: id.into(),
            background,
            children: Vec::new(),
        }
    }
}

impl ParentElement for TitleBarArea {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for TitleBarArea {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let colors = Colors::active(cx);
        let is_pressed = window.use_keyed_state(
            ElementId::from(format!("{}-pressed", self.id)),
            cx,
            |_, _| false,
        );
        let (on_down, on_up, on_move, on_out) = (
            is_pressed.clone(),
            is_pressed.clone(),
            is_pressed.clone(),
            is_pressed,
        );
        div()
            .id(self.id)
            .window_control_area(WindowControlArea::Drag)
            .flex_none()
            .h(px(TITLE_BAR_HEIGHT))
            .flex()
            .items_center()
            .bg(self.background)
            .border_b_1()
            .border_color(colors.subtle_border)
            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                on_down.update(cx, |pressed, _| *pressed = true);
            })
            .on_mouse_up(MouseButton::Left, move |_, _, cx| {
                on_up.update(cx, |pressed, _| *pressed = false);
            })
            .on_mouse_down_out(move |_, _, cx| on_out.update(cx, |pressed, _| *pressed = false))
            .on_mouse_move(move |_, window, cx| {
                if *on_move.read(cx) {
                    on_move.update(cx, |pressed, _| *pressed = false);
                    window.start_window_move();
                }
            })
            .on_click(|event, window, _| {
                if event.click_count() == 2 {
                    window.titlebar_double_click();
                }
            })
            .children(self.children)
    }
}
