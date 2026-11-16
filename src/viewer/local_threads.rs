//! The threads of a local review. They never leave the machine: they are kept in the review's state
//! file, in their own section, and every change to them is a locked read and write of that file, so
//! the app and `agent-reply` each change the newest threads and never write a stale copy back.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::code_source::CodeSource;

use crate::file_path::FilePath;
use crate::line_number::LineNumber;
use crate::pull_request::{PullRequestUrl, Sha};
use crate::review_subject::ReviewSubject;

use super::comments::CommentPlace;
use super::outdated_lines::{FollowedLines, follow_outdated};
use super::ports::Resolution;
use super::review_state::ReviewStateFile;
use super::step_page::Side;
use super::threads::{CommentId, ReviewThread, ThreadComment, ThreadId};

/// Where a review's threads are kept: on GitHub for a PR, in its state file for a local review.
#[derive(Debug, Clone)]
pub enum ThreadHome {
    GitHub(PullRequestUrl),
    StateFile(LocalThreadStore),
}

impl ThreadHome {
    pub fn of(subject: &ReviewSubject, state_file: &ReviewStateFile) -> Self {
        match subject {
            ReviewSubject::PullRequest(pr) => Self::GitHub(pr.clone()),
            ReviewSubject::Local(_) => {
                Self::StateFile(LocalThreadStore::in_file(state_file.clone()))
            }
        }
    }
}

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
    /// is followed to its lines as GitHub's outdated threads are.
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

impl LocalThreads {
    pub fn is_empty(&self) -> bool {
        self.threads.is_empty()
    }

    fn next_id(&mut self, kind: &str) -> String {
        self.last_id += 1;
        format!("local-{kind}-{}", self.last_id)
    }

