//! What each row of a page is. Rows are stored one per line of a page, so a payload much bigger than
//! a pointer goes behind an `Arc`.

use std::collections::BTreeMap;
use std::ops::Range;
use std::path::PathBuf;
use std::sync::Arc;

use crate::file_path::FilePath;
use crate::pieces::PieceId;
use crate::tour::Step;
use crate::viewer::{
    Capture, CodeLink, CommentPlace, CommentProblem, DiffLine, HiddenLines, LineKind, LineLocation,
    Markdown, NextScreen, NotesButton, Place, QueuedComment, RenderedBlock, StepId, SummaryStats,
    ThreadView, TourLead, ViewSwitch,
};

#[derive(Clone)]
pub enum PageRow {
    SummaryTitle(Arc<str>),
    SummaryStats(Arc<SummaryStatsRow>),
    SummaryBullet(Arc<str>),
    RouteHeading,
    RouteStop(Arc<RouteStopRow>),
    TourLead(Arc<TourLead>),
    /// The summary's rows down to the reviewer summary, with the PR description as a column of its own,
    /// so a wide window puts the description beside them.
    SummaryColumns(Arc<SummaryColumns>),
    StepHeading(Arc<StepHeading>),
    StepIntro(Arc<StepIntro>),
    /// Its height follows the drawing; the layout re-measures it when the drawing changes.
    RepinNotes {
        gone_pieces: usize,
        text_may_be_old: bool,
    },
    Toolbar(Arc<Toolbar>),
    SafetyNetIntro,
    PieceHeader(Arc<PieceHeading>),
    /// Inside a file block of several pieces: where the next piece starts.
    PieceMarker(Arc<PieceHeading>),
    /// In the change since the read, a piece without lines was not touched since.
    NoChangeSinceRead,
    BinaryFile(Arc<BinaryFileRow>),
    /// The pictures of an image file; comments on the whole file hang under it.
    Image {
        path: FilePath,
        is_loaded: bool,
    },
    OnlyWhitespace,
    /// A top-level block of a Markdown file in its rendered view.
    RenderedBlock(Arc<RenderedBlock>),
    /// A Markdown file in its rendered view whose text is not read yet.
    RenderedFileWaiting,
    HunkGap(Arc<HunkGapRow>),
    Line(Arc<ShownLine>),
    /// Under its line inside the piece box, or after the code.
    Thread {
        view: Arc<ThreadView>,
        place: ThreadRowPlace,
        /// Of the reply box, if open; its text is read from the review when drawn.
        reply_lines: Option<usize>,
    },
    QueuedComment(Arc<QueuedComment>),
    /// A comment box; its text is read from the review when drawn, so only its line count is a part of the row.
    Composer {
        place: Arc<CommentPlace>,
        text_lines: usize,
        problem: Option<CommentProblem>,
        is_sending: bool,
    },
    /// The ask button under the last of the lines that have Asks.
    AskMarker(Arc<AskAt>),
    /// The space under a piece box; the box's last row draws its bottom edge.
    PieceEnd,
    ScreenFooter(Arc<ScreenFooter>),
}

/// Where a thread's row sits on the page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ThreadRowPlace {
    /// Hangs under the line it is about.
    UnderLine,
    /// Follows the step's code, for a thread with no line to hang under.
    AfterCode,
}

/// An ask button and what it asks about.
#[derive(Debug, Clone)]
pub struct AskAt {
    /// Tells this button apart from the page's other ask buttons.
    pub id: Arc<str>,
    pub place: Place,
    pub label: String,
}

pub struct StepHeading {
    pub id: StepId,
    pub title: Arc<str>,
    /// "Step 3 of 10"; `None` when the step is not in the reading order.
    pub number: Option<(usize, usize)>,
    pub is_ticked: bool,
    /// A step whose pieces all went away in a re-pin.
    pub is_gone: bool,
    /// Skim and ticked steps fold by a click on their title.
    pub folds_on_title_click: bool,
}

/// The controls over a screen's pieces, in one row.
pub struct Toolbar {
    pub ask: Option<AskAt>,
    pub fold: Option<FoldControl>,
    pub full_diff: Option<FullDiffControl>,
    /// `None` where no code is shown.
    pub ignores_whitespace: Option<bool>,
    /// `None` where no code is shown.
    pub wraps_lines: Option<bool>,
}

#[derive(Hash)]
pub struct FoldControl {
    pub step: StepId,
    pub is_folded: bool,
    pub piece_count: usize,
}

#[derive(Hash)]
pub struct FullDiffControl {
    pub step: StepId,
    pub is_showing_full_diff: bool,
}

/// The way on at the end of a screen.
#[derive(Hash)]
pub struct ScreenFooter {
    /// The step that is ticked on the way on: the open step, if not read yet.
    pub ticks: Option<StepId>,
    pub has_previous: bool,
    pub next: Option<NextScreen>,
    /// On the last screen once every step is read: the end of the tour.
    pub lead: Option<TourLead>,
}

