use gpui::{App, FontWeight, IntoElement, ParentElement, RenderOnce, Styled, Window, div, px};

use crate::tour::Step;

use super::chip::callout;
use super::icon::{Icon, sized_icon};
use super::markdown_view::{InlineMarkdown, bullet};
use super::palette::{Colors, Tone};

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

/// The line height of `text_base`, which the TLDR's bullets centre on.
const TLDR_LINE: gpui::Pixels = px(26.);

/// A small label over a note, a section or a screen's heading: one style for every such label.
pub fn note_label(text: impl Into<gpui::SharedString>) -> gpui::Div {
    div()
        .text_xs()
        .font_weight(FontWeight::SEMIBOLD)
        .child(text.into())
}

impl RenderOnce for StepNotes {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let colors = Colors::active(cx);
        let check = self.check.map(|check| {
            callout(&colors, Tone::Accent)
                .flex()
                .flex_col()
                .gap_1p5()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1p5()
                        .text_color(colors.accent)
                        .child(sized_icon(Icon::ListChecks, colors.accent, px(14.)))
                        .child(note_label("WHAT TO CHECK")),
                )
                .child(InlineMarkdown::new("step-check", &check))
        });
        let tldr = (!self.tldr.is_empty()).then(|| {
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(note_label("TLDR").pb_0p5().text_color(colors.muted_text))
                .children(self.tldr.into_iter().enumerate().map(|(index, line)| {
                    div()
                        .flex()
                        .items_start()
                        .gap_3()
                        .child(bullet(&colors, TLDR_LINE))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .child(InlineMarkdown::new(format!("step-tldr-{index}"), &line)),
                        )
                }))
        });
        div()
            .max_w(px(760.))
            .flex()
            .flex_col()
            .gap_5()
            .children(check)
            .children(tldr)
    }
}
