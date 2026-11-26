//! The Definition card: what a name hovered in a diff line means at its commit. The model is the
//! viewer's `DefinitionCard`, built once; this only draws it.

use std::rc::Rc;
use std::sync::Arc;

use gpui::prelude::FluentBuilder;
use gpui::{
    App, Div, FontWeight, HighlightStyle, InteractiveElement, IntoElement, ParentElement, Pixels,
    SharedString, StatefulInteractiveElement, Styled, StyledText, Window, div, px,
};

use crate::navigation::ServerAnswer;
use crate::viewer::{
    CodeRun, DefinitionCard, DefinitionRow, FoundNames, NameTarget, ServerQuestion, UsageFile,
    UsageLine,
};

use super::button::{ButtonKind, ButtonSize, button};
use super::chip::chip;
use super::overlay::{overlay_panel, picker_row_at};
use super::palette::{CODE_FONT, Colors, Tone};
use super::sidebar_row::RowPlace;

const NAME_CARD_WIDTH: f32 = 640.;
/// How tall the lists are before they scroll.
const NAME_CARD_LISTS_HEIGHT: f32 = 400.;
const DOC_LINES: usize = 2;
/// Wide enough for five digits of a line number in the code font at `text_xs`.
const LINE_GUTTER: f32 = 40.;

/// What a click on a row of the card does: jumps in the tour or opens the code.
pub type OnNameTarget = Rc<dyn Fn(&NameTarget, &mut Window, &mut App)>;

/// What a click on Yes or No under the card's question does.
pub type OnServerAnswer = Rc<dyn Fn(ServerAnswer, &mut Window, &mut App)>;

/// `room` is the width the window leaves for the card, so a narrow window shrinks it.
pub fn name_card(
    colors: &Colors,
    card: &DefinitionCard,
    on_open: &OnNameTarget,
    on_answer: &OnServerAnswer,
    room: Pixels,
) -> Div {
    let panel = overlay_panel(colors).w(px(NAME_CARD_WIDTH)).max_w(room);
    match card {
        DefinitionCard::Loading { says, .. } | DefinitionCard::Waiting { says, .. } => panel.child(
            line(colors.muted_text, SharedString::from(Arc::clone(says))),
        ),
        DefinitionCard::Failed { says, .. } => {
            panel.child(line(colors.danger, SharedString::from(Arc::clone(says))))
        }
        DefinitionCard::Found(found) => panel
            .child(header(colors, found))
            .children(
                found
                    .question
                    .as_ref()
                    .map(|question| question_band(colors, question, on_answer)),
            )
            .child(found_lists(colors, found, on_open)),
    }
}

fn line(color: gpui::Rgba, text: SharedString) -> Div {
    div().px_4().py_3().text_sm().text_color(color).child(text)
}

fn header(colors: &Colors, found: &FoundNames) -> Div {
    div()
        .flex_none()
        .px_4()
        .py_2()
        .flex()
        .items_baseline()
        .justify_between()
        .gap_4()
        .border_b_1()
        .border_color(colors.subtle_border)
        .child(
            div()
                .min_w_0()
                .truncate()
                .font_family(*CODE_FONT)
                .text_sm()
                .font_weight(FontWeight::SEMIBOLD)
                .child(SharedString::from(found.word.shared_text())),
        )
        .child(
            div()
                .flex_none()
                .whitespace_nowrap()
                .text_xs()
                .text_color(colors.muted_text)
                .child(SharedString::from(Arc::clone(&found.counts))),
        )
}

fn question_band(colors: &Colors, question: &ServerQuestion, on_answer: &OnServerAnswer) -> Div {
    let answer_button = |id: &'static str, label: &'static str, answer: ServerAnswer| {
        let on_answer = Rc::clone(on_answer);
        button(colors, id, ButtonKind::Secondary, ButtonSize::Small)
            .aria_label(label)
            .on_click(move |_, window, cx| on_answer(answer, window, cx))
            .child(label)
    };
    div()
        .flex_none()
        .px_4()
        .py_2()
        .flex()
        .items_center()
        .justify_between()
        .gap_4()
        .border_b_1()
        .border_color(colors.subtle_border)
        .bg(colors.header_band())
        .text_xs()
        .child(
            div()
                .min_w_0()
                .child(SharedString::from(Arc::clone(&question.says))),
        )
        .child(
            div()
                .flex_none()
                .flex()
                .gap_2()
                .child(answer_button(
                    "name-card-servers-yes",
                    "Yes",
                    ServerAnswer::Yes,
                ))
                .child(answer_button(
                    "name-card-servers-no",
                    "No",
                    ServerAnswer::No,
                )),
        )
}

