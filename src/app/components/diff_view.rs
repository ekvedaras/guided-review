use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;

use gpui::prelude::FluentBuilder;
use gpui::{
    App, Div, FontWeight, HighlightStyle, InteractiveElement, IntoElement, MouseButton,
    ParentElement, Pixels, RenderOnce, Rgba, SharedString, Stateful, StatefulInteractiveElement,
    Styled, StyledText, TextLayout, Window, div, px,
};

use crate::app::code_links::{open_code, open_code_hint, open_code_label};
use crate::app::components::tooltip::tooltip;
use crate::viewer::{
    BlockChange, BoxEdges, CodeLink, HiddenLines, LineKind, LineLocation, MoreContext, PieceLabel,
    ShownLine,
};

use super::button::{ButtonKind, ButtonSize, icon_button};
use super::chip::chip;
use super::icon::{Icon, sized_icon};
use super::palette::{CARD_RADIUS, CODE_FONT, Colors, Tone, inside_edge};

/// What a gap row asks for: a few more lines from one side, or every hidden line.
pub enum GapAsk<'a> {
    More(&'a MoreContext),
    All(&'a HiddenLines),
}

pub type OnGapAsk = Rc<dyn Fn(GapAsk, &mut Window, &mut App)>;

/// What the mouse does on a row. The release is handled above the rows, wherever the mouse is let go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowGesture {
    /// Down on the row's comment button: the drag starts here.
    Press,
    /// The mouse is over the row: a drag in progress reaches it.
    Over,
}

/// What the mouse does on a row's code text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodeGesture {
    /// Down once: a text selection starts here.
    Press,
    /// Down twice in a row: the word here is selected.
    PressTwice,
    /// A move with the button held carries the selection on.
    Over,
}

/// A sideways scroll on a row: how far the row's file box should move its code, in pixels.
pub type OnScrollX = Rc<dyn Fn(Pixels, &mut Window, &mut App)>;

pub type OnImageComment = Rc<dyn Fn(&mut Window, &mut App)>;

pub type OnRowGesture = Rc<dyn Fn(RowGesture, &LineLocation, &mut Window, &mut App)>;

/// The mouse on a row's code text: down starts a text selection there, a move with the button held
/// carries it on. Spots are byte offsets in the row's code.
pub type OnCodeGesture = Rc<dyn Fn(CodeGesture, usize, &mut Window, &mut App)>;

/// How a row takes part in the text selection: what of it is selected, and where its mouse goes.
pub struct CodeSelectionHooks {
    pub selected: Option<Range<usize>>,
    pub on_gesture: OnCodeGesture,
}

// Our own diff view (Zed's is GPL), drawn as rows of a piece box: the header band, one row per line
// grouped in hunks like on GitHub, the bands between hunks, and the box's bottom edge. Each row is one
// item of the screen's list, so only the rows on screen are drawn.

/// One line of a piece: a gutter that opens the line in the IDE or on GitHub, the "+" that starts a comment, and the
/// code as one styled text.
#[derive(IntoElement)]
pub struct DiffRow {
    line: Arc<ShownLine>,
    is_selected: bool,
    /// Without it the row takes no comment.
    on_gesture: Option<OnRowGesture>,
    code_selection: Option<CodeSelectionHooks>,
    /// How far the code is moved left; all rows of a file box share it, so they scroll together.
    scroll_x: Pixels,
    on_scroll_x: Option<OnScrollX>,
    edges: BoxEdges,
}

impl DiffRow {
    pub fn new(line: Arc<ShownLine>) -> Self {
        Self {
            line,
            is_selected: false,
            on_gesture: None,
            code_selection: None,
            scroll_x: Pixels::ZERO,
            on_scroll_x: None,
            edges: BoxEdges::default(),
        }
    }

    pub fn with_edges(mut self, edges: BoxEdges) -> Self {
        self.edges = edges;
        self
    }

    /// Only for a row that does not wrap: a wrapped row has nothing hidden to the side.
    pub fn scrolled_sideways(mut self, scroll_x: Pixels, on_scroll_x: OnScrollX) -> Self {
        if !self.line.wraps {
            self.scroll_x = scroll_x;
            self.on_scroll_x = Some(on_scroll_x);
        }
        self
    }

    pub fn with_code_selection(mut self, hooks: CodeSelectionHooks) -> Self {
        self.code_selection = Some(hooks);
        self
    }

    pub fn selected(mut self, is_selected: bool) -> Self {
        self.is_selected = is_selected;
        self
    }

