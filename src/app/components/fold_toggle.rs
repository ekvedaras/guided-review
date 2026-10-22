use gpui::{
    App, ClickEvent, InteractiveElement, IntoElement, ParentElement, RenderOnce,
    StatefulInteractiveElement, Styled, Window, div,
};

use super::palette::Colors;

type OnToggle = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

/// Opens or closes the pieces of a step that folds (skim or read).
#[derive(IntoElement)]
pub struct FoldToggle {
    folded: bool,
    piece_count: usize,
    on_toggle: OnToggle,
}

impl FoldToggle {
    pub fn new(
        folded: bool,
        piece_count: usize,
        on_toggle: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        Self {
            folded,
            piece_count,
            on_toggle: Box::new(on_toggle),
        }
    }
}

impl RenderOnce for FoldToggle {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let colors = Colors::active(cx);
        let label = match (self.folded, self.piece_count) {
            (true, 1) => "Show its 1 piece".to_string(),
            (true, count) => format!("Show its {count} pieces"),
            (false, _) => "Fold the pieces".to_string(),
        };
        div()
            .id("fold-toggle")
            .text_sm()
            .cursor_pointer()
            .text_color(colors.muted_text)
            .on_click(self.on_toggle)
            .child(label)
    }
}
