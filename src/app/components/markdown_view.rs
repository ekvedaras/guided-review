use gpui::{
    AnyElement, App, ElementId, FontStyle, FontWeight, HighlightStyle, InteractiveElement,
    InteractiveText, IntoElement, ParentElement, RenderOnce, SharedString,
    StatefulInteractiveElement, StrikethroughStyle, Styled, StyledText, UnderlineStyle, Window,
    div, px,
};

use crate::viewer::{Block, Capture, Markdown, Run, SuggestionDiff, highlight_snippet};

use super::palette::{CODE_FONT, Palette};

/// A comment body: the one component every place that shows GitHub markdown uses.
/// `key` tells this body's `<details>` folds apart from the folds of other bodies.
#[derive(IntoElement)]
pub struct MarkdownView {
    key: SharedString,
    markdown: Markdown,
    /// The lines the comment is on, so a suggestion block shows as a diff against them.
    commented_lines: Vec<String>,
}

impl MarkdownView {
    pub fn new(key: impl Into<SharedString>, markdown: Markdown) -> Self {
        Self {
            key: key.into(),
            markdown,
            commented_lines: Vec::new(),
        }
    }

    pub fn on_lines(mut self, commented_lines: Vec<String>) -> Self {
        self.commented_lines = commented_lines;
        self
    }
}

impl RenderOnce for MarkdownView {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        if self.commented_lines.is_empty() {
            return blocks(&self.markdown.blocks, &self.key, window, cx);
        }
        div()
            .flex()
            .flex_col()
            .gap_2()
            .children(
                self.markdown
                    .blocks
                    .iter()
                    .enumerate()
                    .map(|(index, block)| match block {
                        Block::Code {
                            language: Some(language),
                            text,
                        } if language == "suggestion" => {
                            suggestion_view(&SuggestionDiff::of(&self.commented_lines, text))
                        }
                        _ => block_element(block, &format!("{}-{index}", self.key), window, cx),
                    }),
            )
            .into_any_element()
    }
}

fn suggestion_view(diff: &SuggestionDiff) -> AnyElement {
    let rows = |lines: &[String], sign: &'static str, background: gpui::Rgba| {
        lines
            .iter()
            .map(|line| {
                div()
                    .flex()
                    .gap_2()
                    .bg(background)
                    .whitespace_nowrap()
                    .min_h(px(14.))
                    .child(div().flex_none().w(px(12.)).child(sign))
                    .child(line.clone())
            })
            .collect::<Vec<_>>()
    };
    div()
        .flex()
        .flex_col()
        .rounded_md()
        .border_1()
        .border_color(Palette::border())
        .font_family(CODE_FONT)
        .text_sm()
        .overflow_hidden()
        .child(
            div()
                .px_2()
                .py_1()
                .bg(Palette::sidebar())
                .text_color(Palette::muted_text())
                .child("Suggested change"),
        )
        .children(rows(&diff.removed, "-", Palette::removed_line()))
        .children(rows(&diff.added, "+", Palette::added_line()))
        .into_any_element()
}

fn blocks(items: &[Block], key: &str, window: &mut Window, cx: &mut App) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap_2()
        .children(
            items
                .iter()
                .enumerate()
                .map(|(index, block)| block_element(block, &format!("{key}-{index}"), window, cx)),
        )
        .into_any_element()
}

fn block_element(block: &Block, key: &str, window: &mut Window, cx: &mut App) -> AnyElement {
    match block {
        Block::Paragraph(runs) => inline_text(runs, key, 1.0).into_any_element(),
        Block::Heading { level, text } => {
            let scale = [1.5, 1.3, 1.15, 1.05, 1.0, 1.0][usize::from(*level - 1).min(5)];
            let heading = text
                .iter()
                .map(|run| Run {
                    bold: true,
                    ..run.clone()
                })
                .collect::<Vec<_>>();
            inline_text(&heading, key, scale).into_any_element()
        }
        Block::List {
            first_number,
            items,
        } => div()
            .flex()
            .flex_col()
            .gap_1()
            .children(items.iter().enumerate().map(|(index, item)| {
                let marker = match first_number {
                    Some(first) => format!("{}.", first + index as u64),
                    None => "•".to_string(),
                };
                div()
                    .flex()
                    .gap_2()
                    .child(div().flex_none().w(px(20.)).text_right().child(marker))
                    .child(div().flex_1().child(blocks(
                        item,
                        &format!("{key}-{index}"),
                        window,
                        cx,
                    )))
            }))
            .into_any_element(),
        Block::Code { language, text } => code_block(language.as_deref(), text),
        Block::Quote(inner) => div()
            .pl_3()
            .border_l_2()
            .border_color(Palette::border())
            .text_color(Palette::muted_text())
            .child(blocks(inner, key, window, cx))
            .into_any_element(),
        Block::Details { summary, body } => details(summary, body, key, window, cx),
        Block::Rule => div().h(px(1.)).bg(Palette::border()).into_any_element(),
    }
}

