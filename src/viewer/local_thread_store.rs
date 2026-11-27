//! Where a review's threads are kept, and the local threads in a review's state file, changed under its lock.

use super::comments::{CommentPlace, SendMode};
use super::hand_offs::{AgentReplyRefused, AgentReplyRequest, HandOffId, HandOffs, Reviewed};
use super::local_threads::{LocalAuthor, LocalThreads};
use super::outdated_lines::{FollowedLines, follow_outdated};
use super::ports::{HostError, Resolution, ReviewThreadsPort};
use super::review_state::ReviewStateFile;
use super::review_thread::{CommentId, ReviewThread, ThreadId};
use crate::code_source::CodeSource;
use crate::pull_request::{PullRequestUrl, Sha};
use crate::review_subject::ReviewSubject;
use std::collections::BTreeMap;

/// Where one thread is kept: on the host, or in the review's state file.
#[derive(Debug, Clone)]
pub enum ThreadHome {
    Host(PullRequestUrl),
    StateFile(LocalThreadStore),
}

impl ThreadHome {
    /// Whether the thread is kept here.
    pub fn holds(&self, thread: &ReviewThread) -> bool {
        match self {
            Self::Host(_) => !thread.is_private(),
            Self::StateFile(_) => thread.is_private(),
        }
    }
}

/// Where the threads of a review are kept. A local review keeps all of them in its state file. A PR's
/// threads are the host's, and its review also keeps private threads in the state file: questions to
/// the tour agent that the host never sees. Each thread knows its own home.
#[derive(Debug, Clone)]
pub struct ThreadHomes {
    host: Option<PullRequestUrl>,
    private: LocalThreadStore,
}

impl ThreadHomes {
    pub fn of(subject: &ReviewSubject, state_file: &ReviewStateFile) -> Self {
        Self {
            host: subject.pull_request().cloned(),
            private: LocalThreadStore::in_file(state_file.clone()),
        }
    }

    /// Where the thread of a comment that exists already is kept, told by its id.
    pub fn of_comment(&self, comment: &CommentId) -> ThreadHome {
        self.home(!comment.is_private())
    }

    /// Where a new thread sent this way is kept: a task of the agent stays on this machine.
    pub fn for_new(&self, mode: SendMode) -> ThreadHome {
        self.home(!mode.is_task())
    }

    /// Every home the review has, to read all its threads.
    pub fn all(&self) -> Vec<ThreadHome> {
        self.host
            .iter()
            .map(|pr| ThreadHome::Host(pr.clone()))
            .chain([ThreadHome::StateFile(self.private.clone())])
            .collect()
    }

    /// The threads of every home at `head`, and where each outdated one's lines are.
    pub fn read(
        &self,
        host: &(impl ReviewThreadsPort + ?Sized),
        source: &(impl CodeSource + ?Sized),
        head: &Sha,
    ) -> Result<(Vec<ReviewThread>, BTreeMap<ThreadId, FollowedLines>), HostError> {
        let mut threads = Vec::new();
        let mut followed = BTreeMap::new();
        for home in self.all() {
            let (more, moved) = match &home {
                ThreadHome::Host(pr) => host.review_threads(pr).map(|threads| {
                    let moved = follow_outdated(&threads, source, head);
                    (threads, moved)
                })?,
                ThreadHome::StateFile(store) => store
                    .threads_and_followed_at(source, head)
                    .map_err(HostError::Other)?,
            };
            threads.extend(more);
            followed.extend(moved);
        }
        Ok((threads, followed))
    }

    fn home(&self, on_host: bool) -> ThreadHome {
        match &self.host {
            Some(pr) if on_host => ThreadHome::Host(pr.clone()),
            _ => ThreadHome::StateFile(self.private.clone()),
        }
    }
}

impl ThreadHomes {
    /// Where a thread that exists already is kept, told by its id.
    pub fn of_thread(&self, thread: &ThreadId) -> ThreadHome {
        self.home(!thread.is_private())
    }

    /// The agent's answer to a hand-off, to be written where the hand-off's thread is kept.
    pub fn agent_reply(
        &self,
        hand_offs: &HandOffs,
        id: HandOffId,
        body: &str,
    ) -> Result<AgentReplyRequest, AgentReplyRefused> {
        hand_offs.agent_reply(
            (&|thread| self.of_thread(thread), self.reviewed()),
            id,
            body,
        )
    }

    /// The agent's "Working on it…" for a hand-off, to be written where the hand-off's thread is kept.
    pub fn agent_working_on_it(
        &self,
        hand_offs: &HandOffs,
        id: HandOffId,
    ) -> Result<AgentReplyRequest, AgentReplyRefused> {
        hand_offs.agent_working_on_it((&|thread| self.of_thread(thread), self.reviewed()), id)
    }

    fn reviewed(&self) -> Reviewed {
        if self.host.is_some() {
            Reviewed::PullRequest
        } else {
            Reviewed::Local
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

    /// The state file the threads are kept in, for what changes more of it than the threads.
    #[cfg(feature = "snapshots")]
    pub(super) fn file(&self) -> &ReviewStateFile {
        &self.file
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
