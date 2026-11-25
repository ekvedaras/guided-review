//! Where a review's threads are kept, and the local threads in a review's state file, changed under its lock.

use super::comments::CommentPlace;
#[cfg(feature = "snapshots")]
use super::local_threads::without_last_comment;
use super::local_threads::{LocalAuthor, LocalThreads};
use super::outdated_lines::{FollowedLines, follow_outdated};
use super::ports::Resolution;
use super::review_state::ReviewStateFile;
use super::review_thread::{CommentId, ReviewThread, ThreadId};
use crate::code_source::CodeSource;
use crate::pull_request::{PullRequestUrl, Sha};
use crate::review_subject::ReviewSubject;
use std::collections::BTreeMap;

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

    /// A task sent to an agent that does not report its state, held back until the reviewer sends it
    /// anyway, for a snapshot scene of a local review. Nothing is written when a thread holds the task already.
    #[cfg(feature = "snapshots")]
    pub fn stage_held_back_task(
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
                Some(&Ok(threads)),
            );
            hand_offs.hold_back_latest();
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