fn details(
    summary: &[Run],
    body: &[Block],
    key: &str,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let is_open = window.use_keyed_state(
        ElementId::Name(format!("details-{key}").into()),
        cx,
        |_, _| false,
    );
    let open = *is_open.read(cx);
    let title = if summary.is_empty() {
        vec![Run {
            text: "Details".into(),
            ..Run::default()
        }]
    } else {
        summary.to_vec()
    };
    let header = div()
        .id(SharedString::from(format!("details-toggle-{key}")))
        .flex()
        .gap_2()
        .cursor_pointer()
        .on_click(move |_, _, cx| {
            is_open.update(cx, |open, cx| {
                *open = !*open;
                cx.notify();
            })
        })
        .child(div().flex_none().child(if open { "▾" } else { "▸" }))
        .child(
            div()
                .flex_1()
                .child(inline_text(&title, &format!("{key}-summary"), 1.0)),
        );
    div()
        .flex()
        .flex_col()
        .gap_2()
        .child(header)
        .when_open(open, blocks(body, key, window, cx))
        .into_any_element()
}

trait WhenOpen: ParentElement + Sized {
    fn when_open(self, open: bool, content: AnyElement) -> Self {
        if open {
            self.child(div().pl_4().child(content))
        } else {
            self
        }
    }
}

impl<E: ParentElement> WhenOpen for E {}

fn code_block(language: Option<&str>, text: &str) -> AnyElement {
    let spans = highlight_snippet(language, text);
    div()
        .flex()
        .flex_col()
        .px_2()
        .py_1()
        .rounded_md()
        .bg(Palette::sidebar())
        .border_1()
        .border_color(Palette::border())
        .font_family(CODE_FONT)
        .text_sm()
        .overflow_hidden()
        .children(text.lines().zip(spans).map(|(line, spans)| {
            let highlights: Vec<_> = spans
                .iter()
                .filter(|span| {
                    line.is_char_boundary(span.range.start) && line.is_char_boundary(span.range.end)
                })
                .map(|span| {
                    (
                        span.range.clone(),
                        HighlightStyle {
                            color: Some(Palette::code(span.capture).into()),
                            ..Default::default()
                        },
                    )
                })
                .collect();
            div()
                .whitespace_nowrap()
                .min_h(px(14.))
                .child(StyledText::new(line.to_string()).with_highlights(highlights))
        }))
        .into_any_element()
}

fn inline_text(runs: &[Run], key: &str, scale: f32) -> AnyElement {
    let mut text = String::new();
    let mut highlights = Vec::new();
    let mut links = Vec::new();
    for run in runs {
        let range = text.len()..text.len() + run.text.len();
        text.push_str(&run.text);
        let is_styled =
            run.bold || run.italic || run.strikethrough || run.code || run.link.is_some();
        if is_styled && !range.is_empty() {
            highlights.push((range.clone(), style_of(run)));
        }
        if let Some(url) = &run.link {
            links.push((range, url.clone()));
        }
    }
    let styled = StyledText::new(text).with_highlights(highlights);
    let element = div().text_size(gpui::rems(scale));
    if links.is_empty() {
        return element.child(styled).into_any_element();
    }
    let (ranges, urls): (Vec<_>, Vec<_>) = links.into_iter().unzip();
    element
        .child(
            InteractiveText::new(SharedString::from(format!("text-{key}")), styled)
                .on_click(ranges, move |index, _, cx| cx.open_url(&urls[index])),
        )
        .into_any_element()
}

fn style_of(run: &Run) -> HighlightStyle {
    let color = if run.link.is_some() {
        Some(Palette::code(Capture::Function).into())
    } else if run.code {
        Some(Palette::code(Capture::String).into())
    } else {
        None
    };
    HighlightStyle {
        color,
        font_weight: run.bold.then_some(FontWeight::BOLD),
        font_style: run.italic.then_some(FontStyle::Italic),
        background_color: run.code.then(|| Palette::sidebar().into()),
        underline: run.link.is_some().then(|| UnderlineStyle {
            thickness: px(1.),
            color: None,
            wavy: false,
        }),
        strikethrough: run.strikethrough.then(|| StrikethroughStyle {
            thickness: px(1.),
            color: None,
        }),
        ..Default::default()
    }
}
