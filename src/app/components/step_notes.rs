use gpui::{App, FontWeight, IntoElement, ParentElement, RenderOnce, Styled, Window, div};

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
                .flex()
                .flex_col()
                .gap_1()
                .rounded_md()
                .border_l_2()
                .border_color(colors.accent)
                .bg(colors.current)
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(colors.accent)
                        .child("WHAT TO CHECK"),
                )
                .child(InlineMarkdown::new("step-check", &check))
        });
        let tldr = (!self.tldr.is_empty()).then(|| {
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(colors.muted_text)
                        .child("TLDR"),
                )
                .children(self.tldr.into_iter().enumerate().map(|(index, line)| {
                    div().child(InlineMarkdown::new(
                        format!("step-tldr-{index}"),
                        &format!("• {line}"),
                    ))
                }))
        });
        div()
            .max_w(gpui::px(760.))
            .flex()
            .flex_col()
            .gap_3()
            .children(check)
            .children(tldr)
    }
}
