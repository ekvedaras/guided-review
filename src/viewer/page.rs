//! The page of the open screen, laid out as rows for reading. `Review::page` builds it once per change
//! of the review and keeps it, so drawing the rows and every question about what the screen shows
//! read the same page.

mod build;
mod rows;

use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

use crate::findings::FindingId;

pub use rows::{
    BinaryFileRow, BoxEdges, BoxFold, DiagramRow, FindingRow, FixesOffer, FoldControl,
    FullDiffControl, HunkGapRow, PageRow, PieceHeading, PieceLabel, ReviewerSummaryRow,
    RouteStopRow, ScreenFooter, ShownLine, StepHeading, StepIntro, StepNotes, SummaryColumns,
    SummaryStatsRow, ThreadRowPlace, Toolbar, line_count, pluralised,
};

use super::comments::CommentPlace;
use super::diff_line::DiffLine;
use super::file_block::FileBlock;
use super::focused_change::{PageChange, index_of_focus};
use super::jump::{JumpPlace, JumpTarget};
use super::line_location::LineLocation;
use super::overview::OverviewPage;
use super::review::ReviewCore;
use super::review_thread::ThreadId;
use super::row_selection::{RowSelection, SelectedLines};
use super::screen::Screen;
use super::visits::ReadingSpot;
use crate::file_path::FilePath;
use crate::local_facts::LocalChange;
use crate::pieces::PieceId;

/// The content of one screen as rows: the summary, a step's text and its file boxes, or the safety
/// net's, with what hangs under their lines.
#[derive(Clone)]
pub struct Page {
    screen: Screen,
    rows: Vec<PageRow>,
    edges: Vec<BoxEdges>,
    blocks: Vec<FileBlock>,
    diagram: Option<Arc<str>>,
    line_rows: HashMap<LineLocation, usize>,
    pieces: Vec<PieceAt>,
    images_to_load: Vec<FilePath>,
    rendered: Vec<FilePath>,
    changes: Vec<PageChange>,
    /// The index in `changes` of the focused change, when the screen shows it.
    focused: Option<usize>,
}

/// How many rows from the top of the view a spot looks for a line to name it by.
const SPOT_REACH: usize = 40;

#[derive(Clone)]
struct PieceAt {
    id: PieceId,
    path: FilePath,
    row: usize,
}

/// The rows a drag selected, as rows of the page: those in `rows` that show a line of `path`.
pub struct SelectedRows {
    rows: std::ops::RangeInclusive<usize>,
    path: FilePath,
}

/// The header of a file block that has scrolled away above the view, kept in sight.
pub struct PinnedHeader {
    pub header: Arc<PieceHeading>,
    pub header_row: usize,
    /// The symbol of the piece the top row is in, in a block of several pieces; a block of one
    /// piece names its symbol in its own header.
    pub symbol: Option<Arc<str>>,
}

impl SelectedRows {
    pub fn contains(&self, row: usize, line: &LineLocation) -> bool {
        self.rows.contains(&row) && line.path == self.path
    }
}

