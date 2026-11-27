//! The threads of a local review. They never leave the machine: they are kept in the review's state
//! file, in their own section, and every change to them is a locked read and write of that file, so
//! the app and `agent-reply` each change the newest threads and never write a stale copy back.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::code_source::CodeSource;

use crate::file_path::FilePath;
use crate::line_number::LineNumber;
use crate::pull_request::Sha;

use super::comments::CommentPlace;
use super::line_location::{LineLocation, Side};
use super::outdated_lines::{FollowedLines, follow_outdated};
use super::ports::Resolution;
use super::review_thread::{CommentId, ReviewThread, ThreadComment, ThreadId};

/// Who wrote a comment of a local thread. A local thread knows it, so no mark in the text says it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LocalAuthor {
    Reviewer,
    Agent,
}

impl LocalAuthor {
    fn shown_name(self) -> &'static str {
        match self {
            Self::Reviewer => "You",
            Self::Agent => "Your agent",
        }
    }
}

/// The `local_threads` section of a local review's state file.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalThreads {
    threads: Vec<LocalThread>,
    /// The last number given to a thread or a comment; ids are never given twice, even after a delete.
    last_id: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct LocalThread {
    id: ThreadId,
    place: CommentPlace,
    /// The snapshot the place's line numbers belong to. After Update the thread is outdated there and
    /// is followed to its lines as the host's outdated threads are.
    written_at: Sha,
    /// Set when Update followed the thread to a new snapshot: what its lines said before the move, so the
    /// thread shows as moved without the old snapshot, which Update lets git collect.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    moved: Option<Moved>,
    resolved: bool,
    comments: Vec<LocalComment>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Moved {
    before: Vec<String>,
    were_changed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct LocalComment {
    id: CommentId,
    by: LocalAuthor,
    body: String,
}

/// What the ids a private thread store gives start with; no host's id does.
const PRIVATE_ID_START: &str = "local-";

impl ThreadId {
    /// Whether a private thread store gave the id, not a host.
    pub fn is_private(&self) -> bool {
        self.as_str().starts_with(PRIVATE_ID_START)
    }
}

impl CommentId {
    /// Whether a private thread store gave the id, not a host.
    pub fn is_private(&self) -> bool {
        self.as_str().starts_with(PRIVATE_ID_START)
    }
}

impl ReviewThread {
    /// Whether the thread is kept on this machine only, which a host never sees.
    pub fn is_private(&self) -> bool {
        self.id.is_private()
    }
}

impl LocalThreads {
    pub fn is_empty(&self) -> bool {
        self.threads.is_empty()
    }

    fn next_id(&mut self, kind: &str) -> String {
        self.last_id += 1;
        format!("{PRIVATE_ID_START}{kind}-{}", self.last_id)
    }

    pub(super) fn start(&mut self, place: &CommentPlace, body: &str, at: &Sha) -> ThreadId {
        let id = ThreadId::new(&self.next_id("thread"));
        let comment = LocalComment {
            id: CommentId::new(&self.next_id("comment")),
            by: LocalAuthor::Reviewer,
            body: body.to_owned(),
        };
        self.threads.push(LocalThread {
            id: id.clone(),
            place: place.clone(),
            written_at: at.clone(),
            moved: None,
            resolved: false,
            comments: vec![comment],
        });
        id
    }

    pub(super) fn reply(
        &mut self,
        thread: &ThreadId,
        body: &str,
        by: LocalAuthor,
    ) -> anyhow::Result<CommentId> {
        let id = CommentId::new(&self.next_id("comment"));
        self.thread_mut(thread)?.comments.push(LocalComment {
            id: id.clone(),
            by,
            body: body.to_owned(),
        });
        Ok(id)
    }

    pub(super) fn edit(&mut self, comment: &CommentId, body: &str) -> anyhow::Result<()> {
        let found = self
            .threads
            .iter_mut()
            .flat_map(|thread| &mut thread.comments)
            .find(|said| &said.id == comment)
            .ok_or_else(|| anyhow::anyhow!("no comment {comment} in the local threads"))?;
        body.clone_into(&mut found.body);
        Ok(())
    }

    /// A thread whose last comment goes with it, as on GitHub.
    pub(super) fn delete(&mut self, comment: &CommentId) -> anyhow::Result<()> {
        let thread = self
            .threads
            .iter_mut()
            .find(|thread| thread.comments.iter().any(|said| &said.id == comment))
            .ok_or_else(|| anyhow::anyhow!("no comment {comment} in the local threads"))?;
        thread.comments.retain(|said| &said.id != comment);
        self.threads.retain(|thread| !thread.comments.is_empty());
        Ok(())
    }

    pub(super) fn set_resolved(
        &mut self,
        thread: &ThreadId,
        resolution: Resolution,
    ) -> anyhow::Result<()> {
        self.thread_mut(thread)?.resolved = resolution == Resolution::Resolved;
        Ok(())
    }

    fn thread_mut(&mut self, id: &ThreadId) -> anyhow::Result<&mut LocalThread> {
        self.threads
            .iter_mut()
            .find(|thread| &thread.id == id)
            .ok_or_else(|| anyhow::anyhow!("no thread {id} in the local threads"))
    }

    /// Where Update moved threads to, as `follow_outdated` tells it for threads it followed from a snapshot.
    pub fn moved(&self) -> BTreeMap<ThreadId, FollowedLines> {
        self.threads
            .iter()
            .filter_map(|thread| {
                let moved = thread.moved.as_ref()?;
                let (start, end) = thread.lines()?;
                Some((
                    thread.id.clone(),
                    FollowedLines {
                        start,
                        end,
                        before: moved.before.clone(),
                        were_changed: moved.were_changed,
                    },
                ))
            })
            .collect()
    }

    /// What the review of the PR the threads move to takes of them: each open thread's own words, at the
    /// same lines when the code there is the same at `head` and followed to where they went when it is not,
    /// else on the file. Resolved threads are left behind. Reads the code, so it runs off the main thread.
    pub(super) fn moved_to(&self, source: &dyn CodeSource, head: &Sha) -> Vec<MovedComment> {
        let open: Vec<&LocalThread> = self
            .threads
            .iter()
            .filter(|thread| !thread.resolved)
            .collect();
        let seen: Vec<ReviewThread> = open
            .iter()
            .map(|thread| thread.as_review_thread(head))
            .collect();
        let followed = follow_outdated(&seen, source, head);
        open.into_iter()
            .zip(seen)
            .filter_map(|(thread, seen)| {
                let body = thread.reviewer_words()?;
                let place = if seen.is_outdated {
                    match (followed.get(&thread.id), &thread.place) {
                        (Some(lines), CommentPlace::Lines { end, .. }) => CommentPlace::Lines {
                            start: LineLocation {
                                path: end.path.clone(),
                                side: Side::Head,
                                line: lines.start,
                            },
                            end: LineLocation {
                                path: end.path.clone(),
                                side: Side::Head,
                                line: lines.end,
                            },
                        },
                        _ => CommentPlace::on_file(thread.place.path()),
                    }
                } else {
                    thread.place.clone()
                };
                Some(MovedComment { place, body })
            })
            .collect()
    }

    /// Takes every thread that was written on an older snapshot to `head`, at the lines `followed` found
    /// for it. A thread that could not be followed stays where it was.
    pub(super) fn move_to(&mut self, head: &Sha, followed: &BTreeMap<ThreadId, FollowedLines>) {
        for thread in &mut self.threads {
            let Some(found) = followed
                .get(&thread.id)
                .filter(|_| thread.written_at != *head)
            else {
                continue;
            };
            let CommentPlace::Lines { start, end } = &thread.place else {
                continue;
            };
            let (mut start, mut end) = (start.clone(), end.clone());
            start.line = found.start;
            end.line = found.end;
            let earlier = thread.moved.take().filter(|moved| moved.were_changed);
            thread.moved = Some(Moved {
                before: earlier
                    .as_ref()
                    .map_or_else(|| found.before.clone(), |moved| moved.before.clone()),
                were_changed: found.were_changed || earlier.is_some(),
            });
            thread.place = CommentPlace::Lines { start, end };
            thread.written_at = head.clone();
        }
    }

    /// The threads as the page places them, seen from the snapshot `head`.
    pub fn at(&self, head: &Sha) -> Vec<ReviewThread> {
        self.threads
            .iter()
            .map(|thread| thread.as_review_thread(head))
            .collect()
    }
}

/// A thread's words, for the draft the PR's review gets of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct MovedComment {
    pub(super) place: CommentPlace,
    pub(super) body: String,
}

impl LocalThread {
    fn lines(&self) -> Option<(LineNumber, LineNumber)> {
        match &self.place {
            CommentPlace::File(_) => None,
            CommentPlace::Lines { start, end } => Some((start.line, end.line)),
        }
    }

    /// What the reviewer wrote in the thread; the agent's answers are the agent's, not the reviewer's to post.
    fn reviewer_words(&self) -> Option<String> {
        let words: Vec<&str> = self
            .comments
            .iter()
            .filter(|said| said.by == LocalAuthor::Reviewer)
            .map(|said| said.body.as_str())
            .collect();
        (!words.is_empty()).then(|| words.join("\n\n"))
    }

    fn as_review_thread(&self, head: &Sha) -> ReviewThread {
        let (path, side, start, end): (FilePath, Side, Option<LineNumber>, Option<LineNumber>) =
            match &self.place {
                CommentPlace::File(path) => (path.clone(), Side::Head, None, None),
                CommentPlace::Lines { start, end } => (
                    end.path.clone(),
                    end.side,
                    (start != end).then_some(start.line),
                    Some(end.line),
                ),
            };
        let is_file_comment = self.place.is_file_comment();
        let is_outdated = !is_file_comment && (self.written_at != *head || self.moved.is_some());
        ReviewThread {
            id: self.id.clone(),
            is_resolved: self.resolved,
            is_outdated,
            path,
            line: if is_outdated { None } else { end },
            start_line: if is_outdated { None } else { start },
            side,
            is_file_comment,
            original_line: end,
            original_start_line: start,
            original_commit: Some(self.written_at.to_string()),
            comments: self
                .comments
                .iter()
                .map(LocalComment::as_thread_comment)
                .collect(),
        }
    }
}

impl LocalComment {
    fn as_thread_comment(&self) -> ThreadComment {
        ThreadComment {
            id: self.id.clone(),
            author: self.by.shown_name().to_owned(),
            avatar_url: None,
            is_bot: false,
            by_agent: Some(self.by == LocalAuthor::Agent),
            by_viewer: self.by == LocalAuthor::Reviewer,
            body: self.body.clone(),
            url: String::new(),
            reactions: Vec::new(),
        }
    }
}

/// The thread as it was when the task was sent, before the agent answered.
#[cfg(feature = "snapshots")]
pub(super) fn without_last_comment(thread: &ReviewThread) -> ReviewThread {
    let mut sent = thread.clone();
    sent.comments.pop();
    sent
}
