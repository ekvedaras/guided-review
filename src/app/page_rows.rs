//! The review's page as the rows of one `gpui::list`. The page is built in the viewer; this keeps the
//! one the list shows, from one change of the review to the next, so a frame only draws the rows on
//! screen and builds nothing.
//!
//! Rule for every change of the review: take the review's page into new `PageRows` and tell the list
//! what `change_from` says. A row's key holds what decides its identity and height; rows whose key stayed
//! keep their measured height and the reader's scroll position, rows with a new key are measured again.
//! So a row's look may change freely (colours, ask counts, the drag selection), but whatever changes its
//! height must be in its key. Rows on screen are measured on every frame anyway, so the key matters for
//! the rows off screen.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;

use crate::app::components::markdown_view::{OpenDetails, PreparedMarkdown};
use crate::viewer::{
    AskAt, BoxEdges, JumpTarget, LineKind, Page, PageRow, Review, RowSelection, SelectedRows,
    ShownLine, StepHeading, line_count,
};

/// What the PR description's `MarkdownView` is keyed by, so its folds can be told apart.
pub const DESCRIPTION_KEY: &str = "pr-description";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RowKey(u64);

/// The summary's two markdown texts, readied for drawing once per page.
#[derive(Default)]
struct PreparedSummary {
    reviewer_summary: Option<Rc<PreparedMarkdown>>,
    description: Option<Rc<PreparedMarkdown>>,
}

impl PreparedSummary {
    fn of(page: &Page) -> Self {
        let Some(columns) = page.rows().iter().find_map(|row| match row {
            PageRow::SummaryColumns(columns) => Some(columns),
            _ => None,
        }) else {
            return Self::default();
        };
        Self {
            reviewer_summary: columns
                .reviewer_summary
                .as_ref()
                .map(|summary| Rc::new(PreparedMarkdown::of(summary.markdown.clone()))),
            description: columns
                .description
                .as_ref()
                .map(|description| Rc::new(PreparedMarkdown::of(description.as_ref().clone()))),
        }
    }
}

/// The page the list shows, with what the list keeps per row.
pub struct PageRows {
    /// `None` before the first page, so whatever comes next starts at its top.
    page: Option<Arc<Page>>,
    keys: Vec<RowKey>,
    summary: PreparedSummary,
}

/// What the list must do to show new rows in place of old ones.
#[derive(Debug, PartialEq, Eq)]
pub enum RowsChange {
    /// Another screen, or another review: start at its top.
    OtherScreen,
    Unchanged,
    /// The rows in `old` became `new_count` rows; the rest kept their place and height.
    Spliced {
        old: Range<usize>,
        new_count: usize,
    },
}

impl PageRows {
    /// Rows of no screen, so whatever comes next starts at its top.
    pub fn none() -> Self {
        Self {
            page: None,
            keys: Vec::new(),
            summary: PreparedSummary::default(),
        }
    }

    /// `open_details` are the `<details>` of comments the reader opened, which make their thread taller.
    pub fn of(review: &Review, open_details: &OpenDetails) -> Self {
        let page = Arc::clone(review.page());
        Self {
            keys: keys_of(&page, open_details),
            summary: PreparedSummary::of(&page),
            page: Some(page),
        }
    }

    /// Whether these rows were laid out from this very page.
    pub fn shows(&self, page: &Arc<Page>) -> bool {
        self.page
            .as_ref()
            .is_some_and(|shown| Arc::ptr_eq(shown, page))
    }

    pub fn len(&self) -> usize {
        self.page.as_deref().map_or(0, Page::len)
    }

    pub fn row(&self, index: usize) -> Option<&PageRow> {
        self.page.as_deref()?.row(index)
    }

    pub fn edges(&self, index: usize) -> BoxEdges {
        self.page
            .as_deref()
            .map_or_default(|page| page.edges(index))
    }

    pub fn line(&self, index: usize) -> Option<&ShownLine> {
        self.page.as_deref()?.line(index)
    }

    pub fn diagram(&self) -> Option<&str> {
        self.page.as_deref()?.diagram()
    }

    pub fn diagram_row(&self) -> Option<usize> {
        self.page.as_deref()?.diagram_row()
    }

    pub fn description_row(&self) -> Option<usize> {
        self.page.as_deref()?.description().map(|(row, _)| row)
    }

    /// The Mermaid text in the PR description card, in reading order.
    pub fn description_diagrams(&self) -> Vec<&str> {
        self.page
            .as_deref()
            .and_then(Page::description)
            .map_or_default(|(_, description)| description.diagrams())
    }

    pub fn prepared_reviewer_summary(&self) -> Option<Rc<PreparedMarkdown>> {
        self.summary.reviewer_summary.clone()
    }

    pub fn prepared_description(&self) -> Option<Rc<PreparedMarkdown>> {
        self.summary.description.clone()
    }

