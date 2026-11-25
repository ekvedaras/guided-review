//! The comments list: the review's threads, open ones before resolved ones, each under the heading of
//! the screen that shows it, so the reviewer finds a thread among the changes and goes to it.

use super::findings_list::shortened_place_within;
use super::review_thread::{ReviewThread, ThreadComment, ThreadId};
use super::screen::Screen;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommentsListRow {
    pub id: ThreadId,
    /// `file:line`, or the file of a comment on the whole file.
    pub place: String,
    /// Who started the thread.
    pub author: String,
    pub avatar_url: Option<String>,
    pub is_by_agent: bool,
    /// The first line of prose, or "Suggested change" for a comment that is only a suggestion.
    pub first_line: String,
    pub is_suggestion: bool,
    pub replies: usize,
    pub is_resolved: bool,
    pub is_outdated: bool,
    pub is_under_cursor: bool,
}

/// The rows of one screen, under its heading: "Step 3 · Title", "Safety net", a basic tour's file, or
/// "Not in this tour".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenGroup {
    pub heading: String,
    pub rows: Vec<CommentsListRow>,
}

/// The open threads, or the resolved ones, each count in its title: "Open · 3".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommentsSection {
    pub title: String,
    pub is_resolved: bool,
    /// The Resolved section folds: its heading stays, its rows go. The open section is always shown.
    pub is_shown: bool,
    /// The cursor is on the section's heading, which only the Resolved section lets it stop on.
    pub is_header_under_cursor: bool,
    pub groups: Vec<ScreenGroup>,
}

/// Where going to a thread of the list took the reviewer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThreadVisit {
    /// To the screen that shows it, which the window scrolls to its row.
    OnScreen,
    /// No screen shows it: to its first comment on GitHub.
    OnHost(String),
    /// Neither: the thread is gone, or a local thread no screen shows.
    Nowhere,
}

/// A thread, and the screen that shows it with that screen's heading.
#[derive(Debug, Clone)]
pub(super) struct ListedThread {
    pub thread: ReviewThread,
    pub shown_on: Option<(Screen, String)>,
}

const NOT_IN_THIS_TOUR: &str = "Not in this tour";

/// The longest place the list shows: its column has one width for every row.
const PLACE_CHARS: usize = 34;

#[derive(Debug, Default, Clone)]
pub struct CommentsList {
    threads: Vec<ListedThread>,
    cursor: usize,
    resolved_are_shown: bool,
}

impl CommentsList {
    /// `threads` come in reading order; open ones are listed before resolved ones, each keeping that order.
    pub(super) fn of(threads: Vec<ListedThread>) -> Self {
        let (resolved, open): (Vec<_>, Vec<_>) = threads
            .into_iter()
            .partition(|listed| listed.thread.is_resolved);
        Self {
            threads: open.into_iter().chain(resolved).collect(),
            cursor: 0,
            resolved_are_shown: false,
        }
        .unfolded_without_open_threads()
    }

    /// With no open thread the resolved ones are all there is, so they show.
    fn unfolded_without_open_threads(mut self) -> Self {
        self.resolved_are_shown = self.open_count() == 0;
        self
    }

    fn open_count(&self) -> usize {
        self.threads
            .iter()
            .filter(|listed| !listed.thread.is_resolved)
            .count()
    }

    fn resolved_count(&self) -> usize {
        self.threads.len() - self.open_count()
    }

    /// The cursor's stops: the open rows, then the Resolved heading, then the resolved rows while shown.
    fn stop_count(&self) -> usize {
        match self.resolved_count() {
            0 => self.open_count(),
            resolved if self.resolved_are_shown => self.open_count() + 1 + resolved,
            _ => self.open_count() + 1,
        }
    }

    /// The stop of the thread at `position` of `threads`, which has none while it is folded away.
    fn stop_of(&self, position: usize) -> Option<usize> {
        match position.checked_sub(self.open_count()) {
            None => Some(position),
            Some(_) if self.resolved_are_shown => Some(position + 1),
            Some(_) => None,
        }
    }

    fn position_under_cursor(&self) -> Option<usize> {
        match self.cursor.cmp(&self.open_count()) {
            std::cmp::Ordering::Less => Some(self.cursor),
            std::cmp::Ordering::Equal => None,
            std::cmp::Ordering::Greater => Some(self.cursor - 1),
        }
    }

    /// The same list over the threads another list holds now: the cursor stays on its thread, or
    /// its heading, and the Resolved section stays as the reviewer left it.
    #[must_use]
    pub fn refreshed_to(&self, newer: &Self) -> Self {
        let mut list = Self {
            threads: newer.threads.clone(),
            cursor: 0,
            resolved_are_shown: self.resolved_are_shown || newer.resolved_are_shown,
        };
        let thread_stop = self.thread_under_cursor().and_then(|id| {
            let position = list
                .threads
                .iter()
                .position(|listed| listed.thread.id == *id)?;
            list.stop_of(position)
        });
        let header_stop =
            (self.is_on_resolved_header() && list.resolved_count() > 0).then(|| list.open_count());
        list.cursor = thread_stop
            .or(header_stop)
            .unwrap_or(self.cursor)
            .min(list.stop_count().saturating_sub(1));
        list
    }

    pub fn has_resolved(&self) -> bool {
        self.resolved_count() > 0
    }

    pub const fn resolved_are_shown(&self) -> bool {
        self.resolved_are_shown
    }