    pub fn with_gestures(mut self, on_gesture: OnRowGesture) -> Self {
        self.on_gesture = Some(on_gesture);
        self
    }
}

impl RenderOnce for DiffRow {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let line = self.line;
        let location = line.location.clone();
        let colors = Colors::active(cx);
        let highlights: Vec<_> = line
            .highlights
            .iter()
            .map(|(range, capture)| {
                let style = HighlightStyle {
                    color: Some(colors.code(*capture).into()),
                    ..Default::default()
                };
                (range.clone(), style)
            })
            .collect();
        let word_background = match line.kind {
            LineKind::Removed => Some(colors.removed_word),
            LineKind::Added => Some(colors.added_word),
            LineKind::Context { .. } => None,
        };
        let highlights = match word_background {
            // Plain text in a changed word: syntax colours do not all read on its stronger ground.
            Some(background) => {
                let word = HighlightStyle {
                    color: Some(colors.text.into()),
                    background_color: Some(background.into()),
                    ..Default::default()
                };
                line.changed_words
                    .iter()
                    .fold(highlights, |highlights, range| {
                        laid_over(&highlights, range.clone(), word)
                    })
            }
            None => highlights,
        };
        let selected = self
            .code_selection
            .as_ref()
            .and_then(|hooks| hooks.selected.clone());
        let highlights = match selected {
            Some(selected) => with_selection(&highlights, selected, colors.text_selection),
            None => highlights,
        };
        let code = StyledText::new(SharedString::from(&line.code)).with_highlights(highlights);
        let code_layout = code.layout().clone();
        let code = div()
            .flex_1()
            .min_w_0()
            .overflow_hidden()
            .child(div().relative().left(-self.scroll_x).child(code));
        let code = match self.code_selection.map(|hooks| hooks.on_gesture) {
            Some(on_gesture) => {
                let (on_press, on_move) = (Rc::clone(&on_gesture), on_gesture);
                let (press_layout, move_layout) = (code_layout.clone(), code_layout);
                code.cursor_text()
                    .on_mouse_down(MouseButton::Left, move |event, window, cx| {
                        let byte = byte_at(&press_layout, event.position);
                        let gesture = match event.click_count {
                            2 => CodeGesture::PressTwice,
                            _ => CodeGesture::Press,
                        };
                        on_press(gesture, byte, window, cx);
                    })
                    .on_mouse_move(move |event, window, cx| {
                        if event.pressed_button == Some(MouseButton::Left) {
                            let byte = byte_at(&move_layout, event.position);
                            on_move(CodeGesture::Over, byte, window, cx);
                        }
                    })
            }
            None => code,
        };
        let link = line.code_link.clone();
        let gutter_background = match line.kind {
            _ if self.is_selected => colors.current_border,
            LineKind::Removed => colors.removed_gutter,
            LineKind::Added => colors.added_gutter,
            LineKind::Context { .. } => colors.background,
        };
        let edges = self.edges;
        let gutter = div()
            .id(SharedString::from(format!(
                "gutter-{}-{:?}-{}",
                location.path, location.side, location.line
            )))
            .flex_none()
            .cursor_pointer()
            .bg(gutter_background)
            .text_color(if self.is_selected {
                colors.text
            } else {
                colors.muted_text
            })
            .hover(|gutter| gutter.text_color(colors.accent))
            .tooltip(tooltip({
                let link = link.clone();
                move |cx| open_code_hint(&link, cx)
            }))
            .on_click(move |_, _, cx| open_code(&link, cx))
            .whitespace_nowrap()
            .when(edges.opens, |gutter| {
                gutter.rounded_tl(px(INSIDE_EDGE_RADIUS))
            })
            .when(edges.closes, |gutter| {
                gutter.rounded_bl(px(INSIDE_EDGE_RADIUS))
            })
            .child(SharedString::from(&line.gutter));
        let comment_button = div().w(px(16.)).flex_none();
        let comment_button = match self
            .on_gesture
            .clone()
            .filter(|_| line.offers_comment_button)
        {
            Some(on_gesture) => {
                let at = location.clone();
                comment_button
                    .self_stretch()
                    .flex()
                    .items_center()
                    .justify_center()
                    .opacity(0.)
                    .group_hover("diff-row", |button| button.opacity(1.))
                    .cursor_pointer()
                    .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                        on_gesture(RowGesture::Press, &at, window, cx);
                    })
                    .child(sized_icon(Icon::Plus, colors.accent, px(12.)))
                    .into_any_element()
            }
            None => comment_button.into_any_element(),
        };
        let row = box_row(&colors, edges)
            .group("diff-row")
            .flex()
            .overflow_hidden()
            .when(!line.wraps, Styled::whitespace_nowrap)
            .font_family(CODE_FONT)
            .text_sm()
            .when_some(self.on_scroll_x, |row, on_scroll_x| {
                row.on_scroll(move |event, window, cx| {
                    let delta = event.delta.pixel_delta(window.line_height());
                    if delta.x.abs() > delta.y.abs() {
                        on_scroll_x(-delta.x, window, cx);
                        cx.stop_propagation();
                    }
                })
            })
            .when_some(self.on_gesture, |row, on_gesture| {
                row.on_mouse_move(move |_, window, cx| {
                    on_gesture(RowGesture::Over, &location, window, cx);
                })
            });
        let fill = round_inside(edges, div().flex_1().min_w_0().flex());
        let fill = match line.kind {
            _ if self.is_selected => fill.bg(colors.current),
            LineKind::Removed => fill.bg(colors.removed_line),
            LineKind::Added => fill.bg(colors.added_line),
            LineKind::Context { .. } => fill,
        };
        row.child(fill.child(gutter).child(comment_button).child(code))
    }
}