impl Page {
    pub(super) fn of(review: &ReviewCore) -> Self {
        let screen = review.screen().clone();
        let laid = match &screen {
            Screen::Summary => Some(build::summary(review)),
            Screen::Overview => Some(build::overview(review)),
            Screen::File(path) => review
                .file_page()
                .map(|page| build::file(review, path, page)),
            Screen::Step(_) => review.current_step().map(|page| build::step(review, page)),
            Screen::SafetyNet => review
                .safety_net()
                .map(|page| build::safety_net(review, page)),
        };
        let build::Laid {
            rows,
            blocks,
            diagram,
            rendered,
            changes,
        } = laid.unwrap_or_else(|| build::Laid {
            rows: Vec::new(),
            blocks: Vec::new(),
            diagram: None,
            rendered: Vec::new(),
            changes: Vec::new(),
        });
        let focused = review
            .display_options()
            .focused_change()
            .and_then(|focus| index_of_focus(focus, &changes));
        let mut line_rows = HashMap::new();
        let mut pieces = Vec::new();
        for (index, row) in rows.iter().enumerate() {
            match row {
                PageRow::Line(line) => {
                    line_rows.entry(line.location.clone()).or_insert(index);
                }
                PageRow::PieceHeader(heading) | PageRow::PieceMarker(heading) => {
                    pieces.push(PieceAt {
                        id: heading.piece.clone(),
                        path: heading.path.clone(),
                        row: index,
                    });
                }
                _ => {}
            }
        }
        let mut images_to_load: Vec<FilePath> = Vec::new();
        for path in blocks
            .iter()
            .flat_map(|block| &block.pieces)
            .filter(|piece| piece.is_image)
            .map(|piece| piece.id.path())
        {
            if review.display_options().image_preview(path).is_none()
                && !images_to_load.contains(path)
            {
                images_to_load.push(path.clone());
            }
        }
        Self {
            screen,
            edges: rows::box_edges(&rows),
            rows,
            blocks,
            diagram,
            line_rows,
            pieces,
            images_to_load,
            rendered,
            changes,
            focused,
        }
    }

    /// Every change of the screen, top to bottom.
    pub fn changes(&self) -> &[PageChange] {
        &self.changes
    }

    pub(super) fn focused_index(&self) -> Option<usize> {
        self.focused
    }

    /// The change that should take the focus when the window shows these rows: the first one in
    /// view, unless the focused change is in view. `None` when the focus stays.
    pub fn change_to_focus_for_view(&self, visible: &Range<usize>) -> Option<&PageChange> {
        if self
            .focused_change()
            .is_some_and(|change| change.is_in_view(visible))
        {
            return None;
        }
        self.changes
            .iter()
            .find(|change| change.is_in_view(visible))
    }

    /// Whether the change at this row of the page is the focused one.
    pub fn is_in_focused_change(&self, row: usize) -> bool {
        self.focused_change()
            .is_some_and(|change| change.rows.contains(&row))
    }

    /// The change that has the focus; none on a screen with no code, or while the focus is elsewhere.
    pub fn focused_change(&self) -> Option<&PageChange> {
        self.changes.get(self.focused?)
    }

    pub fn screen(&self) -> &Screen {
        &self.screen
    }

    pub fn rows(&self) -> &[PageRow] {
        &self.rows
    }