    #[cfg(feature = "snapshots")]
    pub fn description_fold_keys(&self) -> Vec<String> {
        self.summary
            .description
            .as_ref()
            .map_or_default(|description| description.fold_keys(DESCRIPTION_KEY))
    }

    #[cfg(feature = "snapshots")]
    pub fn first_summary_picture(
        &self,
    ) -> Option<crate::app::components::picture_zoom::ZoomedPicture> {
        self.summary.reviewer_summary.as_ref()?.first_picture()
    }

    pub fn has_pictures_waiting(&self) -> bool {
        self.page.as_deref().is_some_and(Page::has_pictures_waiting)
    }

    pub fn widest_line(&self, path: &crate::file_path::FilePath) -> usize {
        self.page
            .as_deref()
            .map_or(0, |page| page.widest_line(path))
    }

    pub fn row_of_jump(&self, target: &JumpTarget) -> usize {
        self.page
            .as_deref()
            .map_or(0, |page| page.row_of_jump(target))
    }

    pub fn selected_rows(&self, selection: &RowSelection) -> Option<SelectedRows> {
        self.page.as_deref()?.selected_rows(selection)
    }

    pub fn screen_ask(&self) -> Option<&AskAt> {
        self.page.as_deref()?.screen_ask()
    }

    pub fn step_heading(&self) -> Option<&Arc<StepHeading>> {
        self.page.as_deref()?.step_heading()
    }

    /// The footer of the screen as it is once every step is read, and its heading ticked.
    #[cfg(feature = "snapshots")]
    pub fn stage_all_read(&mut self, steps: usize, open_details: &OpenDetails) {
        let Some(page) = &self.page else {
            return;
        };
        let mut staged = Page::clone(page);
        staged.stage_all_read(steps);
        self.keys = keys_of(&staged, open_details);
        self.page = Some(Arc::new(staged));
    }

    pub fn change_from(&self, old: &Self) -> RowsChange {
        let screen = |rows: &Self| rows.page.as_deref().map(Page::screen).cloned();
        if screen(self) != screen(old) {
            return RowsChange::OtherScreen;
        }
        changed_span(&old.keys, &self.keys)
    }
}

/// A row's edges are a part of its key: a row that comes to close its box is drawn and measured anew.
fn keys_of(page: &Page, open_details: &OpenDetails) -> Vec<RowKey> {
    page.rows()
        .iter()
        .enumerate()
        .map(|(index, row)| {
            let mut hasher = DefaultHasher::new();
            key(row, open_details).0.hash(&mut hasher);
            page.edges(index).hash(&mut hasher);
            RowKey(hasher.finish())
        })
        .collect()
}