pub(super) fn byte_at(layout: &TextLayout, position: gpui::Point<Pixels>) -> usize {
    layout
        .index_for_position(position)
        .unwrap_or_else(|closest| closest)
}

/// Lays the selection's background over the syntax colours, which must stay sorted and apart.
pub(super) fn with_selection(
    highlights: &[(Range<usize>, HighlightStyle)],
    selected: Range<usize>,
    background: Rgba,
) -> Vec<(Range<usize>, HighlightStyle)> {
    let selection_style = HighlightStyle {
        background_color: Some(background.into()),
        ..Default::default()
    };
    laid_over(highlights, selected, selection_style)
}

/// Lays `style` over the styles in `range`; the styles must stay sorted and apart.
fn laid_over(
    highlights: &[(Range<usize>, HighlightStyle)],
    selected: Range<usize>,
    selection_style: HighlightStyle,
) -> Vec<(Range<usize>, HighlightStyle)> {
    let mut cut: Vec<usize> = vec![selected.start, selected.end];
    for (range, _) in highlights {
        cut.push(range.start);
        cut.push(range.end);
    }
    cut.sort_unstable();
    cut.dedup();
    cut.windows(2)
        .filter_map(|bounds| {
            let &[start, end] = bounds else {
                return None;
            };
            let piece = start..end;
            let syntax = highlights
                .iter()
                .find(|(range, _)| range.start <= piece.start && piece.end <= range.end)
                .map(|(_, style)| *style);
            let is_selected = selected.start <= piece.start && piece.end <= selected.end;
            let style = match (syntax, is_selected) {
                (Some(style), true) => style.highlight(selection_style),
                (Some(style), false) => style,
                (None, true) => selection_style,
                (None, false) => return None,
            };
            Some((piece, style))
        })
        .collect()
}

const INSIDE_EDGE_RADIUS: f32 = inside_edge(CARD_RADIUS);

pub fn round_inside(edges: BoxEdges, layer: Div) -> Div {
    layer
        .when(edges.opens, |layer| layer.rounded_t(px(INSIDE_EDGE_RADIUS)))
        .when(edges.closes, |layer| {
            layer.rounded_b(px(INSIDE_EDGE_RADIUS))
        })
}

/// A row of a piece box, drawing the box's top or bottom edge where it opens or closes there.
pub fn box_row(colors: &Colors, edges: BoxEdges) -> Div {
    piece_row(colors)
        .when(edges.opens, |row| {
            row.border_t_1().rounded_t(px(CARD_RADIUS))
        })
        .when(edges.closes, |row| {
            row.border_b_1()
                .rounded_b(px(CARD_RADIUS))
                .shadow(colors.card_shadow())
        })
}

/// What hangs under a line, like a thread, sits between the box's two parts, as wide as the box.
pub fn hung_row() -> Div {
    div().w_full().px(px(1.))
}

/// Every row of a piece sits between the box's side borders.
pub fn piece_row(colors: &Colors) -> Div {
    div()
        .w_full()
        .border_x_1()
        .border_color(colors.subtle_border)
}

pub const PIECE_HEADER_HEIGHT: Pixels = px(34.);

/// The top of a piece box: the piece's name, its re-pin label, and its buttons (open the code, ask).
#[derive(IntoElement)]
pub struct PieceHeader {
    piece: PieceLabel,
    repin_label: Option<&'static str>,
    buttons: Vec<Stateful<Div>>,
    fold_toggle: Option<Stateful<Div>>,
    closes_box: bool,
}

