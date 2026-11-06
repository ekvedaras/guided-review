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

    pub fn thread_so_far(&self) -> &[ThreadMessage] {
        &self.thread_so_far
    }

    pub fn state(&self) -> &HandOffState {
        &self.state
    }
}

/// What a hand-off is made from: the comment as GitHub has it now, in the threads read right after the send.
pub(super) struct SentComment<'a> {
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

    /// The comment started a thread, which is the one whose last comment is the text just sent
    /// and that no hand-off holds yet.
    pub(super) fn record_new_thread(
        &mut self,
        path: &FilePath,
        sent: &SentComment<'_>,
        threads: Option<&Result<Vec<ReviewThread>, String>>,
    ) {
        let found = read_of(threads).and_then(|threads| {
            threads
                .iter()
                .find(|thread| {
                    !self.holds(&thread.id) && &thread.path == path && ends_with(thread, sent.body)
                })
                .ok_or_else(|| "GitHub shows no thread with the comment".to_owned())
        });
        let (thread, so_far, state) = match found {
            Ok(thread) => (Some(thread.id.clone()), Vec::new(), HandOffState::Queued),
            Err(reason) => (None, Vec::new(), HandOffState::Failed(reason)),
        };
        self.push(sent, thread, so_far, state);
    }

    /// The reply joined a thread GitHub already had, so the thread is known and the agent gets what was said in it.
    pub(super) fn record_reply(
        &mut self,
        thread_id: &ThreadId,
        sent: SentComment<'_>,
        threads: Option<&Result<Vec<ReviewThread>, String>>,
    ) {
        let found = read_of(threads).and_then(|threads| {
            threads
                .iter()
                .find(|thread| &thread.id == thread_id)
                .ok_or_else(|| "GitHub shows no such thread".to_owned())
        });
        let mut sent = sent;
        let (so_far, state) = match found {
            Ok(thread) => {
                sent.place = place_of(thread);
                let said = if ends_with(thread, sent.body) {
                    thread
                        .comments
                        .split_last()
                        .map_or(&[][..], |(_, said)| said)
                } else {
                    &thread.comments[..]
                };
                let messages = said
                    .iter()
                    .map(|comment| ThreadMessage {
                        author: comment.author.clone(),
                        body: comment.body.clone(),
                    })
                    .collect();
                (messages, HandOffState::Queued)
            }
            Err(reason) => (Vec::new(), HandOffState::Failed(reason)),
        };
        self.push(&sent, Some(thread_id.clone()), so_far, state);
    }

    fn push(
        &mut self,
        sent: &SentComment<'_>,
        thread: Option<ThreadId>,
        thread_so_far: Vec<ThreadMessage>,
        state: HandOffState,
    ) {
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
            thread_so_far,
            state,
        });
    }

    fn holds(&self, thread: &ThreadId) -> bool {
        self.0
            .iter()
            .any(|hand_off| hand_off.thread.as_ref() == Some(thread))
    }
}

fn read_of(threads: Option<&Result<Vec<ReviewThread>, String>>) -> Result<&[ReviewThread], String> {
    match threads {
        Some(Ok(threads)) => Ok(threads),
        Some(Err(reason)) => Err(format!("the threads could not be read: {reason}")),
        None => Err("the threads were not read".to_owned()),
    }
}

fn ends_with(thread: &ReviewThread, body: &str) -> bool {
    thread
        .comments
        .last()
        .is_some_and(|comment| comment.body.trim() == body.trim())
}

fn place_of(thread: &ReviewThread) -> String {
    match thread.line {
        Some(line) if !thread.is_file_comment => format!("{} line {line}", thread.path),
        _ => thread.path.to_string(),
    }
}