fn key(row: &PageRow, open_details: &OpenDetails) -> RowKey {
    let mut hasher = DefaultHasher::new();
    std::mem::discriminant(row).hash(&mut hasher);
    match row {
        PageRow::SummaryTitle(text) | PageRow::SummaryBullet(text) => {
            text.hash(&mut hasher);
        }
        PageRow::StepHeading(heading) => {
            heading.id.to_string().hash(&mut hasher);
            heading.title.hash(&mut hasher);
            heading.number.hash(&mut hasher);
        }
        PageRow::SummaryColumns(columns) => {
            for row in &columns.summary {
                key(row, open_details).0.hash(&mut hasher);
            }
            if let Some(summary) = &columns.reviewer_summary {
                summary.markdown.hash(&mut hasher);
                summary.notes_button.hash(&mut hasher);
                summary.picture_buttons.hash(&mut hasher);
            }
            columns.description.hash(&mut hasher);
        }
        PageRow::AskMarker(ask) => ask.id.hash(&mut hasher),
        PageRow::Toolbar(toolbar) => {
            toolbar.ask.as_ref().map(|ask| &ask.id).hash(&mut hasher);
            toolbar.fold.hash(&mut hasher);
            toolbar.full_diff.hash(&mut hasher);
            toolbar.ignores_whitespace.hash(&mut hasher);
            toolbar.wraps_lines.hash(&mut hasher);
        }
        PageRow::ScreenFooter(footer) => footer.hash(&mut hasher),
        PageRow::SummaryStats(stats) => stats.hash(&mut hasher),
        PageRow::RouteStop(stop) => stop.hash(&mut hasher),
        PageRow::TourLead(lead) => lead.hash(&mut hasher),
        PageRow::StepIntro(intro) => {
            intro.why.hash(&mut hasher);
            intro.notes.hash(&mut hasher);
            intro
                .diagram
                .as_ref()
                .map(|diagram| &diagram.step)
                .hash(&mut hasher);
            intro
                .diagram
                .as_ref()
                .map(|diagram| &diagram.notes_button)
                .hash(&mut hasher);
        }
        PageRow::RepinNotes {
            gone_pieces,
            text_may_be_old,
        } => {
            gone_pieces.hash(&mut hasher);
            text_may_be_old.hash(&mut hasher);
        }
        PageRow::PieceHeader(heading) | PageRow::PieceMarker(heading) => {
            heading.fold.hash(&mut hasher);
            heading.piece.hash(&mut hasher);
            heading.repin_label.hash(&mut hasher);
            heading.view.hash(&mut hasher);
        }
        PageRow::RenderedBlock(block) => block.hash(&mut hasher),
        PageRow::BinaryFile(binary) => {
            binary.piece.hash(&mut hasher);
            binary.link.hash(&mut hasher);
        }
        PageRow::Image { path, is_loaded } => {
            path.hash(&mut hasher);
            is_loaded.hash(&mut hasher);
        }
        PageRow::HunkGap(gap) => {
            gap.id.hash(&mut hasher);
            gap.gap.hash(&mut hasher);
        }
        PageRow::Line(line) => {
            line.location.path.hash(&mut hasher);
            line.location.side.hash(&mut hasher);
            line.location.line.hash(&mut hasher);
            matches!(line.kind, LineKind::Context { .. }).hash(&mut hasher);
            line.wraps.hash(&mut hasher);
        }
        PageRow::Thread {
            view,
            place,
            reply_lines,
        } => {
            view.thread.id.hash(&mut hasher);
            view.is_folded.hash(&mut hasher);
            for comment in &view.thread.comments {
                comment.id.hash(&mut hasher);
                comment.body.hash(&mut hasher);
                for count in &comment.reactions {
                    count.reaction.hash(&mut hasher);
                    count.count.hash(&mut hasher);
                    count.viewer_has_reacted.hash(&mut hasher);
                }
            }
            view.commented_lines.hash(&mut hasher);
            view.hand_off_line.hash(&mut hasher);
            view.failed_hand_off.hash(&mut hasher);
            view.delete_asked.hash(&mut hasher);
            reply_lines.hash(&mut hasher);
            for (id, text) in &view.edit_drafts {
                id.hash(&mut hasher);
                line_count(text).hash(&mut hasher);
            }
            open_details
                .open_under(&format!("thread-{}-", view.thread.id))
                .hash(&mut hasher);
            place.hash(&mut hasher);
        }
        PageRow::QueuedComment(queued) => {
            queued.draft.place.hash(&mut hasher);
            queued.failed_because.hash(&mut hasher);
        }
        PageRow::Composer {
            place,
            text_lines,
            problem,
            is_sending,
        } => {
            place.hash(&mut hasher);
            text_lines.hash(&mut hasher);
            problem.hash(&mut hasher);
            is_sending.hash(&mut hasher);
        }
        PageRow::SafetyNetIntro
        | PageRow::RouteHeading
        | PageRow::NoChangeSinceRead
        | PageRow::OnlyWhitespace
        | PageRow::RenderedFileWaiting
        | PageRow::PieceEnd => {}
    }
    RowKey(hasher.finish())
}

