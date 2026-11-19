//! A finding of the AI review on the page: a card marked as AI, with its severity, under the line it is about,
//! and marked outdated with what its lines said once new commits changed them.

use gpui::{Div, IntoElement, ParentElement, Styled, div};

use super::card::card;
use super::chip::chip;
use super::markdown_view::{MarkdownView, parsed_markdown};
use super::palette::{Colors, Tone};
use super::thread_view::before_block;
use crate::findings::Severity;
use crate::viewer::{FindingRow, WhyAuthor};

pub fn tone_of(severity: Severity) -> Tone {
    match severity {
        Severity::MustFix => Tone::Danger,
        Severity::ShouldFix => Tone::Warning,
        Severity::Nit => Tone::Neutral,
    }
}

/// `actions` are the buttons that decide the finding, which the layout makes because their clicks are its.
pub fn finding_card(colors: &Colors, row: &FindingRow, actions: impl IntoElement) -> Div {
    card(colors)
        .m_2()
        .px_3()
        .py_2()
        .flex()
        .flex_col()
        .gap_1p5()
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(chip(colors, Tone::Accent).child("AI"))
                .child(chip(colors, tone_of(row.severity)).child(row.severity.to_string()))
                .children(
                    row.outdated
                        .as_ref()
                        .map(|_| chip(colors, Tone::Warning).child("Outdated")),
                ),
        )
        .child(
            MarkdownView::new(format!("finding-{}", row.id), parsed_markdown(&row.body))
                .into_element(),
        )
        .children(
            row.outdated
                .as_ref()
                .map(|before| before_block(colors, "When found, the lines were:", before)),
        )
        .children(row.fix.as_ref().map(|line| {
            div()
                .text_sm()
                .text_color(colors.muted_text)
                .child(line.to_string())
        }))
        .children(row.thread.iter().enumerate().map(|(at, said)| {
            let who = match said.author {
                WhyAuthor::Reviewer => "You",
                WhyAuthor::ReviewAgent => "Review agent",
            };
            div()
                .pl_3()
                .border_l_2()
                .border_color(colors.border)
                .flex()
                .flex_col()
                .gap_0p5()
                .child(div().text_xs().text_color(colors.muted_text).child(who))
                .child(
                    MarkdownView::new(
                        format!("finding-{}-why-{at}", row.id),
                        parsed_markdown(&said.body),
                    )
                    .into_element(),
                )
        }))
        .children(row.asked.as_ref().map(|line| {
            div()
                .text_sm()
                .text_color(colors.muted_text)
                .child(line.to_string())
        }))
        .child(actions)
}