impl PieceHeader {
    pub fn new(
        piece: PieceLabel,
        repin_label: Option<&'static str>,
        buttons: Vec<Stateful<Div>>,
    ) -> Self {
        Self {
            piece,
            repin_label,
            buttons,
            fold_toggle: None,
            closes_box: false,
        }
    }

    /// Nothing follows it in its box: it rounds the box's bottom corners too.
    pub fn closing_its_box(mut self, closes_box: bool) -> Self {
        self.closes_box = closes_box;
        self
    }

    /// A chevron before the name that opens or folds the box.
    pub fn with_fold_toggle(mut self, toggle: Stateful<Div>) -> Self {
        self.fold_toggle = Some(toggle);
        self
    }
}

impl RenderOnce for PieceHeader {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let colors = Colors::active(cx);
        let piece = self.piece;
        let symbol = piece.symbol.map(|symbol| {
            div()
                .flex_none()
                .text_color(colors.accent)
                .child(SharedString::from(symbol))
        });
        div()
            .w_full()
            .h(PIECE_HEADER_HEIGHT)
            .flex()
            .items_center()
            .px_3()
            .gap_2()
            .bg(colors.header_band())
            .border_1()
            .rounded_t(px(CARD_RADIUS))
            .when(self.closes_box, |header| {
                header
                    .rounded_b(px(CARD_RADIUS))
                    .shadow(colors.card_shadow())
            })
            .border_color(colors.subtle_border)
            .text_sm()
            .children(self.fold_toggle)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap_2()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .font_family(CODE_FONT)
                    .child(
                        div()
                            .flex()
                            .min_w_0()
                            .overflow_hidden()
                            .child(
                                div()
                                    .min_w_0()
                                    .text_color(colors.muted_text)
                                    .text_ellipsis_start()
                                    .overflow_hidden()
                                    .child(SharedString::from(piece.folder)),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(SharedString::from(piece.file_name)),
                            ),
                    )
                    .children(symbol),
            )
            .children(
                self.repin_label
                    .map(|label| chip(&colors, Tone::Warning).child(label)),
            )
            .children(self.buttons)
    }
}

/// Inside a box of several pieces of one file: a thin dim line where the next piece starts.
/// Its buttons show only under the mouse, so the code reads as one file.
#[derive(IntoElement)]
pub struct PieceMarker {
    piece: PieceLabel,
    repin_label: Option<&'static str>,
    buttons: Vec<Stateful<Div>>,
    edges: BoxEdges,
}

impl PieceMarker {
    pub fn new(
        piece: PieceLabel,
        repin_label: Option<&'static str>,
        buttons: Vec<Stateful<Div>>,
    ) -> Self {
        Self {
            piece,
            repin_label,
            buttons,
            edges: BoxEdges::default(),
        }
    }

    pub fn with_edges(mut self, edges: BoxEdges) -> Self {
        self.edges = edges;
        self
    }
}

impl RenderOnce for PieceMarker {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let colors = Colors::active(cx);
        let name = self.piece.shown_name().map(SharedString::from);
        box_row(&colors, self.edges)
            .group("piece-marker")
            .h(px(24.))
            .flex()
            .items_center()
            .gap_2()
            .px_3()
            .border_t_1()
            .text_xs()
            .font_family(CODE_FONT)
            .text_color(colors.muted_text)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .children(name),
            )
            .children(
                self.repin_label
                    .map(|label| chip(&colors, Tone::Warning).child(label)),
            )
            .child(
                div()
                    .flex()
                    .flex_none()
                    .gap_2()
                    .opacity(0.)
                    .group_hover("piece-marker", |buttons| buttons.opacity(1.))
                    .children(self.buttons),
            )
    }
}

/// The space under a piece box, before the next one.
pub fn piece_end() -> gpui::AnyElement {
    div().pb_4().into_any_element()
}

pub fn no_change_since_read_row(colors: &Colors, edges: BoxEdges) -> impl IntoElement {
    box_row(colors, edges)
        .px_2()
        .py_1()
        .text_sm()
        .text_color(colors.muted_text)
        .child("No change since you read it")
}

pub fn only_whitespace_row(colors: &Colors, edges: BoxEdges) -> impl IntoElement {
    box_row(colors, edges)
        .px_2()
        .py_1()
        .text_sm()
        .text_color(colors.muted_text)
        .child("Only whitespace changed.")
}

