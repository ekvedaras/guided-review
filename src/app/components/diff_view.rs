use std::rc::Rc;

use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, App, ElementId, HighlightStyle, InteractiveElement, IntoElement, MouseButton,
    ParentElement, RenderOnce, SharedString, StatefulInteractiveElement, Styled, StyledText,
    Window, div, px,
};

use crate::repin::RepinStatus;
use crate::viewer::{
    DiffHunk, DiffLine, Edge, GitHubLink, LineKind, LineLocation, MoreContext, PieceDiff,
    SelectedLines,
};

use super::palette::{CODE_FONT, Palette};

/// Opens a line of the head in the reviewer's IDE.
pub type OnOpenInIde = Rc<dyn Fn(&LineLocation, &mut App)>;

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

pub type OnRowGesture = Rc<dyn Fn(RowGesture, &LineLocation, &mut Window, &mut App)>;

/// The rows a drag selects and how to tell the layout what the mouse does on rows.
#[derive(Clone)]
pub struct RowSelecting {
    pub selected: Option<SelectedLines>,
    pub on_gesture: OnRowGesture,
}

/// Our own diff view (Zed's is GPL). One row per line, grouped in hunks like on GitHub, and each row keeps
/// its `LineLocation` so comments can hang off any row later. A click on a line number opens that line on GitHub.
#[derive(IntoElement)]
pub struct DiffView {
    piece: PieceDiff,
    on_more_context: OnMoreContext,
    /// Shown right under the row at their location.
    threads: Vec<(LineLocation, AnyElement)>,
    is_change_since_read: bool,
    selecting: Option<RowSelecting>,
    open_in_ide: Option<OnOpenInIde>,
    /// The picture of an image file, shown above its text diff when it has one (an SVG).
    image_row: Option<AnyElement>,
}

impl DiffView {
    pub fn new(piece: PieceDiff, on_more_context: OnMoreContext) -> Self {
        Self {
            piece,
            on_more_context,
            threads: Vec::new(),
            is_change_since_read: false,
            selecting: None,
            open_in_ide: None,
            image_row: None,
        }
    }

    /// Cmd-click on a line number opens that line in the IDE; without it the click always opens GitHub.
    pub fn with_open_in_ide(mut self, open_in_ide: Option<OnOpenInIde>) -> Self {
        self.open_in_ide = open_in_ide;
        self
    }

    pub fn with_image_row(mut self, image_row: Option<AnyElement>) -> Self {
        self.image_row = image_row;
        self
    }

    /// In that view a piece without lines was not touched since the reviewer read the step.
    pub fn in_change_since_read(mut self, is_change_since_read: bool) -> Self {
        self.is_change_since_read = is_change_since_read;
        self
    }

    /// Without it the rows take no comments.
    pub fn with_row_selecting(mut self, selecting: RowSelecting) -> Self {
        self.selecting = Some(selecting);
        self
    }

    pub fn with_threads(mut self, threads: Vec<(LineLocation, AnyElement)>) -> Self {
        self.threads = threads;
        self
    }
}

/// Hands each thread to the piece that has its line. A thread whose line no piece has is dropped.
pub fn threads_by_piece(
    pieces: &[PieceDiff],
    threads: Vec<(LineLocation, AnyElement)>,
) -> Vec<Vec<(LineLocation, AnyElement)>> {
    let mut by_piece: Vec<Vec<_>> = pieces.iter().map(|_| Vec::new()).collect();
    for (location, element) in threads {
        if let Some(index) = pieces
            .iter()
            .position(|piece| piece.lines().any(|line| line.location == location))
        {
            by_piece[index].push((location, element));
        }
    }
    by_piece
}

