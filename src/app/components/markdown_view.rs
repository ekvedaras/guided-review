use gpui::{
    AnyElement, App, BorrowAppContext, FontStyle, FontWeight, HighlightStyle, InteractiveElement,
    InteractiveText, IntoElement, ParentElement, RenderOnce, SharedString,
    StatefulInteractiveElement, StrikethroughStyle, Styled, StyledText, UnderlineStyle, Window,
    div, px,
};

use std::collections::HashSet;

use gpui::Global;

use crate::app::code_links::open_link;
use crate::viewer::{Block, Capture, Markdown, Run, SuggestionDiff, highlight_snippet};

use super::palette::{CODE_FONT, Colors};

/// The `<details>` the reader opened, by the key of the body they are in. Kept in one place, not in each
/// body, so the layout can see them: an open one makes its row taller.
#[derive(Default)]
pub struct OpenDetails(HashSet<String>);

impl Global for OpenDetails {}

impl OpenDetails {
    fn toggle(&mut self, key: &str) {
        if !self.0.remove(key) {
            self.0.insert(key.to_string());
        }
    }

    /// The open ones whose key starts with `prefix`, in a fixed order.
    pub fn open_under(&self, prefix: &str) -> Vec<String> {
        let mut open: Vec<String> = self
            .0
            .iter()
            .filter(|key| key.starts_with(prefix))
            .cloned()
            .collect();
        open.sort();
        open
    }
}

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
        let colors = Colors::active(cx);
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
                        } if language == "suggestion" => suggestion_view(
                            colors,
                            &SuggestionDiff::of(&self.commented_lines, text),
                        ),
                        _ => block_element(block, &format!("{}-{index}", self.key), window, cx),
                    }),
            )
            .into_any_element()
    }
}

