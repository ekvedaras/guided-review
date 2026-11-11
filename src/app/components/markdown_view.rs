use gpui::{
    AnyElement, App, Div, FontStyle, FontWeight, HighlightStyle, InteractiveElement,
    InteractiveText, IntoElement, ParentElement, RenderOnce, SharedString,
    StatefulInteractiveElement, StrikethroughStyle, Styled, StyledText, TextLayout, UnderlineStyle,
    Window, canvas, div, fill, img, point, px,
};
use std::path::Path;

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;

use crate::app::active_session::{details_are_open, toggle_details};
use crate::app::code_links::open_link;
use crate::image_facts::is_svg_path;
use crate::viewer::{Block, Capture, FenceLine, Markdown, Run, Span, fence_lines};

use super::diagram::{DiagramSet, DiagramView};
use super::icon::{Icon, sized_icon};
use super::motion::turn_to;
use super::palette::{CODE_FONT, CONTROL_RADIUS, Colors, INNER_RADIUS};
use super::picture_zoom::{ZoomedPicture, zoom_picture};

/// The mark before an item of a list without numbers: a small dot, centred on the item's first line.
pub fn bullet(colors: &Colors, line_height: gpui::Pixels) -> Div {
    div()
        .flex_none()
        .h(line_height)
        .flex()
        .items_center()
        .child(div().size(px(5.)).rounded_full().bg(colors.muted_text))
}

/// The line height of `text_sm`, which markdown on cards is set in.
const SMALL_LINE: gpui::Pixels = px(22.);

/// The `<details>` the reader opened, by the key of the body they are in. Kept in one place, not in each
/// body, so the layout can see them: an open one makes its row taller.
#[derive(Default)]
pub struct OpenDetails(HashSet<String>);

impl OpenDetails {
    pub fn is_open(&self, key: &str) -> bool {
        self.0.contains(key)
    }