    fn start(&mut self, place: &CommentPlace, body: &str, at: &Sha) -> ThreadId {
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

    fn reply(
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

    fn edit(&mut self, comment: &CommentId, body: &str) -> anyhow::Result<()> {
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
    fn delete(&mut self, comment: &CommentId) -> anyhow::Result<()> {
        let thread = self
            .threads
            .iter_mut()
            .find(|thread| thread.comments.iter().any(|said| &said.id == comment))
            .ok_or_else(|| anyhow::anyhow!("no comment {comment} in the local threads"))?;
        thread.comments.retain(|said| &said.id != comment);
        self.threads.retain(|thread| !thread.comments.is_empty());
        Ok(())
    }

    fn set_resolved(&mut self, thread: &ThreadId, resolution: Resolution) -> anyhow::Result<()> {
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

    /// Takes every thread that was written on an older snapshot to `head`, at the lines `followed` found
    /// for it. A thread that could not be followed stays where it was.
    fn move_to(&mut self, head: &Sha, followed: &BTreeMap<ThreadId, FollowedLines>) {
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

impl LocalThread {
    fn lines(&self) -> Option<(LineNumber, LineNumber)> {
        match &self.place {
            CommentPlace::File(_) => None,
            CommentPlace::Lines { start, end } => Some((start.line, end.line)),
        }
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
fn without_last_comment(thread: &ReviewThread) -> ReviewThread {
    let mut sent = thread.clone();
    sent.comments.pop();
    sent
}

/// The local threads in a review's state file. Each change reads the file again under its lock and
/// writes only this section back.
#[derive(Debug, Clone)]
pub struct LocalThreadStore {
    file: ReviewStateFile,
}

impl LocalThreadStore {
    pub fn in_file(file: ReviewStateFile) -> Self {
        Self { file }
    }

    pub fn threads_at(&self, head: &Sha) -> anyhow::Result<Vec<ReviewThread>> {
        Ok(self.file.load()?.local_threads().at(head))
    }

    /// The threads at `head` and where each outdated one's lines are: the ones Update moved know it
    /// themselves, the others are followed from their snapshot.
    pub fn threads_and_followed_at(
        &self,
        source: &(impl CodeSource + ?Sized),
        head: &Sha,
    ) -> anyhow::Result<(Vec<ReviewThread>, BTreeMap<ThreadId, FollowedLines>)> {
        let state = self.file.load()?;
        let threads = state.local_threads().at(head);
        let moved = state.local_threads().moved();
        let to_follow: Vec<ReviewThread> = threads
            .iter()
            .filter(|thread| !moved.contains_key(&thread.id))
            .cloned()
            .collect();
        let mut followed = follow_outdated(&to_follow, source, head);
        followed.extend(moved);
        Ok((threads, followed))
    }

    /// Update: every thread written on an older snapshot is followed to `head` once and keeps its new place,
    /// so no thread depends on the old snapshot after this.
    pub fn follow_to(&self, source: &(impl CodeSource + ?Sized), head: &Sha) -> anyhow::Result<()> {
        let threads = self.threads_at(head)?;
        let followed = follow_outdated(&threads, source, head);
        self.change(|threads| {
            threads.move_to(head, &followed);
            Ok(())
        })
    }

    pub fn start(&self, place: &CommentPlace, body: &str, at: &Sha) -> anyhow::Result<ThreadId> {
        self.change(|threads| Ok(threads.start(place, body, at)))
    }

    pub fn reply(
        &self,
        thread: &ThreadId,
        body: &str,
        by: LocalAuthor,
    ) -> anyhow::Result<CommentId> {
        self.change(|threads| threads.reply(thread, body, by))
    }

    pub fn edit(&self, comment: &CommentId, body: &str) -> anyhow::Result<()> {
        self.change(|threads| threads.edit(comment, body))
    }

    pub fn delete(&self, comment: &CommentId) -> anyhow::Result<()> {
        self.change(|threads| threads.delete(comment))
    }

    pub fn set_resolved(&self, thread: &ThreadId, resolution: Resolution) -> anyhow::Result<()> {
        self.change(|threads| threads.set_resolved(thread, resolution))
    }

    /// A task the agent answered, written as Send now and `agent-reply` would write it, for a snapshot
    /// scene of a local review. Nothing is written when the review has threads already.
    #[cfg(feature = "snapshots")]
    pub fn stage_answered_task(
        &self,
        place: &CommentPlace,
        head: &Sha,
        (task, answer): (&str, &str),
    ) -> anyhow::Result<()> {
        use super::hand_offs::{AgentReplied, SentComment, SentFrom};

        if !self.threads_at(head)?.is_empty() {
            return Ok(());
        }
        let thread = self.start(place, task, head)?;
        self.reply(&thread, answer, LocalAuthor::Agent)?;
        let threads = self.threads_at(head)?;
        let mut answered = Ok(());
        self.file.update(|state| {
            let hand_offs = state.hand_offs_mut();
            hand_offs.record_new_thread(
                place.path(),
                &SentComment {
                    place: place.describe(),
                    from: SentFrom::File(place.path().clone()),
                    mode: super::comments::SendMode::SendNow,
                    body: task,
                    code: None,
                },
                Some(&Ok(threads.iter().map(without_last_comment).collect())),
            );
            if let Some(id) = hand_offs.all().last().map(super::hand_offs::HandOff::id) {
                answered = hand_offs.replied(id, &AgentReplied::default());
            }
        })?;
        Ok(answered?)
    }

    /// A task added to tasks and not sent yet, written as Add to tasks would write it, for a snapshot
    /// scene of a local review. Nothing is written when a thread holds the task already.
    #[cfg(feature = "snapshots")]
    pub fn stage_queued_task(
        &self,
        place: &CommentPlace,
        head: &Sha,
        task: &str,
    ) -> anyhow::Result<()> {
        use super::hand_offs::{SentComment, SentFrom};

        let staged = self.threads_at(head)?;
        if staged
            .iter()
            .any(|thread| thread.comments.iter().any(|said| said.body == task))
        {
            return Ok(());
        }
        self.start(place, task, head)?;
        let threads = self.threads_at(head)?;
        self.file.update(|state| {
            state.hand_offs_mut().record_new_thread(
                place.path(),
                &SentComment {
                    place: place.describe(),
                    from: SentFrom::File(place.path().clone()),
                    mode: super::comments::SendMode::AddToTasks,
                    body: task,
                    code: None,
                },
                Some(&Ok(threads)),
            );
        })
    }

    /// Nothing is written when the change is refused.
    fn change<T>(
        &self,
        change: impl FnOnce(&mut LocalThreads) -> anyhow::Result<T>,
    ) -> anyhow::Result<T> {
        let mut changed = None;
        self.file.update_if(|state| {
            let result = change(state.local_threads_mut());
            let keep = result.is_ok();
            changed = Some(result);
            keep
        })?;
        changed.unwrap_or_else(|| Err(anyhow::anyhow!("the local threads were not changed")))
    }
}