/// The summary's strip of counts, each cut when the page is built.
#[derive(Hash)]
pub struct SummaryStatsRow {
    pub steps: Arc<str>,
    pub files: Arc<str>,
    pub added: Arc<str>,
    pub removed: Arc<str>,
    /// `None` when no thread is open.
    pub open_threads: Option<Arc<str>>,
}

impl SummaryStatsRow {
    pub(super) fn of(stats: SummaryStats) -> Self {
        let count = |count: usize, one: &str, many: &str| -> Arc<str> {
            if count == 1 {
                format!("1 {one}").into()
            } else {
                format!("{count} {many}").into()
            }
        };
        Self {
            steps: count(stats.steps, "step", "steps"),
            files: count(stats.files, "file changed", "files changed"),
            added: format!("+{}", stats.added_lines).into(),
            removed: format!("\u{2212}{}", stats.removed_lines).into(),
            open_threads: (stats.open_threads > 0)
                .then(|| count(stats.open_threads, "open thread", "open threads")),
        }
    }
}

/// One stop of the summary's route list.
#[derive(Hash)]
pub struct RouteStopRow {
    pub id: StepId,
    pub number: Arc<str>,
    pub title: Arc<str>,
    pub why: Arc<str>,
    /// "+2 details"; `None` with none.
    pub details: Option<Arc<str>>,
    pub is_read: bool,
    pub has_check: bool,
    pub is_first: bool,
    pub is_last: bool,
}

/// The fold of one file box of a step.
#[derive(Clone, Hash)]
pub struct BoxFold {
    pub step: StepId,
    pub path: FilePath,
    pub is_folded: bool,
}

pub struct PieceHeading {
    /// Set on the header of a file box in a step; the safety net does not fold.
    pub fold: Option<BoxFold>,
    pub piece: PieceId,
    pub label: PieceLabel,
    pub path: FilePath,
    pub ask: Option<AskAt>,
    /// "changed" or "new" after a re-pin.
    pub repin_label: Option<&'static str>,
    pub code_link: Option<CodeLink>,
    /// Set on the header of a box whose file can be rendered.
    pub view: Option<ViewSwitch>,
}

/// A piece's name cut into what is shown, once, so drawing shares the text instead of copying it.
#[derive(Debug, Clone)]
pub struct PieceLabel {
    /// With its trailing slash, or empty at the repo root.
    pub folder: Arc<str>,
    pub file_name: Arc<str>,
    pub symbol: Option<Arc<str>>,
    /// The last part of the symbol: `total` of `Order::total`.
    pub symbol_name: Option<Arc<str>>,
    /// A part the tour numbers (`hunk-1`) where the file has no symbol to name it by; the number
    /// means nothing to the reader, so it is not shown.
    pub is_part: bool,
}

impl PieceLabel {
    pub fn of(id: &PieceId) -> Self {
        Self {
            folder: id.folder().into(),
            file_name: id.file_name().into(),
            symbol: id.symbol_path().filter(|_| !id.is_hunk()).map(Arc::from),
            symbol_name: id.symbol_name().filter(|_| !id.is_hunk()).map(Arc::from),
            is_part: id.is_hunk(),
        }
    }

    /// What a marker inside a box of several pieces names: the symbol, the file for a whole-file
    /// piece, and nothing for a numbered part, which the gap above it already sets apart.
    pub fn shown_name(&self) -> Option<Arc<str>> {
        self.symbol
            .clone()
            .or_else(|| (!self.is_part).then(|| Arc::clone(&self.file_name)))
    }
}

pub struct BinaryFileRow {
    pub piece: Arc<str>,
    pub link: CodeLink,
}

pub struct ReviewerSummaryRow {
    pub markdown: Markdown,
    pub notes_button: NotesButton,
    pub picture_buttons: Arc<BTreeMap<PathBuf, NotesButton>>,
}

pub struct DiagramRow {
    pub step: StepId,
    pub notes_button: NotesButton,
}

/// What a step says before its code: its why, what to check, and its diagram. One row, so a wide
/// window can put the diagram beside the text.
pub struct StepIntro {
    pub why: Arc<str>,
    pub notes: Option<StepNotes>,
    pub diagram: Option<DiagramRow>,
}

/// The check hint and the TLDR the tour writer put on a step.
#[derive(Debug, Clone, Hash)]
pub struct StepNotes {
    pub check: Option<String>,
    pub tldr: Vec<String>,
}

impl StepNotes {
    /// `None` when the step has neither.
    pub(super) fn of(step: &Step) -> Option<Self> {
        (step.check.is_some() || !step.tldr.is_empty()).then(|| Self {
            check: step.check.clone(),
            tldr: step.tldr.clone(),
        })
    }
}

pub struct HunkGapRow {
    pub id: Arc<str>,
    pub gap: HiddenLines,
    /// "12 hidden lines", cut when the page is built.
    pub label: Arc<str>,
}