    pub fn toggle(&mut self, key: &str) {
        if !self.0.remove(key) {
            self.0.insert(key.to_owned());
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

/// Texts parsed once and shared by every frame that draws them. Keyed by the source text itself, so a
/// changed text is a new key and is parsed again; nothing is served stale.
struct ParsedTexts<T> {
    by_source: HashMap<Box<str>, Rc<T>>,
}

impl<T> ParsedTexts<T> {
    /// More distinct texts than a review shows at once; past it the memo starts over instead of growing.
    const MOST: usize = 4096;

    fn new() -> Self {
        Self {
            by_source: HashMap::new(),
        }
    }

    fn get_or_parse(&mut self, source: &str, parse: impl FnOnce(&str) -> T) -> Rc<T> {
        if let Some(parsed) = self.by_source.get(source) {
            return Rc::clone(parsed);
        }
        if self.by_source.len() >= Self::MOST {
            self.by_source.clear();
        }
        let parsed = Rc::new(parse(source));
        self.by_source.insert(source.into(), Rc::clone(&parsed));
        parsed
    }
}

thread_local! {
    static PARSED_MARKDOWN: RefCell<ParsedTexts<PreparedMarkdown>> = RefCell::new(ParsedTexts::new());
    static PARSED_INLINE: RefCell<ParsedTexts<InlineText>> = RefCell::new(ParsedTexts::new());
}

/// The markdown of `source`, parsed on its first draw and shared after that.
pub fn parsed_markdown(source: &str) -> Rc<PreparedMarkdown> {
    PARSED_MARKDOWN.with_borrow_mut(|parsed| {
        parsed.get_or_parse(source, |source| {
            PreparedMarkdown::of(Markdown::parse(source))
        })
    })
}

/// A parsed body with everything drawing it needs made once: text joined, headings made bold, code lines
/// cut and their colours checked, a suggestion's added lines split. Drawing only borrows it.
#[derive(Debug, PartialEq)]
pub struct PreparedMarkdown {
    pub(super) markdown: Markdown,
    blocks: Vec<PreparedBlock>,
}

impl PreparedMarkdown {
    pub fn of(markdown: Markdown) -> Self {
        let blocks = prepare(&markdown.blocks);
        Self { markdown, blocks }
    }

    pub fn diagrams(&self) -> Vec<&str> {
        self.markdown.diagrams()
    }

    #[cfg(feature = "snapshots")]
    /// The keys `MarkdownView` gives its top-level `<details>` folds when drawn under `key`.
    pub fn fold_keys(&self, key: &str) -> Vec<String> {
        self.blocks
            .iter()
            .enumerate()
            .filter(|(_, block)| matches!(block, PreparedBlock::Details { .. }))
            .map(|(index, _)| format!("{key}-{index}"))
            .collect()
    }

    #[cfg(feature = "snapshots")]
    /// Its first picture outside any list, quote or fold.
    pub fn first_picture(&self) -> Option<ZoomedPicture> {
        self.blocks.iter().find_map(PreparedBlock::zoomed)
    }
}

impl std::hash::Hash for PreparedMarkdown {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.markdown.hash(state);
    }
}

#[derive(Debug, PartialEq)]
enum PreparedBlock {
    Text {
        text: Rc<InlineText>,
        scale: f32,
    },
    List {
        first_number: Option<u64>,
        items: Vec<Vec<Self>>,
    },
    Code {
        lines: Vec<CodeLine>,
    },
    /// Drawn as code where the view has no diagram for it.
    Diagram {
        source: Arc<str>,
        lines: Vec<CodeLine>,
    },
    /// Drawn as a diff against the commented lines when there are some, as plain code when not.
    Suggestion {
        added: Vec<SharedString>,
        lines: Vec<CodeLine>,
    },
    Quote(Vec<Self>),
    Details {
        summary: Rc<InlineText>,
        body: Vec<Self>,
    },
    Rule,
    Picture {
        path: Arc<Path>,
        /// Width over height, so the picture's room is kept before it loads.
        ratio: f32,
        /// Its width at the screen's two pixels to a point, the widest it is drawn.
        widest: f32,
    },
}

impl PreparedBlock {
    fn zoomed(&self) -> Option<ZoomedPicture> {
        match self {
            Self::Picture {
                path,
                ratio,
                widest,
            } => Some(ZoomedPicture {
                path: Arc::clone(path),
                ratio: *ratio,
                pixel_width: if is_svg_path(&path.to_string_lossy()) {
                    f32::INFINITY
                } else {
                    widest * 2.
                },
            }),
            _ => None,
        }
    }
}

#[derive(Debug, PartialEq)]
struct CodeLine {
    text: SharedString,
    /// Only the colours that cut the line on char boundaries.
    highlights: Vec<(Range<usize>, Capture)>,
    kind: FenceLine,
}

fn prepare(blocks: &[Block]) -> Vec<PreparedBlock> {
    blocks.iter().map(prepare_block).collect()
}

fn prepare_block(block: &Block) -> PreparedBlock {
    if let (Some(source), Block::Code { text, spans, .. }) = (block.diagram(), block) {
        return PreparedBlock::Diagram {
            source: source.into(),
            lines: code_lines(None, text, spans),
        };
    }
    match block {
        Block::Paragraph(runs) => PreparedBlock::Text {
            text: Rc::new(InlineText::of(runs)),
            scale: 1.0,
        },
        Block::Heading { level, text } => {
            let scale = [1.5, 1.3, 1.15, 1.05, 1.0, 1.0]
                .get(usize::from(level.saturating_sub(1)))
                .copied()
                .unwrap_or(1.0);
            let bold: Vec<Run> = text
                .iter()
                .map(|run| Run {
                    bold: true,
                    ..run.clone()
                })
                .collect();
            PreparedBlock::Text {
                text: Rc::new(InlineText::of(&bold)),
                scale,
            }
        }
        Block::List {
            first_number,
            items,
        } => PreparedBlock::List {
            first_number: *first_number,
            items: items.iter().map(|item| prepare(item)).collect(),
        },
        Block::Code {
            language,
            text,
            spans,
        } => {
            let lines = code_lines(language.as_deref(), text, spans);
            if language.as_deref() == Some("suggestion") {
                PreparedBlock::Suggestion {
                    added: text
                        .lines()
                        .map(|line| SharedString::from(line.to_owned()))
                        .collect(),
                    lines,
                }
            } else {
                PreparedBlock::Code { lines }
            }
        }
        Block::Quote(inner) => PreparedBlock::Quote(prepare(inner)),
        Block::Details { summary, body } => PreparedBlock::Details {
            summary: Rc::new(InlineText::of(&details_title(summary))),
            body: prepare(body),
        },
        Block::Rule => PreparedBlock::Rule,
        Block::Picture {
            path,
            width,
            height,
            ..
        } => PreparedBlock::Picture {
            path: Arc::from(path.as_path()),
            ratio: *width as f32 / (*height).max(1) as f32,
            widest: *width as f32 / 2.,
        },
    }
}

fn details_title(summary: &[Run]) -> Vec<Run> {
    if summary.is_empty() {
        vec![Run {
            text: "Details".into(),
            ..Run::default()
        }]
    } else {
        summary.to_vec()
    }
}

fn code_lines(language: Option<&str>, text: &str, spans: &[Vec<Span>]) -> Vec<CodeLine> {
    text.lines()
        .zip(spans)
        .zip(fence_lines(language, text))
        .map(|((line, spans), kind)| CodeLine {
            kind,
            text: SharedString::from(line.to_owned()),
            highlights: spans
                .iter()
                .filter(|span| {
                    let range = span.range();
                    line.is_char_boundary(range.start) && line.is_char_boundary(range.end)
                })
                .map(|span| (span.range(), span.capture))
                .collect(),
        })
        .collect()
}

fn parsed_inline(source: &str) -> Rc<InlineText> {
    PARSED_INLINE.with_borrow_mut(|parsed| {
        parsed.get_or_parse(source, |source| {
            InlineText::of(&runs_of(&Markdown::parse(source).blocks))
        })
    })
}

/// A comment body: the one component every place that shows GitHub markdown uses.
/// `key` tells this body's `<details>` folds apart from the folds of other bodies.
#[derive(IntoElement)]
pub struct MarkdownView {
    key: SharedString,
    markdown: Rc<PreparedMarkdown>,
    /// The lines the comment is on, so a suggestion block shows as a diff against them.
    commented_lines: Arc<[Arc<str>]>,
    embeds: Embeds,
}

/// What a body shows in place of its own text: drawn diagrams, and something under each picture.
#[derive(Default)]
struct Embeds {
    diagrams: DiagramSet,
    under_picture: Option<UnderPicture>,
}

/// Drawn under a picture of the tour's folder, by its path; `None` draws nothing.
pub type UnderPicture = Rc<dyn Fn(&Path, &mut App) -> Option<AnyElement>>;

impl MarkdownView {
    pub fn new(key: impl Into<SharedString>, markdown: Rc<PreparedMarkdown>) -> Self {
        Self {
            key: key.into(),
            markdown,
            commented_lines: Arc::new([]),
            embeds: Embeds::default(),
        }
    }

    /// Mermaid fences show as these diagrams; a fence with none in the set shows as code.
    pub fn with_diagrams(mut self, diagrams: DiagramSet) -> Self {
        self.embeds.diagrams = diagrams;
        self
    }

    pub fn under_each_picture(mut self, under_picture: UnderPicture) -> Self {
        self.embeds.under_picture = Some(under_picture);
        self
    }

    pub fn on_lines(mut self, commented_lines: Arc<[Arc<str>]>) -> Self {
        self.commented_lines = commented_lines;
        self
    }
}

impl RenderOnce for MarkdownView {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        blocks(
            &self.markdown.blocks,
            &self.key,
            &self.commented_lines,
            &self.embeds,
            window,
            cx,
        )
    }
}

fn suggestion_view(colors: &Colors, removed: &[Arc<str>], added: &[SharedString]) -> AnyElement {
    let row = |line: SharedString, sign: &'static str, background: gpui::Rgba| {
        div()
            .flex()
            .gap_2()
            .bg(background)
            .whitespace_nowrap()
            .min_h(px(14.))
            .child(div().flex_none().w(px(12.)).child(sign))
            .child(line)
    };
    div()
        .flex()
        .flex_col()
        .rounded(px(CONTROL_RADIUS))
        .border_1()
        .border_color(colors.subtle_border)
        .bg(colors.background)
        .font_family(CODE_FONT)
        .text_sm()
        .overflow_hidden()
        .child(
            div()
                .px_2()
                .py_1()
                .bg(colors.header_band())
                .border_b_1()
                .border_color(colors.subtle_border)
                .text_color(colors.muted_text)
                .child("Suggested change"),
        )
        .children(removed.iter().map(|line| {
            row(
                SharedString::from(Arc::clone(line)),
                "-",
                colors.removed_line,
            )
        }))
        .children(
            added
                .iter()
                .map(|line| row(line.clone(), "+", colors.added_line)),
        )
        .into_any_element()
}

