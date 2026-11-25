//! The comments list: one row per review thread, in reading order, so the reviewer finds a thread
//! among the changes and goes to it.

use super::findings_list::shortened_place;
use super::review_thread::{ReviewThread, ThreadId};
use super::screen::Screen;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommentsListRow {
    pub id: ThreadId,
    /// `file:line`, or the file of a comment on the whole file.
    pub place: String,
    /// The step that shows it, "Safety net", or a basic tour's file; none when no screen shows it.
    pub screen_title: Option<String>,
    /// Who started the thread.
    pub author: String,
    pub first_line: String,
    pub replies: usize,
    pub is_resolved: bool,
    pub is_outdated: bool,
    pub is_under_cursor: bool,
}

/// Where going to a thread of the list took the reviewer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThreadVisit {
    /// To the screen that shows it, which the window scrolls to its row.
    OnScreen,
    /// No screen shows it: to its first comment on GitHub.
    OnGitHub(String),
    /// Neither: the thread is gone, or a local thread no screen shows.
    Nowhere,
}

/// A thread, and the screen that shows it with that screen's title.
#[derive(Debug, Clone)]
pub(super) struct ListedThread {
    pub thread: ReviewThread,
    pub shown_on: Option<(Screen, String)>,
}

#[derive(Debug, Default, Clone)]
pub struct CommentsList {
    threads: Vec<ListedThread>,
    cursor: usize,
}

impl CommentsList {
    pub(super) const fn of(threads: Vec<ListedThread>) -> Self {
        Self { threads, cursor: 0 }
    }

    pub fn rows(&self) -> Vec<CommentsListRow> {
        self.threads
            .iter()
            .enumerate()
            .map(|(position, listed)| CommentsListRow {
                is_under_cursor: position == self.cursor,
                ..row_of(listed)
            })
            .collect()
    }

    pub const fn is_empty(&self) -> bool {
        self.threads.is_empty()
    }

    pub fn thread_under_cursor(&self) -> Option<&ThreadId> {
        self.threads
            .get(self.cursor)
            .map(|listed| &listed.thread.id)
    }

    pub fn cursor_down(&mut self) {
        self.cursor = (self.cursor + 1).min(self.threads.len().saturating_sub(1));
    }

    pub const fn cursor_up(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    /// A click on a row puts the cursor there.
    pub fn move_cursor_to(&mut self, id: &ThreadId) {
        if let Some(position) = self
            .threads
            .iter()
            .position(|listed| listed.thread.id == *id)
        {
            self.cursor = position;
        }
    }
}

fn row_of(listed: &ListedThread) -> CommentsListRow {
    let thread = &listed.thread;
    let opening = thread.comments.first();
    CommentsListRow {
        id: thread.id.clone(),
        place: place_of(thread),
        screen_title: listed.shown_on.as_ref().map(|(_, title)| title.clone()),
        author: opening.map_or_default(super::review_thread::ThreadComment::shown_author),
        first_line: opening
            .and_then(|comment| comment.shown_body().lines().next())
            .unwrap_or_default()
            .to_owned(),
        replies: thread.comments.len().saturating_sub(1),
        is_resolved: thread.is_resolved,
        is_outdated: thread.is_outdated,
        is_under_cursor: false,
    }
}

/// An outdated thread has no line at the head, so it is named by the line it was written on.
fn place_of(thread: &ReviewThread) -> String {
    match thread.line.or(thread.original_line) {
        Some(line) if !thread.is_file_comment => {
            shortened_place(&format!("{}:{line}", thread.path))
        }
        _ => shortened_place(&thread.path.to_string()),
    }
}