    pub fn row(&self, index: usize) -> Option<&PageRow> {
        self.rows.get(index)
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// Where the row meets the edge of its piece box.
    pub fn edges(&self, index: usize) -> BoxEdges {
        self.edges.get(index).copied().unwrap_or_default()
    }

    /// The line a row shows; `None` for a row that is not a line.
    pub fn line(&self, index: usize) -> Option<&ShownLine> {
        match self.rows.get(index)? {
            PageRow::Line(line) => Some(line),
            _ => None,
        }
    }

    /// The code at the top of the view when it shows `row`, to come back to it after rows change.
    pub fn spot_at(&self, row: usize) -> ReadingSpot {
        let anchor = (row..self.rows.len())
            .take(SPOT_REACH)
            .find_map(|index| self.line(index))
            .map(|line| line.location.clone());
        ReadingSpot::new(anchor, row)
    }

    /// The row to scroll to for `spot`: its code where this page shows it, else the row it was at.
    pub fn row_of_spot(&self, spot: &ReadingSpot) -> usize {
        spot.anchor()
            .and_then(|code| self.row_of_line(code))
            .unwrap_or_else(|| spot.row_hint().min(self.rows.len().saturating_sub(1)))
    }

    /// The row a finding is on; `None` when this page does not show it.
    pub fn row_of_finding(&self, id: &FindingId) -> Option<usize> {
        self.rows
            .iter()
            .position(|row| matches!(row, PageRow::Finding(finding) if finding.id == *id))
    }

    pub fn row_of_thread(&self, id: &ThreadId) -> Option<usize> {
        self.rows
            .iter()
            .position(|row| matches!(row, PageRow::Thread { view, .. } if view.thread.id == *id))
    }

    /// The row of the comment box at `place`; `None` when this page does not show it.
    pub fn row_of_comment_box(&self, place: &CommentPlace) -> Option<usize> {
        self.rows.iter().position(
            |row| matches!(row, PageRow::Composer { place: shown, .. } if **shown == *place),
        )
    }

    /// The first row that shows the line.
    pub fn row_of_line(&self, line: &LineLocation) -> Option<usize> {
        self.line_rows.get(line).copied()
    }

    /// The row a jump lands on: a line's row, a file's first piece, a symbol's piece (its file's
    /// header while the file's piece labels are hidden), else the top.
    pub fn row_of_jump(&self, target: &JumpTarget) -> usize {
        let piece = match &target.place {
            JumpPlace::StepTop | JumpPlace::Overview => None,
            JumpPlace::Line(line) => match self.row_of_line(line) {
                Some(row) => return row,
                None => self.pieces.iter().find(|piece| piece.path == line.path),
            },
            JumpPlace::File(path) => self.pieces.iter().find(|piece| piece.path == *path),
            JumpPlace::Piece(id) => self
                .pieces
                .iter()
                .find(|piece| piece.id == *id)
                .or_else(|| self.pieces.iter().find(|piece| piece.path == *id.path())),
        };
        piece.map_or(0, |piece| piece.row)
    }

    /// The rows a drag runs over, for drawing them; `None` when it started or is now off this page.
    pub fn selected_rows(&self, selection: &RowSelection) -> Option<SelectedRows> {
        let anchor = self.row_of_line(selection.anchor())?;
        let head = self.row_of_line(selection.head())?;
        Some(SelectedRows {
            rows: anchor.min(head)..=anchor.max(head),
            path: selection.anchor().path.clone(),
        })
    }

    /// Every line the screen shows, top to bottom; a rendered file shows none.
    pub fn lines(&self) -> impl Iterator<Item = &DiffLine> + Clone {
        self.blocks
            .iter()
            .filter(|block| !self.rendered.contains(&block.path))
            .flat_map(FileBlock::lines)
    }

    /// The lines a drag selected; `None` when where it started or where it is now is not on this page.
    pub fn selected(&self, selection: &RowSelection) -> Option<SelectedLines> {
        selection.resolve(self.lines())
    }

    /// The pictures of the page that are not loaded yet, each once, folded boxes too.
    pub fn images_to_load(&self) -> &[FilePath] {
        &self.images_to_load
    }

    /// A picture row on the page waits for its picture.
    pub fn has_pictures_waiting(&self) -> bool {
        self.rows.iter().any(|row| {
            matches!(
                row,
                PageRow::Image {
                    is_loaded: false,
                    ..
                }
            )
        })
    }

    /// The mermaid source of the diagram this screen shows, if it shows one.
    pub fn diagram(&self) -> Option<&str> {
        self.diagram.as_deref()
    }

    pub fn diagram_row(&self) -> Option<usize> {
        self.rows
            .iter()
            .position(|row| matches!(row, PageRow::StepIntro(intro) if intro.diagram.is_some()))
    }

    /// The overview of a basic tour, when this is its page.
    pub fn overview(&self) -> Option<&OverviewPage> {
        self.rows.iter().find_map(|row| match row {
            PageRow::Overview(overview) => Some(overview.as_ref()),
            _ => None,
        })
    }

    /// The row of the summary that holds the PR description, and the description.
    pub fn description(&self) -> Option<(usize, &crate::viewer::Markdown)> {
        self.rows
            .iter()
            .enumerate()
            .find_map(|(index, row)| match row {
                PageRow::SummaryColumns(columns) => columns
                    .description
                    .as_ref()
                    .map(|description| (index, description.as_ref())),
                PageRow::Overview(overview) => overview
                    .description
                    .as_ref()
                    .map(|description| (index, description)),
                _ => None,
            })
    }

    /// Where a local review's change to the shown file comes from; `None` for a PR and off a file screen.
    pub fn screen_change(&self) -> Option<LocalChange> {
        self.rows.iter().find_map(|row| match row {
            PageRow::Toolbar(toolbar) => toolbar.change,
            _ => None,
        })
    }

    /// Whether the screen's toolbar has the Hide whitespace toggle.
    pub fn offers_hiding_whitespace(&self) -> bool {
        self.rows.iter().any(
            |row| matches!(row, PageRow::Toolbar(toolbar) if toolbar.ignores_whitespace.is_some()),
        )
    }

    /// What stays pinned at the top while `top_row` is the first row in view: the header of its file
    /// block, once that header has scrolled away, with the symbol the reader is in.
    pub fn pinned_header(&self, top_row: usize) -> Option<PinnedHeader> {
        if matches!(
            self.rows.get(top_row)?,
            PageRow::PieceHeader(_) | PageRow::PieceEnd
        ) {
            return None;
        }
        let mut symbol = None;
        for (index, row) in self.rows.get(..top_row)?.iter().enumerate().rev() {
            match row {
                PageRow::PieceEnd => return None,
                PageRow::PieceMarker(marker) if symbol.is_none() => {
                    symbol = Some(marker.label.symbol.clone());
                }
                PageRow::PieceHeader(header) => {
                    return Some(PinnedHeader {
                        header: Arc::clone(header),
                        header_row: index,
                        symbol: symbol.flatten(),
                    });
                }
                _ => {}
            }
        }
        None
    }

    /// The heading of a step page.
    pub fn step_heading(&self) -> Option<&Arc<StepHeading>> {
        match self.rows.first()? {
            PageRow::StepHeading(heading) => Some(heading),
            _ => None,
        }
    }

    /// The most characters a line of this file has on the page; how far its box can scroll sideways.
    pub fn widest_line(&self, path: &FilePath) -> usize {
        self.rows
            .iter()
            .filter_map(|row| match row {
                PageRow::Line(line) if line.location.path == *path => {
                    Some(line.code.chars().count())
                }
                _ => None,
            })
            .max()
            .unwrap_or(0)
    }

    /// The footer of the last screen as it is when steps were passed unread: how many, and the way
    /// back to the first of them.
    #[cfg(feature = "snapshots")]
    pub fn stage_unread_end(&mut self, lead: &crate::viewer::TourLead, unread_left: usize) {
        for row in &mut self.rows {
            if let PageRow::ScreenFooter(footer) = row {
                *row = PageRow::ScreenFooter(Arc::new(ScreenFooter {
                    ticks: None,
                    has_previous: footer.has_previous,
                    next: None,
                    file_place: footer.file_place,
                    lead: Some(lead.clone()),
                    unread_left,
                }));
            }
        }
    }

    /// The footer of the page as it is once every step is read, and its heading ticked.
    #[cfg(feature = "snapshots")]
    pub fn stage_all_read(&mut self, steps: usize) {
        for row in &mut self.rows {
            match row {
                PageRow::ScreenFooter(footer) => {
                    *row = PageRow::ScreenFooter(Arc::new(ScreenFooter {
                        ticks: None,
                        has_previous: footer.has_previous,
                        next: footer.next.clone(),
                        file_place: footer.file_place,
                        lead: Some(crate::viewer::TourLead::Finish {
                            steps,
                            finish: Some(crate::viewer::FinishAction::FinishReview),
                        }),
                        unread_left: 0,
                    }));
                }
                PageRow::StepHeading(heading) => {
                    *row = PageRow::StepHeading(Arc::new(StepHeading {
                        id: heading.id.clone(),
                        title: Arc::clone(&heading.title),
                        number: heading.number,
                        is_ticked: true,
                        is_gone: heading.is_gone,
                        folds_on_title_click: heading.folds_on_title_click,
                    }));
                }
                _ => {}
            }
        }
    }
}