fn changed_span(old: &[RowKey], new: &[RowKey]) -> RowsChange {
    let same_start = old.iter().zip(new).take_while(|(a, b)| a == b).count();
    let same_end = old
        .iter()
        .skip(same_start)
        .rev()
        .zip(new.iter().skip(same_start).rev())
        .take_while(|(a, b)| a == b)
        .count();
    let old_changed = same_start..old.len() - same_end;
    let new_count = new.len() - same_end - same_start;
    if old_changed.is_empty() && new_count == 0 {
        RowsChange::Unchanged
    } else {
        RowsChange::Spliced {
            old: old_changed,
            new_count,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::file_path::FilePath;
    use crate::line_number::LineNumber;
    use crate::viewer::{
        CommentId, CommentPlace, CommentProblem, Reaction, ReactionCount, ReviewThread, Side,
        ThreadComment, ThreadId, ThreadRowPlace, ThreadView,
    };
    use std::collections::BTreeMap;

    fn line(number: usize) -> LineNumber {
        LineNumber::try_from(number).unwrap()
    }

    fn thread_view() -> ThreadView {
        ThreadView {
            thread: ReviewThread {
                id: ThreadId::new("T1"),
                is_resolved: false,
                is_outdated: false,
                path: FilePath::new("a.rs").unwrap(),
                line: Some(line(1)),
                start_line: None,
                side: Side::Head,
                is_file_comment: false,
                original_line: Some(line(1)),
                original_start_line: None,
                original_commit: None,
                comments: vec![ThreadComment {
                    avatar_url: None,
                    id: CommentId::new("C1"),
                    author: "alice".into(),
                    is_bot: false,
                    by_viewer: false,
                    body: "why?".into(),
                    url: String::new(),
                    reactions: Vec::new(),
                }],
            },
            is_folded: false,
            anchor: None,
            commented_lines: Arc::new([]),
            reply_draft: None,
            edit_drafts: BTreeMap::new(),
            delete_asked: None,
            hand_off_line: None,
            agent_is_working: false,
            followed: None,
            failed_hand_off: None,
        }
    }

    fn thread_key(view: ThreadView, reply_lines: Option<usize>, open: &[&str]) -> RowKey {
        let mut open_details = OpenDetails::default();
        for details in open {
            open_details.toggle(details);
        }
        key(
            &PageRow::Thread {
                view: Arc::new(view),
                place: ThreadRowPlace::UnderLine,
                reply_lines,
            },
            &open_details,
        )
    }

    #[test]
    fn a_thread_row_is_measured_again_when_a_reaction_a_body_or_a_details_changes_its_height() {
        let plain = thread_key(thread_view(), None, &[]);

        let mut reacted = thread_view();
        reacted.thread.comments[0].reactions = vec![ReactionCount {
            reaction: Reaction::Heart,
            count: 1,
            viewer_has_reacted: true,
        }];
        let mut edited = thread_view();
        edited.thread.comments[0].body = "why?\n\nreally".into();

        assert_ne!(plain, thread_key(reacted, None, &[]));
        assert_ne!(plain, thread_key(edited, None, &[]));
        assert_ne!(plain, thread_key(thread_view(), None, &["thread-T1-0-2"]));
        assert_ne!(plain, thread_key(thread_view(), Some(1), &[]));
        assert_ne!(
            thread_key(thread_view(), Some(1), &[]),
            thread_key(thread_view(), Some(2), &[])
        );
    }

    fn thread_key_editing(text: Option<&str>) -> RowKey {
        let mut view = thread_view();
        view.edit_drafts = text
            .map(|text| (CommentId::new("C1"), text.to_owned()))
            .into_iter()
            .collect();
        thread_key(view, None, &[])
    }

    #[test]
    fn a_thread_row_is_measured_again_when_a_comment_edit_opens_or_changes_its_line_count() {
        assert_ne!(thread_key_editing(None), thread_key_editing(Some("a")));
        assert_ne!(
            thread_key_editing(Some("a")),
            thread_key_editing(Some("a\nb"))
        );
        assert_eq!(
            thread_key_editing(Some("a")),
            thread_key_editing(Some("a longer one"))
        );
    }

    #[test]
    fn typing_inside_a_line_leaves_the_thread_row_as_it_was() {
        let mut typed = thread_view();
        typed.reply_draft = Some("because".into());
        let mut typed_more = thread_view();
        typed_more.reply_draft = Some("because it".into());

        assert_eq!(
            thread_key(typed, Some(line_count("because")), &[]),
            thread_key(typed_more, Some(line_count("because it")), &[])
        );
    }

    fn composer_key(text: &str, problem: Option<CommentProblem>) -> RowKey {
        key(
            &PageRow::Composer {
                place: Arc::new(CommentPlace::on_file(&FilePath::new("a.rs").unwrap())),
                text_lines: line_count(text),
                problem,
                is_sending: false,
            },
            &OpenDetails::default(),
        )
    }

    #[test]
    fn a_comment_box_row_changes_with_its_line_count_and_its_problem_only() {
        assert_eq!(composer_key("a", None), composer_key("a longer one", None));
        assert_ne!(composer_key("a", None), composer_key("a\nb", None));
        assert_ne!(
            composer_key("a", None),
            composer_key("a", Some(CommentProblem::Rejected("rejected".into())))
        );
    }

    fn keys(ids: &[u64]) -> Vec<RowKey> {
        ids.iter().copied().map(RowKey).collect()
    }

    #[test]
    fn same_rows_need_no_change() {
        assert_eq!(
            changed_span(&keys(&[1, 2, 3]), &keys(&[1, 2, 3])),
            RowsChange::Unchanged
        );
    }

    #[test]
    fn rows_shown_in_the_middle_splice_only_there() {
        assert_eq!(
            changed_span(&keys(&[1, 2, 3, 4]), &keys(&[1, 2, 7, 8, 9, 3, 4])),
            RowsChange::Spliced {
                old: 2..2,
                new_count: 3
            }
        );
    }

    #[test]
    fn a_changed_row_is_spliced_alone() {
        assert_eq!(
            changed_span(&keys(&[1, 2, 3]), &keys(&[1, 5, 3])),
            RowsChange::Spliced {
                old: 1..2,
                new_count: 1
            }
        );
    }

    #[test]
    fn repeated_rows_are_not_counted_twice() {
        assert_eq!(
            changed_span(&keys(&[1, 1]), &keys(&[1])),
            RowsChange::Spliced {
                old: 1..2,
                new_count: 0
            }
        );
    }
}