fn suggestion_view(colors: Colors, diff: &SuggestionDiff) -> AnyElement {
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
        .border_color(colors.border)
        .font_family(CODE_FONT)
        .text_sm()
        .overflow_hidden()
        .child(
            div()
                .px_2()
                .py_1()
                .bg(colors.surface)
                .text_color(colors.muted_text)
                .child("Suggested change"),
        )
        .children(rows(&diff.removed, "-", colors.removed_line))
        .children(rows(&diff.added, "+", colors.added_line))
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
    let colors = Colors::active(cx);
    match block {
        Block::Paragraph(runs) => inline_text(colors, runs, key, 1.0).into_any_element(),
        Block::Heading { level, text } => {
            let scale = [1.5, 1.3, 1.15, 1.05, 1.0, 1.0][usize::from(*level - 1).min(5)];
            let heading = text
                .iter()
                .map(|run| Run {
                    bold: true,
                    ..run.clone()
                })
                .collect::<Vec<_>>();
            inline_text(colors, &heading, key, scale).into_any_element()
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
        Block::Code { language, text } => code_block(colors, language.as_deref(), text),
        Block::Quote(inner) => div()
            .pl_3()
            .border_l_2()
            .border_color(colors.border)
            .text_color(colors.muted_text)
            .child(blocks(inner, key, window, cx))
            .into_any_element(),
        Block::Details { summary, body } => details(summary, body, key, window, cx),
        Block::Rule => div().h(px(1.)).bg(colors.border).into_any_element(),
    }
}

fn details(
    summary: &[Run],
    body: &[Block],
    key: &str,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let colors = Colors::active(cx);
    let open = cx
        .try_global::<OpenDetails>()
        .is_some_and(|details| details.0.contains(key));
    let toggled_key = key.to_string();
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
            if !cx.has_global::<OpenDetails>() {
                cx.set_global(OpenDetails::default());
            }
            cx.update_global::<OpenDetails, _>(|details, _| details.toggle(&toggled_key));
            cx.refresh_windows();
        })
        .child(div().flex_none().child(if open { "▾" } else { "▸" }))
        .child(
            div()
                .flex_1()
                .child(inline_text(colors, &title, &format!("{key}-summary"), 1.0)),
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

fn code_block(colors: Colors, language: Option<&str>, text: &str) -> AnyElement {
    let spans = highlight_snippet(language, text);
    div()
        .flex()
        .flex_col()
        .px_2()
        .py_1()
        .rounded_md()
        .bg(colors.surface)
        .border_1()
        .border_color(colors.border)
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
                            color: Some(colors.code(span.capture).into()),
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

/// Tour text the writer wrote: a title, a bullet, a note. One line of runs that takes its size and colour
/// from the element around it.
#[derive(IntoElement)]
pub struct InlineMarkdown {
    key: SharedString,
    runs: Vec<Run>,
}

impl InlineMarkdown {
    pub fn new(key: impl Into<SharedString>, source: &str) -> Self {
        Self {
            key: key.into(),
            runs: runs_of(&Markdown::parse(source).blocks),
        }
    }
}

impl RenderOnce for InlineMarkdown {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        styled_runs(Colors::active(cx), &self.runs, &self.key)
    }
}

fn runs_of(blocks: &[Block]) -> Vec<Run> {
    let mut runs: Vec<Run> = Vec::new();
    for block in blocks {
        let block_runs = match block {
            Block::Paragraph(runs) | Block::Heading { text: runs, .. } => runs.clone(),
            Block::Details { summary, .. } => summary.clone(),
            Block::Quote(inner) => runs_of(inner),
            Block::List { items, .. } => items.iter().flat_map(|item| runs_of(item)).collect(),
            Block::Code { text, .. } => vec![Run {
                text: text.clone(),
                code: true,
                ..Run::default()
            }],
            Block::Rule => Vec::new(),
        };
        if !runs.is_empty() && !block_runs.is_empty() {
            runs.push(Run {
                text: " ".into(),
                ..Run::default()
            });
        }
        runs.extend(block_runs);
    }
    runs
}

fn inline_text(colors: Colors, runs: &[Run], key: &str, scale: f32) -> AnyElement {
    div()
        .text_size(gpui::rems(scale))
        .child(styled_runs(colors, runs, key))
        .into_any_element()
}

fn styled_runs(colors: Colors, runs: &[Run], key: &str) -> AnyElement {
    let mut text = String::new();
    let mut highlights = Vec::new();
    let mut links = Vec::new();
    for run in runs {
        let range = text.len()..text.len() + run.text.len();
        text.push_str(&run.text);
        let is_styled =
            run.bold || run.italic || run.strikethrough || run.code || run.link.is_some();
        if is_styled && !range.is_empty() {
            highlights.push((range.clone(), style_of(colors, run)));
        }
        if let Some(url) = &run.link {
            links.push((range, url.clone()));
        }
    }
    let styled = StyledText::new(text).with_highlights(highlights);
    if links.is_empty() {
        return styled.into_any_element();
    }
    let (ranges, urls): (Vec<_>, Vec<_>) = links.into_iter().unzip();
    InteractiveText::new(SharedString::from(format!("text-{key}")), styled)
        .on_click(ranges, move |index, _, cx| open_link(&urls[index], cx))
        .into_any_element()
}

fn style_of(colors: Colors, run: &Run) -> HighlightStyle {
    let color = if run.link.is_some() {
        Some(colors.code(Capture::Function).into())
    } else if run.code {
        Some(colors.code(Capture::String).into())
    } else {
        None
    };
    HighlightStyle {
        color,
        font_weight: run.bold.then_some(FontWeight::BOLD),
        font_style: run.italic.then_some(FontStyle::Italic),
        background_color: run.code.then(|| colors.surface.into()),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tour_text_keeps_its_bold_and_code_and_drops_the_markers() {
        let runs = runs_of(&Markdown::parse("**Too little stock** joins `Reason`").blocks);

        let text: String = runs.iter().map(|run| run.text.as_str()).collect();
        assert_eq!(text, "Too little stock joins Reason");
        assert!(runs[0].bold);
        assert!(runs.last().is_some_and(|run| run.code));
    }
}