/// A suggestion shows as a diff only among the top-level blocks of a body, where `commented_lines` belong.
fn blocks(
    items: &[PreparedBlock],
    key: &str,
    commented_lines: &[Arc<str>],
    embeds: &Embeds,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap_2()
        .children(items.iter().enumerate().map(|(index, block)| {
            block_element(
                block,
                &format!("{key}-{index}"),
                commented_lines,
                embeds,
                window,
                cx,
            )
        }))
        .into_any_element()
}

/// The width of a reviewer summary's text, the widest a picture in it is drawn.
const PICTURE_ROOM: f32 = 700.;

fn block_element(
    block: &PreparedBlock,
    key: &str,
    commented_lines: &[Arc<str>],
    embeds: &Embeds,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let colors = Colors::active(cx);
    match block {
        PreparedBlock::Text { text, scale } => inline_text(&colors, text, key, *scale),
        PreparedBlock::List {
            first_number,
            items,
        } => div()
            .flex()
            .flex_col()
            .gap_1()
            .children(items.iter().enumerate().map(|(index, item)| {
                let marker = match first_number {
                    Some(first) => div()
                        .text_right()
                        .child(format!("{}.", first + index as u64)),
                    None => div()
                        .flex()
                        .justify_end()
                        .child(bullet(&colors, SMALL_LINE)),
                };
                div()
                    .flex()
                    .gap_2()
                    .child(marker.flex_none().w(px(20.)))
                    .child(div().flex_1().min_w_0().child(blocks(
                        item,
                        &format!("{key}-{index}"),
                        &[],
                        embeds,
                        window,
                        cx,
                    )))
            }))
            .into_any_element(),
        PreparedBlock::Code { lines } => code_block(&colors, key, lines),
        PreparedBlock::Diagram { source, lines } => match embeds.diagrams.get(source) {
            Some(diagram) => DiagramView::new(diagram.clone()).into_any_element(),
            None => code_block(&colors, key, lines),
        },
        PreparedBlock::Suggestion { added, lines } => {
            if commented_lines.is_empty() {
                code_block(&colors, key, lines)
            } else {
                suggestion_view(&colors, commented_lines, added)
            }
        }
        PreparedBlock::Quote(inner) => div()
            .pl_3()
            .border_l_1()
            .border_color(colors.border)
            .text_color(colors.muted_text)
            .child(blocks(inner, key, &[], embeds, window, cx))
            .into_any_element(),
        PreparedBlock::Details { summary, body } => details(summary, body, key, embeds, window, cx),
        PreparedBlock::Rule => div().h(px(1.)).bg(colors.subtle_border).into_any_element(),
        PreparedBlock::Picture {
            path,
            ratio,
            widest,
        } => {
            // A size of its own: one taken from the box's width is measured before that width is
            // known, and the room kept is then that of the widest picture.
            let width = widest.min(PICTURE_ROOM);
            let zoomed = block.zoomed();
            let on_page = Rc::new(Cell::new(None));
            let measured = Rc::clone(&on_page);
            let picture = div()
                .id(SharedString::from(format!("picture-{key}")))
                .relative()
                .self_start()
                .max_w_full()
                .cursor_pointer()
                .on_click(move |_, window, cx| {
                    if let Some(zoomed) = &zoomed {
                        zoom_picture(zoomed.clone(), on_page.get(), window, cx);
                    }
                })
                .child(
                    canvas(
                        move |bounds, _, _| measured.set(Some(bounds)),
                        |_, (), _, _| {},
                    )
                    .absolute()
                    .size_full(),
                )
                .child(
                    img(Arc::clone(path))
                        .w(px(width))
                        .h(px(width / ratio))
                        .max_w_full()
                        .rounded(px(INNER_RADIUS)),
                );
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(picture)
                .children(
                    embeds
                        .under_picture
                        .as_ref()
                        .and_then(|under| under(path, cx)),
                )
                .into_any_element()
        }
    }
}

