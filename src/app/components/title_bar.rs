use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, App, Background, ElementId, InteractiveElement, IntoElement, MouseButton,
    ParentElement, Pixels, RenderOnce, StatefulInteractiveElement, Styled, Window,
    WindowControlArea, div, px,
};

use super::palette::Colors;
use crate::platform::Os;

/// Tall enough for the window buttons the platform puts on it.
pub const TITLE_BAR_HEIGHT: f32 = 40.;

/// Where a title bar's own content starts: past the window buttons, or one margin in where the
/// window manager draws none.
pub fn title_inset() -> Pixels {
    Os::RUNNING.window_buttons_inset().max(px(16.))
}

/// How a title bar meets what is under it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TitleBarEdge {
    /// A hairline in `subtle_border`.
    Border,
    /// Nothing: the ground runs on, or the caller draws its own edge (the trail).
    Open,
}

/// A strip of the window's top edge: it drags the window and a double click zooms it, like a
/// native title bar, while its buttons still take their own clicks.
#[derive(IntoElement)]
pub struct TitleBarArea {
    id: ElementId,
    background: Background,
    edge: TitleBarEdge,
    children: Vec<AnyElement>,
}

impl TitleBarArea {
    pub fn new(id: impl Into<ElementId>, background: impl Into<Background>) -> Self {
        Self {
            id: id.into(),
            background: background.into(),
            edge: TitleBarEdge::Border,
            children: Vec::new(),
        }
    }

    pub fn edge(mut self, edge: TitleBarEdge) -> Self {
        self.edge = edge;
        self
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
            .relative()
            .bg(self.background)
            .when(self.edge == TitleBarEdge::Border, |bar| {
                bar.border_b_1().border_color(colors.subtle_border)
            })
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
