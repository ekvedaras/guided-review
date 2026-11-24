//! The Definition card: what a name hovered in a diff line means at its commit. The model is the
//! viewer's `DefinitionCard`, built once; this only draws it.

use std::rc::Rc;
use std::sync::Arc;

use gpui::prelude::FluentBuilder;
use gpui::{
    App, Div, InteractiveElement, IntoElement, ParentElement, SharedString,
    StatefulInteractiveElement, Styled, Window, div, px,
};

use crate::viewer::{DefinitionCard, DefinitionRow, FoundNames, NameTarget, UsageRow};

use super::chip::chip;
use super::overlay::{overlay_panel, picker_row_at};
use super::palette::{CODE_FONT, Colors, Tone};
use super::sidebar_row::RowPlace;
use super::step_notes::note_label;

const NAME_CARD_WIDTH: f32 = 520.;
/// How tall the lists are before they scroll.
const NAME_CARD_LISTS_HEIGHT: f32 = 380.;
const DOC_LINES: usize = 3;

/// What a click on a row of the card does: jumps in the tour or opens the code.
pub type OnNameTarget = Rc<dyn Fn(&NameTarget, &mut Window, &mut App)>;

pub fn name_card(colors: &Colors, card: &DefinitionCard, on_open: &OnNameTarget) -> Div {
    let panel = overlay_panel(colors).w(px(NAME_CARD_WIDTH));
    match card {
        DefinitionCard::Loading { word } => panel.child(line(
            colors.muted_text,
            SharedString::from(format!("Looking up {word}…")),
        )),
        DefinitionCard::Failed { word, because } => panel.child(line(
            colors.danger,
            SharedString::from(format!("Could not look up {word}: {because}")),
        )),
        DefinitionCard::Found(found) => panel.child(found_lists(colors, found, on_open)),
    }
}

fn line(color: gpui::Rgba, text: SharedString) -> Div {
    div().px_4().py_3().text_sm().text_color(color).child(text)
}

fn found_lists(colors: &Colors, found: &FoundNames, on_open: &OnNameTarget) -> Div {
    let definitions_label = found
        .definitions_label()
        .unwrap_or_else(|| "DEFINITION".to_owned())
        .to_uppercase();
    let usages_label = found.usages_label().map(|label| label.to_uppercase());
    let nothing_found = found.definitions.is_empty() && found.usages.is_empty();
    let notes = (!found.notes.is_empty()).then(|| {
        div()
            .px_4()
            .py_2()
            .flex()
            .flex_col()
            .gap_0p5()
            .border_t_1()
            .border_color(colors.subtle_border)
            .bg(colors.header_band())
            .text_xs()
            .text_color(colors.muted_text)
            .children(found.notes.iter().map(|note| div().child(note.text())))
    });
    div()
        .flex()
        .flex_col()
        .child(
            div()
                .id("name-card-lists")
                .max_h(px(NAME_CARD_LISTS_HEIGHT))
                .overflow_y_scroll()
                .py_2()
                .flex()
                .flex_col()
                .child(heading(colors, definitions_label))
                .when(found.definitions.is_empty(), |lists| {
                    lists.child(
                        div()
                            .px_4()
                            .py_1()
                            .text_xs()
                            .text_color(colors.muted_text)
                            .child(if nothing_found {
                                "Not found in the files searched"
                            } else {
                                "No definition found in the files searched"
                            }),
                    )
                })
                .children(
                    found
                        .definitions
                        .iter()
                        .enumerate()
                        .map(|(index, row)| definition_row(colors, index, row, on_open)),
                )
                .children(usages_label.map(|label| heading(colors, label).pt_3()))
                .children(
                    found
                        .usages
                        .iter()
                        .enumerate()
                        .map(|(index, row)| usage_row(colors, index, row, on_open)),
                )
                .children(found.more_usages_label().map(|label| {
                    div()
                        .px_4()
                        .py_1()
                        .text_xs()
                        .text_color(colors.muted_text)
                        .child(label)
                })),
        )
        .children(notes)
}

fn heading(colors: &Colors, label: String) -> Div {
    note_label(label)
        .px_4()
        .pb_1()
        .text_color(colors.muted_text)
}

fn definition_row(
    colors: &Colors,
    index: usize,
    row: &DefinitionRow,
    on_open: &OnNameTarget,
) -> impl IntoElement {
    let (target, on_open) = (row.target.clone(), Rc::clone(on_open));
    picker_row_at(
        colors,
        SharedString::from(format!("name-definition-{index}")),
        RowPlace::Apart,
        None,
    )
    .py_1()
    .flex_col()
    .items_stretch()
    .gap_0p5()
    .on_click(move |_, window, cx| on_open(&target, window, cx))
    .child(
        div()
            .flex()
            .items_center()
            .gap_2()
            .child(
                div()
                    .flex_none()
                    .text_xs()
                    .text_color(colors.muted_text)
                    .child(row.kind.label()),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .font_family(*CODE_FONT)
                    .text_sm()
                    .child(SharedString::from(Arc::clone(&row.signature))),
            )
            .when(row.in_pr, |top| {
                top.child(chip(colors, Tone::Accent).child("in this PR"))
            }),
    )
    .children(row.doc.clone().map(|doc| {
        div()
            .text_xs()
            .text_color(colors.muted_text)
            .line_clamp(DOC_LINES)
            .child(SharedString::from(doc))
    }))
    .child(
        div()
            .truncate()
            .font_family(*CODE_FONT)
            .text_xs()
            .text_color(colors.muted_text)
            .child(SharedString::from(row.place.clone())),
    )
}

fn usage_row(
    colors: &Colors,
    index: usize,
    row: &UsageRow,
    on_open: &OnNameTarget,
) -> impl IntoElement {
    let (target, on_open) = (row.target.clone(), Rc::clone(on_open));
    picker_row_at(
        colors,
        SharedString::from(format!("name-usage-{index}")),
        RowPlace::Apart,
        None,
    )
    .py_1()
    .gap_2()
    .font_family(*CODE_FONT)
    .text_xs()
    .on_click(move |_, window, cx| on_open(&target, window, cx))
    .child(
        div()
            .flex_none()
            .max_w(px(200.))
            .truncate()
            .text_color(colors.muted_text)
            .child(SharedString::from(row.place.clone())),
    )
    .child(
        div()
            .flex_1()
            .min_w_0()
            .truncate()
            .child(SharedString::from(Arc::clone(&row.text))),
    )
}
