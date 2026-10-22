use std::ops::Range;
use std::rc::Rc;

use gpui::prelude::FluentBuilder;
use gpui::{
    App, Div, ElementId, HighlightStyle, InteractiveElement, IntoElement, MouseButton,
    ParentElement, RenderOnce, SharedString, Stateful, StatefulInteractiveElement, Styled,
    StyledText, Window, div, px,
};

use crate::viewer::{Capture, DiffLine, Edge, GitHubLink, LineKind, LineLocation, MoreContext};

use super::palette::{CODE_FONT, Colors};

pub type OnMoreContext = Rc<dyn Fn(&MoreContext, &mut Window, &mut App)>;

/// What the mouse does on a row. The release is handled above the rows, wherever the mouse is let go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowGesture {
    /// Down on the row's comment button: the drag starts here.
    Press,
    /// The mouse is over the row: a drag in progress reaches it.
    Over,
}

pub type OnImageComment = Rc<dyn Fn(&mut Window, &mut App)>;

/// Opens a line of the head in the reviewer's IDE.
pub type OnOpenInIde = Rc<dyn Fn(&LineLocation, &mut App)>;

pub type OnRowGesture = Rc<dyn Fn(RowGesture, &LineLocation, &mut Window, &mut App)>;

// Our own diff view (Zed's is GPL), drawn as rows of a piece box: the header band, one row per line
// grouped in hunks like on GitHub, the bands between hunks, and the box's bottom edge. Each row is one
// item of the screen's list, so only the rows on screen are drawn.

/// What a `DiffRow` draws, prepared once per change of the review, so a frame copies no text.
pub struct DiffRowLine {
    pub location: LineLocation,
    pub kind: LineKind,
    /// Base and head numbers and the sign, padded so one monospace text fills the gutter.
    gutter: SharedString,
    code: SharedString,
    /// Cut to char boundaries already; coloured when drawn, so a theme switch needs no rebuild.
    highlights: Vec<(Range<usize>, Capture)>,
    github_link: GitHubLink,
    /// `None` for removed lines: the files at head do not have them.
    open_at: Option<LineLocation>,
    /// Rows outside GitHub's hunks, and rows of the change since the read, take no comment.
    takes_comment: bool,
}

impl DiffRowLine {
    pub fn of(line: DiffLine) -> Self {
        let number = line.location.line.to_string();
        let (base_number, head_number, sign) = match line.kind {
            LineKind::Removed => (number, String::new(), '-'),
            LineKind::Added => (String::new(), number, '+'),
            LineKind::Context { base_line, .. } => (base_line.to_string(), number, ' '),
        };
        let highlights = line
            .spans
            .iter()
            .filter(|span| {
                line.text.is_char_boundary(span.range.start)
                    && line.text.is_char_boundary(span.range.end)
            })
            .map(|span| (span.range.clone(), span.capture))
            .collect();
        Self {
            takes_comment: line.takes_comment(),
            open_at: line.open_at().cloned(),
            gutter: format!("{base_number:>5} {head_number:>5} {sign} ").into(),
            code: line.text.into(),
            highlights,
            github_link: line.github_link,
            location: line.location,
            kind: line.kind,
        }
    }
}

/// One line of a piece: a gutter that opens the line on GitHub, the "+" that starts a comment, and the
/// code as one styled text.
#[derive(IntoElement)]
pub struct DiffRow {
    line: Rc<DiffRowLine>,
    is_selected: bool,
    /// Without it the row takes no comment.
    on_gesture: Option<OnRowGesture>,
    open_in_ide: Option<OnOpenInIde>,
}

impl DiffRow {
    pub fn new(line: Rc<DiffRowLine>) -> Self {
        Self {
            line,
            is_selected: false,
            on_gesture: None,
            open_in_ide: None,
        }
    }

