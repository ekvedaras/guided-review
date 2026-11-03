use gpui::{App, IntoElement, ParentElement, RenderOnce, Styled, Window, div};

use crate::tour::Step;

use super::markdown_view::InlineMarkdown;
use super::palette::Colors;

/// The check hint and the TLDR the tour writer put on a step.
#[derive(IntoElement, Clone, Hash)]
pub struct StepNotes {
    check: Option<String>,
    tldr: Vec<String>,
}

impl StepNotes {
    pub fn of(step: &Step) -> Self {
        Self {
            check: step.check.clone(),
            tldr: step.tldr.clone(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.check.is_none() && self.tldr.is_empty()
    }
}

impl RenderOnce for StepNotes {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let colors = Colors::active(cx);
        let check = self.check.map(|check| {
            div()
                .px_3()
                .py_2()
                .rounded_md()
                .border_1()
                .border_color(colors.border)
                .bg(colors.current)
                .child(InlineMarkdown::new(
                    "step-check",
                    &format!("Check: {check}"),
                ))
        });
        let tldr = (!self.tldr.is_empty()).then(|| {
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(div().text_sm().text_color(colors.muted_text).child("TLDR"))
                .children(self.tldr.into_iter().enumerate().map(|(index, line)| {
                    div().child(InlineMarkdown::new(
                        format!("step-tldr-{index}"),
                        &format!("• {line}"),
                    ))
                }))
        });
        div()
            .flex()
            .flex_col()
            .gap_2()
            .children(check)
            .children(tldr)
    }
}