impl HunkGapRow {
    pub(super) fn new(id: String, gap: HiddenLines) -> Self {
        let label = match gap.count {
            Some(1) => "1 hidden line".to_owned(),
            Some(count) => format!("{count} hidden lines"),
            None if gap.after_above.is_some() || gap.before_below.is_some() => {
                "Show more lines".to_owned()
            }
            None => String::new(),
        };
        Self {
            id: id.into(),
            gap,
            label: label.into(),
        }
    }
}

pub struct SummaryColumns {
    pub summary: Vec<PageRow>,
    /// Above the description, in its column.
    pub reviewer_summary: Option<Arc<ReviewerSummaryRow>>,
    pub description: Option<Arc<Markdown>>,
}

/// A line as a row draws it, cut once when the page is built, so drawing copies no text.
pub struct ShownLine {
    pub location: LineLocation,
    pub kind: LineKind,
    /// Base and head numbers and the sign, padded so one monospace text fills the gutter.
    pub gutter: Arc<str>,
    pub code: Arc<str>,
    /// Cut to char boundaries already.
    pub highlights: Vec<(Range<usize>, Capture)>,
    /// The words that changed, cut to char boundaries already.
    pub changed_words: Vec<Range<usize>>,
    pub code_link: CodeLink,
    /// Rows outside GitHub's hunks show no button. Rows of the change since the read show one, to
    /// say why they take no comment.
    pub offers_comment_button: bool,
    /// A long line wraps onto more rows instead of being cut at the box's edge.
    pub wraps: bool,
}

impl ShownLine {
    pub(super) fn of(line: &DiffLine, wraps: bool) -> Self {
        let number = line.location.line.to_string();
        let (base_number, head_number, sign) = match line.kind {
            LineKind::Removed => (number, String::new(), '-'),
            LineKind::Added => (String::new(), number, '+'),
            LineKind::Context { base_line, .. } => (
                base_line.map(|line| line.to_string()).unwrap_or_default(),
                number,
                ' ',
            ),
        };
        let is_cut_on_chars = |range: &Range<usize>| {
            line.text.is_char_boundary(range.start) && line.text.is_char_boundary(range.end)
        };
        Self {
            location: line.location.clone(),
            kind: line.kind,
            gutter: format!("{base_number:>5} {head_number:>5} {sign} ").into(),
            code: line.text.as_str().into(),
            highlights: line
                .spans
                .iter()
                .map(|span| (span.range(), span.capture))
                .filter(|(range, _)| is_cut_on_chars(range))
                .collect(),
            changed_words: line
                .changed
                .iter()
                .filter(|range| is_cut_on_chars(range))
                .cloned()
                .collect(),
            code_link: line.code_link.clone(),
            offers_comment_button: line.offers_comment_button(),
            wraps,
        }
    }
}

/// Where a row of a piece box meets the box's rounded edge: the box closes at its end and above what
/// hangs under a line, like a thread, and opens again below it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct BoxEdges {
    pub opens: bool,
    pub closes: bool,
}

/// What a row is to the piece box around it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BoxPart {
    Outside,
    /// Between the box's sides.
    Boxed,
    /// Under a line but outside the box, like a thread: the box closes above it and opens again below.
    Hung,
    End,
}

impl PageRow {
    fn box_part(&self) -> BoxPart {
        match self {
            Self::PieceHeader(_)
            | Self::PieceMarker(_)
            | Self::NoChangeSinceRead
            | Self::BinaryFile(_)
            | Self::Image { .. }
            | Self::OnlyWhitespace
            | Self::RenderedBlock(_)
            | Self::RenderedFileWaiting
            | Self::HunkGap(_)
            | Self::Line(_) => BoxPart::Boxed,
            Self::Thread {
                place: ThreadRowPlace::UnderLine,
                ..
            }
            | Self::QueuedComment(_)
            | Self::Composer { .. }
            | Self::AskMarker(_) => BoxPart::Hung,
            Self::PieceEnd => BoxPart::End,
            _ => BoxPart::Outside,
        }
    }
}

/// The edges of each row: a boxed row opens the box again under what hangs above it, and closes it
/// over what hangs under it and at the box's end.
pub(super) fn box_edges(rows: &[PageRow]) -> Vec<BoxEdges> {
    let mut before = None;
    let mut edges = Vec::with_capacity(rows.len());
    for (index, row) in rows.iter().enumerate() {
        let part = row.box_part();
        let after = rows.get(index + 1).map(PageRow::box_part);
        edges.push(if part == BoxPart::Boxed {
            BoxEdges {
                opens: before == Some(BoxPart::Hung),
                closes: matches!(after, Some(BoxPart::Hung | BoxPart::End) | None),
            }
        } else {
            BoxEdges::default()
        });
        before = Some(part);
    }
    edges
}

/// Typing only changes a box's height when its line count changes.
pub fn line_count(text: &str) -> usize {
    text.split('\n').count()
}