/// A block of a rendered Markdown file, with a bar on its left saying what the PR did to it. A
/// removed block is dimmed rather than struck through, so it stays readable.
pub fn rendered_block_row(
    colors: &Colors,
    edges: BoxEdges,
    change: Option<BlockChange>,
    block: impl IntoElement,
) -> impl IntoElement {
    let bar: gpui::Hsla = match change {
        Some(BlockChange::Added) => colors.success.into(),
        Some(BlockChange::Changed) => colors.warning.into(),
        Some(BlockChange::Removed) => colors.danger.into(),
        None => gpui::transparent_black(),
    };
    box_row(colors, edges).child(
        div()
            .flex()
            .px_2()
            .gap_3()
            .child(div().flex_none().w(px(3.)).rounded(px(1.5)).bg(bar))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .py_1()
                    .when(change == Some(BlockChange::Removed), |text| {
                        text.opacity(0.5)
                    })
                    .child(block),
            ),
    )
}

pub fn rendered_file_waiting_row(colors: &Colors, edges: BoxEdges) -> impl IntoElement {
    box_row(colors, edges)
        .px_2()
        .py_1()
        .text_sm()
        .text_color(colors.muted_text)
        .child("Reading the file…")
}

/// A binary file has no lines to show, so it gets one row that opens the file.
pub fn binary_file_row(
    colors: &Colors,
    edges: BoxEdges,
    id: SharedString,
    link: CodeLink,
    cx: &App,
) -> impl IntoElement {
    let label = format!("Binary file. {}", open_code_label(&link, cx));
    box_row(colors, edges).child(
        div()
            .id(id)
            .px_2()
            .py_1()
            .text_sm()
            .cursor_pointer()
            .text_color(colors.muted_text)
            .tooltip(tooltip({
                let link = link.clone();
                move |cx| open_code_hint(&link, cx)
            }))
            .on_click(move |_, _, cx| open_code(&link, cx))
            .child(label),
    )
}

/// The band between hunks, like GitHub's `@@` line; clicking it shows more unchanged lines.
/// The unchanged lines between two changes (or above the first, or under the last) as one row: how
/// many, and small arrows that bring more in from each side, or all of them at once.
pub fn hunk_gap(
    colors: &Colors,
    edges: BoxEdges,
    id: SharedString,
    gap: &HiddenLines,
    label: SharedString,
    on_ask: &OnGapAsk,
) -> impl IntoElement {
    let is_top = gap.after_above.is_none();
    let is_end = gap.before_below.is_none();
    let arrow = |more: &MoreContext, icon: Icon, name: &'static str| {
        let (more, on_ask) = (more.clone(), Rc::clone(on_ask));
        icon_button(
            colors,
            SharedString::from(format!("{id}-{name}")),
            ButtonKind::Ghost,
            ButtonSize::Small,
            icon,
            name,
        )
        .h(px(22.))
        .w(px(22.))
        .on_click(move |_, window, cx| on_ask(GapAsk::More(&more), window, cx))
    };
    let down = gap.after_above.as_ref().map(|more| {
        arrow(
            more,
            Icon::ArrowDown,
            if is_end {
                "Show more lines below"
            } else {
                "Show more lines under the change above"
            },
        )
    });
    let up = gap.before_below.as_ref().map(|more| {
        arrow(
            more,
            Icon::ArrowUp,
            if is_top {
                "Show more lines above"
            } else {
                "Show more lines over the change below"
            },
        )
    });
    let all = gap.count.filter(|_| !is_top || !is_end).and_then(|_| {
        (down.is_some() || up.is_some()).then(|| {
            let (gap, on_ask) = (gap.clone(), Rc::clone(on_ask));
            icon_button(
                colors,
                SharedString::from(format!("{id}-all")),
                ButtonKind::Ghost,
                ButtonSize::Small,
                Icon::ChevronsUpDown,
                "Show every hidden line",
            )
            .h(px(22.))
            .w(px(22.))
            .on_click(move |_, window, cx| on_ask(GapAsk::All(&gap), window, cx))
        })
    });
    let has_asks = down.is_some() || up.is_some();
    let band = round_inside(edges, div())
        .id(id)
        .h(px(28.))
        .px_2()
        .flex()
        .items_center()
        .gap_1()
        .bg(colors.surface)
        .text_xs()
        .text_color(colors.muted_text)
        .children(up)
        .children(down)
        .children(all)
        .when(!has_asks, |band| {
            band.child(sized_icon(Icon::Ellipsis, colors.muted_text, px(12.)))
        })
        .child(div().pl_1().child(label));
    box_row(colors, edges).child(band)
}