impl RenderOnce for DiffView {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let id = self.piece.id.to_string();
        let only_whitespace = self.piece.changes_only_whitespace.then(|| {
            div()
                .px_2()
                .py_1()
                .text_sm()
                .text_color(Palette::muted_text())
                .child("Only whitespace changed.")
        });
        let no_change_since_read = self.is_change_since_read
            && self.piece.hunks.is_empty()
            && self.piece.binary_file.is_none();
        let mut threads = self.threads;
        let hunks: Vec<AnyElement> = self
            .piece
            .hunks
            .into_iter()
            .enumerate()
            .flat_map(|(index, hunk)| {
                hunk_rows(
                    &id,
                    index,
                    hunk,
                    &self.on_more_context,
                    self.selecting.as_ref(),
                    self.open_in_ide.as_ref(),
                    &mut threads,
                )
            })
            .collect();
        div()
            .flex()
            .flex_col()
            .border_1()
            .border_color(Palette::border())
            .rounded_md()
            .overflow_hidden()
            .child(
                div()
                    .px_2()
                    .py_1()
                    .bg(Palette::sidebar())
                    .border_b_1()
                    .border_color(Palette::border())
                    .text_sm()
                    .flex()
                    .gap_2()
                    .child(div().font_family(CODE_FONT).child(id.clone()))
                    .children(
                        self.piece
                            .repin_status
                            .and_then(repin_label)
                            .map(|label| div().text_color(Palette::muted_text()).child(label)),
                    ),
            )
            .when(no_change_since_read, |view| {
                view.child(
                    div()
                        .px_2()
                        .py_1()
                        .text_sm()
                        .text_color(Palette::muted_text())
                        .child("No change since you read it"),
                )
            })
            .children(self.piece.binary_file.map(BinaryFileRow::new))
            .children(self.image_row)
            .children(only_whitespace)
            .children(hunks)
    }
}

fn repin_label(status: RepinStatus) -> Option<&'static str> {
    match status {
        RepinStatus::Changed => Some("changed"),
        RepinStatus::New => Some("new"),
        RepinStatus::Same | RepinStatus::Gone => None,
    }
}

/// A binary file has no lines to show, so it gets one row that sends the reviewer to GitHub.
#[derive(IntoElement)]
struct BinaryFileRow {
    link: GitHubLink,
}

impl BinaryFileRow {
    fn new(link: GitHubLink) -> Self {
        Self { link }
    }
}

impl RenderOnce for BinaryFileRow {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let link = self.link;
        div()
            .id(SharedString::from(link.to_string()))
            .px_2()
            .py_1()
            .text_sm()
            .cursor_pointer()
            .text_color(Palette::muted_text())
            .on_click(move |_, _, cx| cx.open_url(link.as_str()))
            .child("Binary file, open it on GitHub")
    }
}

fn hunk_rows(
    piece: &str,
    index: usize,
    hunk: DiffHunk,
    on_more_context: &OnMoreContext,
    selecting: Option<&RowSelecting>,
    open_in_ide: Option<&OnOpenInIde>,
    threads: &mut Vec<(LineLocation, AnyElement)>,
) -> Vec<AnyElement> {
    let mut rows = Vec::new();
    if index > 0 || hunk.more_above.is_some() {
        rows.push(hunk_gap(
            ElementId::from(format!("{piece}-above-{index}")),
            hunk.more_above,
            on_more_context,
        ));
    }
    for line in hunk.rows {
        let location = line.location.clone();
        rows.push(DiffRow::new(line, selecting.cloned(), open_in_ide.cloned()).into_any_element());
        let (here, later): (Vec<_>, Vec<_>) = std::mem::take(threads)
            .into_iter()
            .partition(|(at, _)| *at == location);
        *threads = later;
        rows.extend(here.into_iter().map(|(_, thread)| thread));
    }
    if hunk.more_below.is_some() {
        rows.push(hunk_gap(
            ElementId::from(format!("{piece}-below-{index}")),
            hunk.more_below,
            on_more_context,
        ));
    }
    rows
}

/// The band between hunks, like GitHub's `@@` line; clicking it shows more unchanged lines.
fn hunk_gap(
    id: ElementId,
    more: Option<MoreContext>,
    on_more_context: &OnMoreContext,
) -> AnyElement {
    let band = div()
        .id(id)
        .px_2()
        .bg(Palette::sidebar())
        .text_xs()
        .text_color(Palette::muted_text());
    let Some(more) = more else {
        return band.child("⋯").into_any_element();
    };
    let label = match more.edge {
        Edge::Above => "↑ Show more lines",
        Edge::Below => "↓ Show more lines",
    };
    let on_more_context = on_more_context.clone();
    band.cursor_pointer()
        .on_click(move |_, window, cx| on_more_context(&more, window, cx))
        .child(label)
        .into_any_element()
}