fn details(
    summary: &Rc<InlineText>,
    body: &[PreparedBlock],
    key: &str,
    embeds: &Embeds,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let colors = Colors::active(cx);
    let open = details_are_open(key, cx);
    let toggled_key = key.to_owned();
    let header = div()
        .id(SharedString::from(format!("details-toggle-{key}")))
        .flex()
        .gap_2()
        .cursor_pointer()
        .on_click(move |_, _, cx| {
            toggle_details(&toggled_key, cx);
        })
        .child(
            div()
                .flex_none()
                .h(SMALL_LINE)
                .flex()
                .items_center()
                .child(turn_to(
                    sized_icon(Icon::ChevronRight, colors.muted_text, px(12.)),
                    "details-chevron",
                    open,
                    1.,
                )),
        )
        .child(div().flex_1().min_w_0().child(inline_text(
            &colors,
            summary,
            &format!("{key}-summary"),
            1.0,
        )));
    div()
        .flex()
        .flex_col()
        .gap_2()
        .child(header)
        .when_open(open, blocks(body, key, &[], embeds, window, cx))
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

/// Long lines scroll sideways, as on GitHub; the "Wrap lines" setting does not reach markdown.
fn code_block(colors: &Colors, key: &str, lines: &[CodeLine]) -> AnyElement {
    div()
        .id(SharedString::from(format!("code-block-{key}")))
        .flex()
        .py_1p5()
        .rounded(px(CONTROL_RADIUS))
        .bg(colors.background)
        .border_1()
        .border_color(colors.subtle_border)
        .font_family(CODE_FONT)
        .text_sm()
        .overflow_x_scroll()
        .child(
            div()
                .flex()
                .flex_col()
                .flex_none()
                .min_w_full()
                .children(lines.iter().map(|line| code_line(colors, line))),
        )
        .into_any_element()
}

fn code_line(colors: &Colors, line: &CodeLine) -> Div {
    let highlights: Vec<_> = line
        .highlights
        .iter()
        .map(|(range, capture)| {
            (
                range.clone(),
                HighlightStyle {
                    color: Some(colors.code(*capture).into()),
                    ..Default::default()
                },
            )
        })
        .collect();
    let row = div()
        .px_2()
        .whitespace_nowrap()
        .min_h(px(14.))
        .child(StyledText::new(line.text.clone()).with_highlights(highlights));
    match line.kind {
        FenceLine::Added => row.bg(colors.added_line).text_color(colors.success),
        FenceLine::Removed => row.bg(colors.removed_line).text_color(colors.danger),
        FenceLine::Plain => row,
    }
}

/// Tour text the writer wrote: a title, a bullet, a note. One line of runs that takes its size and colour
/// from the element around it.
#[derive(IntoElement)]
pub struct InlineMarkdown {
    key: SharedString,
    text: Rc<InlineText>,
}

impl InlineMarkdown {
    pub fn new(key: impl Into<SharedString>, source: &str) -> Self {
        Self {
            key: key.into(),
            text: parsed_inline(source),
        }
    }
}

impl RenderOnce for InlineMarkdown {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        styled_text(&Colors::active(cx), &self.text, &self.key)
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
            Block::Picture { alt, .. } => vec![Run {
                text: alt.clone(),
                ..Run::default()
            }],
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

fn inline_text(colors: &Colors, text: &Rc<InlineText>, key: &str, scale: f32) -> AnyElement {
    div()
        .text_size(gpui::rems(scale))
        .child(styled_text(colors, text, key))
        .into_any_element()
}

/// Inline code gets a third of an em of space on each side inside its background, as padding.
const CODE_PADDING: &str = "\u{2004}";

/// Runs joined into one line of text, with no colours in it, so one parse serves every theme.
#[derive(Debug, PartialEq)]
struct InlineText {
    text: SharedString,
    styled: Vec<(Range<usize>, Run)>,
    links: Vec<(Range<usize>, String)>,
    code_ranges: Vec<Range<usize>>,
}

impl InlineText {
    fn of(runs: &[Run]) -> Self {
        let mut text = String::new();
        let mut styled = Vec::new();
        let mut links = Vec::new();
        let mut code_ranges = Vec::new();
        for run in runs {
            if run.code && !run.text.is_empty() {
                text.push_str(CODE_PADDING);
            }
            let range = text.len()..text.len() + run.text.len();
            text.push_str(&run.text);
            if run.code && !run.text.is_empty() {
                text.push_str(CODE_PADDING);
                code_ranges.push(range.start - CODE_PADDING.len()..text.len());
            }
            let is_styled =
                run.bold || run.italic || run.strikethrough || run.code || run.link.is_some();
            if is_styled && !range.is_empty() {
                styled.push((
                    range.clone(),
                    Run {
                        text: String::new(),
                        ..run.clone()
                    },
                ));
            }
            if let Some(url) = &run.link {
                links.push((range, url.clone()));
            }
        }
        Self {
            text: text.into(),
            styled,
            links,
            code_ranges,
        }
    }

    fn highlights(&self, colors: &Colors) -> Vec<(Range<usize>, HighlightStyle)> {
        self.styled
            .iter()
            .map(|(range, run)| (range.clone(), style_of(colors, run)))
            .collect()
    }
}

fn styled_text(colors: &Colors, inline: &Rc<InlineText>, key: &str) -> AnyElement {
    let styled = StyledText::new(inline.text.clone()).with_highlights(inline.highlights(colors));
    let backgrounds = (!inline.code_ranges.is_empty())
        .then(|| code_backgrounds(styled.layout().clone(), inline.code_ranges.clone()));
    let text = if inline.links.is_empty() {
        styled.into_any_element()
    } else {
        let ranges = inline
            .links
            .iter()
            .map(|(range, _)| range.clone())
            .collect();
        let links = Rc::clone(inline);
        InteractiveText::new(SharedString::from(format!("text-{key}")), styled)
            .on_click(ranges, move |index, _, cx| {
                if let Some((_, url)) = links.links.get(index) {
                    open_link(url, cx);
                }
            })
            .into_any_element()
    };
    match backgrounds {
        Some(backgrounds) => div()
            .relative()
            .child(backgrounds)
            .child(text)
            .into_any_element(),
        None => text,
    }
}

/// Rounded backgrounds behind inline code, one per line a code run covers. Drawn under the text,
/// which is laid out by the time this paints.
fn code_backgrounds(layout: TextLayout, ranges: Vec<Range<usize>>) -> AnyElement {
    canvas(
        |_, _, _| (),
        move |_, (), window, cx| {
            let colors = Colors::active(cx);
            let (color, edge) = (colors.surface, colors.subtle_border);
            let line_height = layout.line_height();
            let inset = line_height * 0.1;
            let text = layout.text();
            for range in &ranges {
                let mut points = range
                    .clone()
                    .chain([range.end])
                    .filter(|&index| text.is_char_boundary(index))
                    .filter_map(|index| layout.position_for_index(index));
                let Some(mut start) = points.next() else {
                    continue;
                };
                let mut end = start;
                for at in points {
                    if at.y != start.y {
                        paint_code_background(
                            window,
                            start,
                            end.x,
                            line_height,
                            inset,
                            (color, edge),
                        );
                        start = at;
                    }
                    end = at;
                }
                paint_code_background(window, start, end.x, line_height, inset, (color, edge));
            }
        },
    )
    .absolute()
    .size_full()
    .into_any_element()
}

fn paint_code_background(
    window: &mut Window,
    start: gpui::Point<gpui::Pixels>,
    end_x: gpui::Pixels,
    line_height: gpui::Pixels,
    inset: gpui::Pixels,
    (color, edge): (gpui::Rgba, gpui::Rgba),
) {
    if end_x <= start.x {
        return;
    }
    let bounds = gpui::Bounds::from_corners(
        point(start.x, start.y + inset),
        point(end_x, start.y + line_height - inset),
    );
    window.paint_quad(
        fill(bounds, color)
            .corner_radii(px(4.))
            .border_widths(px(1.))
            .border_color(edge),
    );
}

fn style_of(colors: &Colors, run: &Run) -> HighlightStyle {
    let color = if run.link.is_some() {
        Some(colors.accent.into())
    } else if run.code {
        Some(colors.code(Capture::String).into())
    } else {
        None
    };
    HighlightStyle {
        color,
        font_weight: run.bold.then_some(FontWeight::BOLD),
        font_style: run.italic.then_some(FontStyle::Italic),
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
    fn a_text_is_parsed_once_and_a_changed_text_again() {
        let first = parsed_markdown("**one**");

        assert!(Rc::ptr_eq(&first, &parsed_markdown("**one**")));
        let changed = parsed_markdown("**two**");
        assert!(!Rc::ptr_eq(&first, &changed));
        assert_eq!(changed.markdown, Markdown::parse("**two**"));
    }

    fn prepared(source: &str) -> Vec<PreparedBlock> {
        PreparedMarkdown::of(Markdown::parse(source)).blocks
    }

    #[test]
    fn a_heading_is_prepared_bold_and_a_paragraph_as_it_is() {
        let [heading, paragraph] = &prepared("## Title\n\nplain *soft*")[..] else {
            panic!("expected a heading and a paragraph");
        };

        let (
            PreparedBlock::Text { text: heading, .. },
            PreparedBlock::Text {
                text: paragraph, ..
            },
        ) = (heading, paragraph)
        else {
            panic!("expected text blocks");
        };
        assert_eq!(heading.text.as_ref(), "Title");
        assert!(heading.styled.iter().all(|(_, run)| run.bold));
        assert_eq!(paragraph.text.as_ref(), "plain soft");
        assert!(paragraph.styled.iter().all(|(_, run)| !run.bold));
    }

    #[test]
    fn a_suggestion_is_prepared_with_the_lines_it_adds() {
        let blocks = prepared("```suggestion\nnew a\nnew b\n```");

        let [PreparedBlock::Suggestion { added, .. }] = &blocks[..] else {
            panic!("expected a suggestion");
        };
        assert_eq!(
            added.iter().map(SharedString::as_ref).collect::<Vec<_>>(),
            ["new a", "new b"]
        );
    }

    #[test]
    fn a_code_fence_is_prepared_a_line_at_a_time() {
        let blocks = prepared("```php\n$a = 1;\n$b = 2;\n```");

        let [PreparedBlock::Code { lines }] = &blocks[..] else {
            panic!("expected a code block");
        };
        assert_eq!(
            lines
                .iter()
                .map(|line| line.text.as_ref())
                .collect::<Vec<_>>(),
            ["$a = 1;", "$b = 2;"]
        );
    }

    #[test]
    fn a_diff_fence_is_prepared_with_the_kind_of_each_line() {
        let blocks = prepared("```diff\n same\n-old\n+new\n```");

        let [PreparedBlock::Code { lines }] = &blocks[..] else {
            panic!("expected a code block");
        };
        assert_eq!(
            lines.iter().map(|line| line.kind).collect::<Vec<_>>(),
            [FenceLine::Plain, FenceLine::Removed, FenceLine::Added]
        );
    }

    #[test]
    fn a_changed_title_is_not_drawn_from_the_old_parse() {
        assert_eq!(
            parsed_inline("old `title`").text.as_ref(),
            "old \u{2004}title\u{2004}"
        );
        assert_eq!(
            parsed_inline("new `title`").text.as_ref(),
            "new \u{2004}title\u{2004}"
        );
    }

    #[test]
    fn a_parsed_text_takes_its_colours_from_the_theme_it_is_drawn_in() {
        use super::super::palette::Appearance;
        let text = parsed_inline("see [docs](https://x.test)");

        let light = text.highlights(&Colors::of(Appearance::Light));
        let dark = text.highlights(&Colors::of(Appearance::Dark));

        assert_eq!(
            light.first().and_then(|(_, style)| style.color),
            Some(Colors::of(Appearance::Light).accent.into())
        );
        assert_eq!(
            dark.first().and_then(|(_, style)| style.color),
            Some(Colors::of(Appearance::Dark).accent.into())
        );
    }

    #[test]
    fn tour_text_keeps_its_bold_and_code_and_drops_the_markers() {
        let runs = runs_of(&Markdown::parse("**Too little stock** joins `Reason`").blocks);

        let text: String = runs.iter().map(|run| run.text.as_str()).collect();
        assert_eq!(text, "Too little stock joins Reason");
        assert!(runs[0].bold);
        assert!(runs.last().is_some_and(|run| run.code));
    }
}
