use std::rc::Rc;

use gpui::{
    AnyElement, App, ElementId, HighlightStyle, InteractiveElement, IntoElement, ParentElement,
    RenderOnce, SharedString, StatefulInteractiveElement, Styled, StyledText, Window, div, px,
};

use crate::viewer::{
    DiffHunk, DiffLine, Edge, GitHubLink, LineKind, LineLocation, MoreContext, PieceDiff,
};

use super::palette::{CODE_FONT, Palette};

pub type OnMoreContext = Rc<dyn Fn(&MoreContext, &mut Window, &mut App)>;

/// Our own diff view (Zed's is GPL). One row per line, grouped in hunks like on GitHub, and each row keeps
/// its `LineLocation` so comments can hang off any row later. A click on a line number opens that line on GitHub.
#[derive(IntoElement)]
pub struct DiffView {
    piece: PieceDiff,
    on_more_context: OnMoreContext,
    /// Shown right under the row at their location.
    threads: Vec<(LineLocation, AnyElement)>,
}

impl DiffView {
    pub fn new(piece: PieceDiff, on_more_context: OnMoreContext) -> Self {
        Self {
            piece,
            on_more_context,
            threads: Vec::new(),
        }
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
        let mut threads = self.threads;
        let hunks: Vec<AnyElement> = self
            .piece
            .hunks
            .into_iter()
            .enumerate()
            .flat_map(|(index, hunk)| {
                hunk_rows(&id, index, hunk, &self.on_more_context, &mut threads)
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
                    .font_family(CODE_FONT)
                    .child(id.clone()),
            )
            .children(self.piece.binary_file.map(BinaryFileRow::new))
            .children(only_whitespace)
            .children(hunks)
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
        rows.push(DiffRow::new(line).into_any_element());
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
}

impl DiffRow {
    fn new(line: DiffLine) -> Self {
        Self { line }
    }
}

impl RenderOnce for DiffRow {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let location = &self.line.location;
        let link = self.line.github_link.clone();
        let number = location.line.to_string();
        let (base_number, head_number, sign) = match self.line.kind {
            LineKind::Removed => (number, String::new(), "-"),
            LineKind::Added => (String::new(), number, "+"),
            LineKind::Context { base_line, .. } => (base_line.to_string(), number, " "),
        };
        let row = div()
            .flex()
            .font_family(CODE_FONT)
            .text_sm()
            .child(
                div()
                    .id(SharedString::from(link.to_string()))
                    .flex()
                    .flex_none()
                    .cursor_pointer()
                    .on_click(move |_, _, cx| cx.open_url(link.as_str()))
                    .child(line_number(base_number))
                    .child(line_number(head_number)),
            )
            .child(div().w(px(16.)).flex_none().child(sign));
        let row = match self.line.kind {
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