    /// Enter or a click on the Resolved heading.
    pub fn toggle_resolved_shown(&mut self) {
        self.resolved_are_shown = !self.resolved_are_shown;
        self.cursor = self.cursor.min(self.stop_count().saturating_sub(1));
    }

    pub fn is_on_resolved_header(&self) -> bool {
        self.resolved_count() > 0 && self.cursor == self.open_count()
    }

    /// Right: the Resolved heading unfolds its section.
    pub fn unfold_under_cursor(&mut self) {
        if self.is_on_resolved_header() {
            self.resolved_are_shown = true;
        }
    }

    /// Left: folds the Resolved section; from a resolved row it goes up to the heading.
    pub fn fold_under_cursor(&mut self) {
        if self.is_on_resolved_header() {
            self.resolved_are_shown = false;
        } else if self.cursor > self.open_count() {
            self.cursor = self.open_count();
        }
    }

    pub fn sections(&self) -> Vec<CommentsSection> {
        [false, true]
            .into_iter()
            .filter_map(|is_resolved| self.section(is_resolved))
            .collect()
    }

    fn section(&self, is_resolved: bool) -> Option<CommentsSection> {
        let mut groups: Vec<(Option<&Screen>, ScreenGroup)> = Vec::new();
        for (position, listed) in self.threads.iter().enumerate() {
            if listed.thread.is_resolved != is_resolved {
                continue;
            }
            let screen = listed.shown_on.as_ref().map(|(screen, _)| screen);
            let row = self.row_at(position, listed);
            match groups.last_mut() {
                Some((last, group)) if *last == screen => group.rows.push(row),
                _ => {
                    let heading = listed
                        .shown_on
                        .as_ref()
                        .map_or(NOT_IN_THIS_TOUR, |(_, heading)| heading.as_str());
                    groups.push((
                        screen,
                        ScreenGroup {
                            heading: heading.to_owned(),
                            rows: vec![row],
                        },
                    ));
                }
            }
        }
        let count: usize = groups.iter().map(|(_, group)| group.rows.len()).sum();
        let name = if is_resolved { "Resolved" } else { "Open" };
        (count > 0).then(|| CommentsSection {
            title: format!("{name} · {count}"),
            is_resolved,
            is_shown: !is_resolved || self.resolved_are_shown,
            is_header_under_cursor: is_resolved && self.is_on_resolved_header(),
            groups: groups.into_iter().map(|(_, group)| group).collect(),
        })
    }

    /// Every row the cursor can walk, in its order: the resolved ones only while they are shown.
    pub fn rows(&self) -> Vec<CommentsListRow> {
        self.threads
            .iter()
            .enumerate()
            .filter(|(position, _)| self.stop_of(*position).is_some())
            .map(|(position, listed)| self.row_at(position, listed))
            .collect()
    }

    /// Every thread, folded away or not, for the title's count.
    pub const fn thread_count(&self) -> usize {
        self.threads.len()
    }

    fn row_at(&self, position: usize, listed: &ListedThread) -> CommentsListRow {
        CommentsListRow {
            is_under_cursor: self.stop_of(position) == Some(self.cursor),
            ..row_of(listed)
        }
    }

    /// The thread the cursor is on; none on the Resolved heading.
    pub fn thread_under_cursor(&self) -> Option<&ThreadId> {
        self.threads
            .get(self.position_under_cursor()?)
            .map(|listed| &listed.thread.id)
    }

    /// Past the last stop the cursor comes round to the first.
    pub fn cursor_down(&mut self) {
        self.cursor = (self.cursor + 1)
            .checked_rem(self.stop_count())
            .unwrap_or(0);
    }

    /// Before the first stop the cursor comes round to the last.
    pub fn cursor_up(&mut self) {
        self.cursor = self
            .cursor
            .checked_sub(1)
            .unwrap_or_else(|| self.stop_count().saturating_sub(1));
    }
}

fn row_of(listed: &ListedThread) -> CommentsListRow {
    let thread = &listed.thread;
    let opening = thread.comments.first();
    let body = opening.map_or_default(ThreadComment::shown_body);
    let is_suggestion = body.trim_start().starts_with(SUGGESTION_FENCE);
    let first_line = if is_suggestion {
        "Suggested change".to_owned()
    } else {
        first_line_of_prose(body).to_owned()
    };
    CommentsListRow {
        id: thread.id.clone(),
        place: place_of(thread),
        author: opening.map_or_default(ThreadComment::shown_author),
        avatar_url: opening.and_then(|comment| comment.avatar_url.clone()),
        is_by_agent: opening.is_some_and(ThreadComment::is_by_agent),
        first_line,
        is_suggestion,
        replies: thread.comments.len().saturating_sub(1),
        is_resolved: thread.is_resolved,
        is_outdated: thread.is_outdated,
        is_under_cursor: false,
    }
}

const SUGGESTION_FENCE: &str = "```suggestion";

/// The first line that is not blank and not a code fence.
fn first_line_of_prose(body: &str) -> &str {
    body.lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with("```"))
        .unwrap_or_default()
}

/// An outdated thread has no line at the head, so it is named by the line it was written on.
fn place_of(thread: &ReviewThread) -> String {
    match thread.line.or(thread.original_line) {
        Some(line) if !thread.is_file_comment => {
            shortened_place_within(&format!("{}:{line}", thread.path), PLACE_CHARS)
        }
        _ => shortened_place_within(&thread.path.to_string(), PLACE_CHARS),
    }
}