#[derive(IntoElement)]
struct DiffRow {
    line: DiffLine,
    selecting: Option<RowSelecting>,
    open_in_ide: Option<OnOpenInIde>,
}

impl DiffRow {
    fn new(
        line: DiffLine,
        selecting: Option<RowSelecting>,
        open_in_ide: Option<OnOpenInIde>,
    ) -> Self {
        Self {
            line,
            selecting,
            open_in_ide,
        }
    }
}

impl RenderOnce for DiffRow {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let location = &self.line.location;
        let link = self.line.github_link.clone();
        let ide_at = self.line.open_at().cloned();
        let open_in_ide = self.open_in_ide.clone();
        let number = location.line.to_string();
        let (base_number, head_number, sign) = match self.line.kind {
            LineKind::Removed => (number, String::new(), "-"),
            LineKind::Added => (String::new(), number, "+"),
            LineKind::Context { base_line, .. } => (base_line.to_string(), number, " "),
        };
        let is_selected = self
            .selecting
            .as_ref()
            .and_then(|selecting| selecting.selected.as_ref())
            .is_some_and(|selected| selected.contains(location));
        let comment_button = self
            .selecting
            .as_ref()
            .map(|selecting| comment_button(&self.line, selecting.on_gesture.clone()));
        let row = div()
            .id(SharedString::from(format!(
                "row-{}-{:?}-{}",
                location.path, location.side, location.line
            )))
            .group("diff-row")
            .flex()
            .font_family(CODE_FONT)
            .text_sm()
            .when_some(self.selecting.clone(), |row, selecting| {
                let at = location.clone();
                row.on_mouse_move(move |_, window, cx| {
                    (selecting.on_gesture)(RowGesture::Over, &at, window, cx)
                })
            })
            .child(
                div()
                    .id(SharedString::from(link.to_string()))
                    .flex()
                    .flex_none()
                    .cursor_pointer()
                    .on_click(move |event, _, cx| {
                        match (event.modifiers().secondary(), &open_in_ide, &ide_at) {
                            (true, Some(open_in_ide), Some(at)) => open_in_ide(at, cx),
                            _ => cx.open_url(link.as_str()),
                        }
                    })
                    .child(line_number(base_number))
                    .child(line_number(head_number)),
            )
            .child(
                comment_button.unwrap_or_else(|| div().w(px(20.)).flex_none().into_any_element()),
            )
            .child(div().w(px(16.)).flex_none().child(sign));
        let row = match self.line.kind {
            _ if is_selected => row.bg(Palette::current()),
            LineKind::Removed => row.bg(Palette::removed_line()),
            LineKind::Added => row.bg(Palette::added_line()),
            LineKind::Context { .. } => row,
        };
        row.child(
            div()
                .flex_1()
                .whitespace_nowrap()
                .child(highlighted(self.line)),
        )
    }
}

/// The "+" in the gutter that starts a comment. Rows outside GitHub's hunks have none.
fn comment_button(line: &DiffLine, on_gesture: OnRowGesture) -> AnyElement {
    let space = div().w(px(20.)).flex_none();
    if !line.takes_comment() {
        return space.into_any_element();
    }
    let location = line.location.clone();
    space
        .id("comment-button")
        .text_color(Palette::muted_text())
        .opacity(0.)
        .group_hover("diff-row", |button| button.opacity(1.))
        .cursor_pointer()
        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
            on_gesture(RowGesture::Press, &location, window, cx)
        })
        .child("+")
        .into_any_element()
}

fn highlighted(line: DiffLine) -> StyledText {
    let highlights: Vec<_> = line
        .spans
        .iter()
        .filter(|span| {
            line.text.is_char_boundary(span.range.start)
                && line.text.is_char_boundary(span.range.end)
        })
        .map(|span| {
            let style = HighlightStyle {
                color: Some(Palette::code(span.capture).into()),
                ..Default::default()
            };
            (span.range.clone(), style)
        })
        .collect();
    StyledText::new(line.text).with_highlights(highlights)
}

fn line_number(number: String) -> impl IntoElement {
    div()
        .w(px(48.))
        .flex_none()
        .pr_2()
        .text_right()
        .text_color(Palette::muted_text())
        .child(number)
}