    /// Cmd-click on the gutter opens the line in the IDE; a plain click opens it on GitHub.
    pub fn with_open_in_ide(mut self, open_in_ide: Option<OnOpenInIde>) -> Self {
        self.open_in_ide = open_in_ide;
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
        let highlights = line.highlights.iter().map(|(range, capture)| {
            let style = HighlightStyle {
                color: Some(colors.code(*capture).into()),
                ..Default::default()
            };
            (range.clone(), style)
        });
        let code = StyledText::new(line.code.clone()).with_highlights(highlights);
        let link = line.github_link.clone();
        let ide_at = line.open_at.clone();
        let open_in_ide = self.open_in_ide.clone();
        let gutter = div()
            .id(SharedString::from(link.to_string()))
            .flex_none()
            .cursor_pointer()
            .text_color(colors.muted_text)
            .on_click(move |event, _, cx| {
                match (event.modifiers().secondary(), &open_in_ide, &ide_at) {
                    (true, Some(open_in_ide), Some(at)) => open_in_ide(at, cx),
                    _ => cx.open_url(link.as_str()),
                }
            })
            .child(line.gutter.clone());
        let comment_button = div().w(px(16.)).flex_none();
        let comment_button = match self.on_gesture.clone().filter(|_| line.takes_comment) {
            Some(on_gesture) => {
                let at = location.clone();
                comment_button
                    .text_color(colors.muted_text)
                    .opacity(0.)
                    .group_hover("diff-row", |button| button.opacity(1.))
                    .cursor_pointer()
                    .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                        on_gesture(RowGesture::Press, &at, window, cx)
                    })
                    .child("+")
                    .into_any_element()
            }
            None => comment_button.into_any_element(),
        };
        let row = piece_row(colors)
            .group("diff-row")
            .flex()
            .overflow_hidden()
            .whitespace_nowrap()
            .font_family(CODE_FONT)
            .text_sm()
            .when_some(self.on_gesture, |row, on_gesture| {
                row.on_mouse_move(move |_, window, cx| {
                    on_gesture(RowGesture::Over, &location, window, cx)
                })
            });
        let row = match line.kind {
            _ if self.is_selected => row.bg(colors.current),
            LineKind::Removed => row.bg(colors.removed_line),
            LineKind::Added => row.bg(colors.added_line),
            LineKind::Context { .. } => row,
        };
        row.child(gutter).child(comment_button).child(code)
    }
}

/// Every row of a piece sits between the box's side borders.
pub fn piece_row(colors: Colors) -> Div {
    div().w_full().border_x_1().border_color(colors.border)
}

/// The top of a piece box: the piece's name, its re-pin label, and its buttons (open in IDE, ask).
#[derive(IntoElement)]
pub struct PieceHeader {
    piece: SharedString,
    repin_label: Option<&'static str>,
    buttons: Vec<Stateful<Div>>,
}

impl PieceHeader {
    pub fn new(
        piece: SharedString,
        repin_label: Option<&'static str>,
        buttons: Vec<Stateful<Div>>,
    ) -> Self {
        Self {
            piece,
            repin_label,
            buttons,
        }
    }
}

impl RenderOnce for PieceHeader {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let colors = Colors::active(cx);
        div()
            .w_full()
            .flex()
            .items_center()
            .px_2()
            .gap_2()
            .bg(colors.sidebar)
            .border_1()
            .rounded_t_md()
            .border_color(colors.border)
            .text_sm()
            .child(
                div()
                    .py_1()
                    .overflow_hidden()
                    .font_family(CODE_FONT)
                    .child(self.piece),
            )
            .children(
                self.repin_label
                    .map(|label| div().text_color(colors.muted_text).child(label)),
            )
            .child(div().flex_1())
            .children(self.buttons)
    }
}

/// The bottom edge of a piece box, and the space before the next one.
pub fn piece_end(colors: Colors) -> impl IntoElement {
    div().pb_3().child(
        div()
            .h_1()
            .border_x_1()
            .border_b_1()
            .rounded_b_md()
            .border_color(colors.border),
    )
}

pub fn no_change_since_read_row(colors: Colors) -> impl IntoElement {
    piece_row(colors)
        .px_2()
        .py_1()
        .text_sm()
        .text_color(colors.muted_text)
        .child("No change since you read it")
}

pub fn only_whitespace_row(colors: Colors) -> impl IntoElement {
    piece_row(colors)
        .px_2()
        .py_1()
        .text_sm()
        .text_color(colors.muted_text)
        .child("Only whitespace changed.")
}

/// A binary file has no lines to show, so it gets one row that sends the reviewer to GitHub.
pub fn binary_file_row(colors: Colors, link: GitHubLink) -> impl IntoElement {
    piece_row(colors).child(
        div()
            .id(SharedString::from(link.to_string()))
            .px_2()
            .py_1()
            .text_sm()
            .cursor_pointer()
            .text_color(colors.muted_text)
            .on_click(move |_, _, cx| cx.open_url(link.as_str()))
            .child("Binary file, open it on GitHub"),
    )
}

/// The band between hunks, like GitHub's `@@` line; clicking it shows more unchanged lines.
pub fn hunk_gap(
    colors: Colors,
    id: impl Into<ElementId>,
    more: Option<MoreContext>,
    on_more_context: OnMoreContext,
) -> impl IntoElement {
    let band = div()
        .id(id)
        .px_2()
        .bg(colors.sidebar)
        .text_xs()
        .text_color(colors.muted_text);
    let band = match more {
        None => band.child("⋯"),
        Some(more) => {
            let label = match more.edge {
                Edge::Above => "↑ Show more lines",
                Edge::Below => "↓ Show more lines",
            };
            band.cursor_pointer()
                .on_click(move |_, window, cx| on_more_context(&more, window, cx))
                .child(label)
        }
    };
    piece_row(colors).child(band)
}