fn found_lists(colors: &Colors, found: &FoundNames, on_open: &OnNameTarget) -> Div {
    let nothing_found = found.definitions.is_empty() && found.usages.is_empty();
    let both = !found.definitions.is_empty() && !found.usages.is_empty();
    let notes = (!found.note_lines.is_empty()).then(|| {
        div()
            .flex_none()
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
            .children(
                found
                    .note_lines
                    .iter()
                    .map(|line| div().child(SharedString::from(Arc::clone(line)))),
            )
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
                .when(found.definitions.is_empty(), |lists| {
                    lists.child(
                        div()
                            .flex_none()
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
                .when(both, |lists| {
                    lists.child(
                        div()
                            .flex_none()
                            .h(px(1.))
                            .mx_4()
                            .my_2()
                            .bg(colors.subtle_border),
                    )
                })
                .children(
                    found
                        .usages
                        .iter()
                        .enumerate()
                        .map(|(index, file)| usage_file(colors, index, file, on_open)),
                )
                .children(found.more_usages.as_ref().map(|label| {
                    div()
                        .flex_none()
                        .px_4()
                        .pt_2()
                        .pb_1()
                        .text_xs()
                        .text_color(colors.muted_text)
                        .child(SharedString::from(Arc::clone(label)))
                })),
        )
        .children(notes)
}

fn definition_row(
    colors: &Colors,
    index: usize,
    row: &DefinitionRow,
    on_open: &OnNameTarget,
) -> impl IntoElement {
    let (target, on_open) = (row.target.clone(), Rc::clone(on_open));
    picker_row_at(colors, ("name-definition", index), RowPlace::Apart, None)
        .flex_none()
        .py_1p5()
        .flex_col()
        .items_stretch()
        .gap_0p5()
        .aria_label(format!("Open definition in {}", row.place))
        .on_click(move |_, window, cx| on_open(&target, window, cx))
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .font_family(*CODE_FONT)
                        .text_sm()
                        .child(code_text(colors, &row.signature, &row.signature_runs)),
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
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis_start()
                .font_family(*CODE_FONT)
                .text_xs()
                .text_color(colors.muted_text)
                .child(SharedString::from(Arc::clone(&row.place))),
        )
}

fn usage_file(colors: &Colors, index: usize, file: &UsageFile, on_open: &OnNameTarget) -> Div {
    let (target, open_file) = (file.target.clone(), Rc::clone(on_open));
    div()
        .flex_none()
        .flex()
        .flex_col()
        .when(index > 0, Styled::mt_1)
        .child(
            picker_row_at(colors, ("name-usage-file", index), RowPlace::Apart, None)
                .py_1()
                .gap_2()
                .font_family(*CODE_FONT)
                .text_xs()
                .aria_label(format!("Open {}", file.name))
                .on_click(move |_, window, cx| open_file(&target, window, cx))
                .child(
                    div()
                        .flex_none()
                        .max_w(px(280.))
                        .truncate()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(SharedString::from(Arc::clone(&file.name))),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis_start()
                        .text_color(colors.muted_text)
                        .child(SharedString::from(Arc::clone(&file.folder))),
                )
                .child(
                    div()
                        .flex_none()
                        .text_color(colors.muted_text)
                        .child(SharedString::from(Arc::clone(&file.count))),
                ),
        )
        .children(
            file.lines
                .iter()
                .enumerate()
                .map(|(line, row)| usage_line(colors, (index, line), row, on_open)),
        )
}

fn usage_line(
    colors: &Colors,
    (file, line): (usize, usize),
    row: &UsageLine,
    on_open: &OnNameTarget,
) -> impl IntoElement {
    let (target, on_open) = (row.target.clone(), Rc::clone(on_open));
    picker_row_at(
        colors,
        ("name-usage-line", file * 1000 + line),
        RowPlace::Apart,
        None,
    )
    .flex_none()
    .py_0p5()
    .gap_3()
    .font_family(*CODE_FONT)
    .text_xs()
    .aria_label(format!("Open line {}", row.number))
    .on_click(move |_, window, cx| on_open(&target, window, cx))
    .child(
        div()
            .flex_none()
            .w(px(LINE_GUTTER))
            .text_right()
            .text_color(colors.muted_text)
            .child(SharedString::from(Arc::clone(&row.number))),
    )
    .child(div().flex_1().min_w_0().truncate().child(code_text(
        colors,
        &row.snippet.text,
        &row.snippet.runs,
    )))
}

/// The text coloured like the diff's code; the hovered name is semibold in its own colour.
fn code_text(colors: &Colors, text: &Arc<str>, runs: &[CodeRun]) -> StyledText {
    StyledText::new(SharedString::from(Arc::clone(text))).with_highlights(runs.iter().map(|run| {
        let color = run
            .capture
            .map_or(colors.text, |capture| colors.code(capture));
        (
            run.range.clone(),
            HighlightStyle {
                color: Some(color.into()),
                font_weight: run.is_name.then_some(FontWeight::SEMIBOLD),
                ..HighlightStyle::default()
            },
        )
    }))
}
