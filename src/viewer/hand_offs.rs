use std::fmt;

use serde::{Deserialize, Serialize};

use crate::file_path::FilePath;

use super::comments::SendMode;
use super::review::StepId;
use super::threads::{ReviewThread, ThreadId};

/// Who a comment is for. The plain send actions are for the PR's author; only Ask agent also
/// reaches the tour agent.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Audience {
    #[default]
    Author,
    Agent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct HandOffId(u32);

impl fmt::Display for HandOffId {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        write!(formatter, "h{}", self.0)
    }
}

/// Where a hand-off is, as the reviewer sees it under the comment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HandOffState {
    Queued,
    Delivered,
    Working,
    Answered,
    EndedWithoutAnswer,
    Failed(String),
}

/// One earlier comment of the thread, for the agent's context.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThreadMessage {
    pub author: String,
    pub body: String,
}

/// One reviewer comment sent with Ask agent, as passed to the tour agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandOff {
    id: HandOffId,
    place: String,
    step: Option<StepId>,
    mode: SendMode,
    comment: String,
    /// `None` when the threads read after the send did not show it.
    thread: Option<ThreadId>,
    /// The thread before this comment, plain replies included.
    thread_so_far: Vec<ThreadMessage>,
    state: HandOffState,
}

impl HandOff {
    pub fn id(&self) -> HandOffId {
        self.id
    }

    pub fn comment(&self) -> &str {
        &self.comment
    }

    pub fn thread(&self) -> Option<&ThreadId> {
        self.thread.as_ref()
    }

    pub fn mode(&self) -> SendMode {
        self.mode
    }

    pub fn state(&self) -> &HandOffState {
        &self.state
    }
}

/// What a hand-off is made from: the comment as GitHub has it now, in the threads read right after the send.
pub(super) struct SentComment<'a> {
    pub path: &'a FilePath,
    pub place: String,
    pub step: Option<StepId>,
    pub mode: SendMode,
    pub body: &'a str,
}

/// The hand-offs of one PR, oldest first. Kept in the review state.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandOffs(Vec<HandOff>);

impl HandOffs {
    pub fn all(&self) -> &[HandOff] {
        &self.0
    }

    /// The comment started a thread, which is the one whose only comment is the text just sent
    /// and that no hand-off holds yet. `Failed` when the threads could not say which.
    pub(super) fn record_new_thread(
        &mut self,
        sent: &SentComment<'_>,
        threads: Option<&Result<Vec<ReviewThread>, String>>,
    ) {
        let thread = match threads {
            Some(Ok(threads)) => threads
                .iter()
                .find(|thread| {
                    !self.holds(&thread.id)
                        && &thread.path == sent.path
                        && thread
                            .comments
                            .last()
                            .is_some_and(|comment| comment.body.trim() == sent.body.trim())
                })
                .map(|thread| thread.id.clone())
                .ok_or_else(|| "GitHub shows no thread with the comment".to_owned()),
            Some(Err(reason)) => Err(format!("the threads could not be read: {reason}")),
            None => Err("the threads were not read".to_owned()),
        };
        let (thread, state) = match thread {
            Ok(thread) => (Some(thread), HandOffState::Queued),
            Err(reason) => (None, HandOffState::Failed(reason)),
        };
        let id = HandOffId(
            self.0
                .iter()
                .map(|hand_off| hand_off.id.0 + 1)
                .max()
                .unwrap_or(0),
        );
        self.0.push(HandOff {
            id,
            place: sent.place.clone(),
            step: sent.step.clone(),
            mode: sent.mode,
            comment: sent.body.to_owned(),
            thread,
            thread_so_far: Vec::new(),
            state,
        });
    }

    fn holds(&self, thread: &ThreadId) -> bool {
        self.0
            .iter()
            .any(|hand_off| hand_off.thread.as_ref() == Some(thread))
    }
}
