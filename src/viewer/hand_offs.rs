use std::fmt;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::file_path::FilePath;
use crate::pull_request::PullRequestUrl;

use super::comments::SendMode;
use super::ports::{GitHubError, ReviewThreadsPort, ReviewWritesPort};
use super::review::StepId;
use super::threads::{ReviewThread, ThreadId};
use super::tour_agent::{AgentStatus, SendOutcome, TourAgents};

/// Marks what the tour agent wrote on GitHub, where it shares the reviewer's account.
const ROBOT: &str = "🤖";

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

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("{0:?} is not a hand-off id like h0")]
pub struct NotAHandOffId(String);

impl std::str::FromStr for HandOffId {
    type Err = NotAHandOffId;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        text.strip_prefix('h')
            .unwrap_or(text)
            .parse()
            .ok()
            .map(Self)
            .ok_or_else(|| NotAHandOffId(text.to_owned()))
    }
}

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

impl HandOffState {
    /// What the reviewer reads under the comment.
    pub fn line(&self) -> String {
        match self {
            Self::Queued => "Waiting for the agent to be free".to_owned(),
            Self::Delivered => "Delivered to the agent".to_owned(),
            Self::Working => "The agent is working on it".to_owned(),
            Self::Answered => "The agent answered".to_owned(),
            Self::EndedWithoutAnswer => "The agent finished without answering".to_owned(),
            Self::Failed(reason) => format!("Hand-off failed: {reason}"),
        }
    }
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

    /// What the tour agent is typed: everything it needs to answer without asking back.
    fn message(&self, pr: &PullRequestUrl) -> String {
        let id = self.id;
        let mut lines = vec![
            format!("Reviewer comment {id} on {pr}"),
            format!("Place: {}", self.place),
        ];
        lines.extend(self.step.iter().map(|step| format!("Step: {step}")));
        lines.push(
            match self.mode {
                SendMode::AddToReview => "Sent: added to the pending review",
                SendMode::CommentNow => "Sent: posted at once",
            }
            .to_owned(),
        );
        if !self.thread_so_far.is_empty() {
            lines.push("Thread so far:".to_owned());
            lines.extend(
                self.thread_so_far
                    .iter()
                    .map(|said| format!("- {}: {}", said.author, said.body)),
            );
        }
        lines.push(format!("Comment: {}", self.comment));
        lines.push(format!(
            "Answer with `guided-review agent-reply {pr} {id} --body <text>`. If the comment shows the tour is wrong, \
             fix the tour file, run `guided-review check {pr}` until it is clean, then `guided-review open {pr}`. \
             Never change the PR's code."
        ));
        lines.join("\n")
    }
}

/// Sending the oldest queued hand-off to the tour agent, owned so it can run on a background thread.
pub struct DeliveryRequest {
    id: HandOffId,
    pr: PullRequestUrl,
    text: String,
}

impl DeliveryRequest {
    pub fn run(self, agents: &TourAgents) -> DeliveryOutcome {
        let how = match agents.send_if_idle(&self.pr, &self.text) {
            Ok(SendOutcome::Delivered) => Delivery::Delivered,
            Ok(SendOutcome::Refused(_)) => Delivery::Waiting,
            Ok(SendOutcome::NoAgent) => match agents.start(&self.pr, &self.pr) {
                Ok(()) => Delivery::AgentStarted,
                Err(error) => Delivery::Failed(format!("{error:#}")),
            },
            Err(error) => Delivery::Failed(format!("{error:#}")),
        };
        DeliveryOutcome { id: self.id, how }
    }
}

pub struct DeliveryOutcome {
    id: HandOffId,
    how: Delivery,
}

impl DeliveryOutcome {
    /// A tour agent was started, so there is a new terminal to follow.
    pub fn started_an_agent(&self) -> bool {
        matches!(self.how, Delivery::AgentStarted)
    }
}

enum Delivery {
    Delivered,
    /// A new tour agent was started; the hand-off is sent once it is idle.
    AgentStarted,
    /// The agent is not ready; the hand-off stays queued.
    Waiting,
    Failed(String),
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

    /// The oldest queued hand-off, unless one was delivered and its agent has not started on it:
    /// they go one at a time, in the order they were sent.
    pub(super) fn delivery_request(&self, pr: &PullRequestUrl) -> Option<DeliveryRequest> {
        if self.0.iter().any(|h| h.state == HandOffState::Delivered) {
            return None;
        }
        let next = self.0.iter().find(|h| h.state == HandOffState::Queued)?;
        Some(DeliveryRequest {
            id: next.id,
            pr: pr.clone(),
            text: next.message(pr),
        })
    }

    pub(super) fn apply_delivery(&mut self, outcome: DeliveryOutcome) {
        let state = match outcome.how {
            Delivery::Delivered => HandOffState::Delivered,
            Delivery::Waiting | Delivery::AgentStarted => return,
            Delivery::Failed(reason) => HandOffState::Failed(reason),
        };
        if let Some(hand_off) = self.0.iter_mut().find(|h| h.id == outcome.id) {
            hand_off.state = state;
        }
    }

    /// A delivered hand-off is working once the agent is seen working; when it then goes idle with no
    /// answer, it ended without one.
    pub(super) fn agent_status_is(&mut self, status: AgentStatus) {
        for hand_off in &mut self.0 {
            hand_off.state = match (&hand_off.state, status) {
                (HandOffState::Delivered, AgentStatus::Working) => HandOffState::Working,
                (HandOffState::Working, AgentStatus::Idle) => HandOffState::EndedWithoutAnswer,
                _ => continue,
            };
        }
    }

    /// The line under a thread: where its latest hand-off is. `None` when nothing was handed off in it.
    pub(super) fn line_for(&self, thread: &ThreadId) -> Option<Arc<str>> {
        self.0
            .iter()
            .rfind(|hand_off| hand_off.thread.as_ref() == Some(thread))
            .map(|hand_off| hand_off.state.line().into())
    }

    /// The agent's answer arrived, whatever the hand-off was doing before.
    pub fn answered(&mut self, id: HandOffId) -> Result<(), AgentReplyRefused> {
        let known = self.0.iter().map(|hand_off| hand_off.id).collect();
        let hand_off = self
            .0
            .iter_mut()
            .find(|hand_off| hand_off.id == id)
            .ok_or(AgentReplyRefused::Unknown { id, known })?;
        hand_off.state = HandOffState::Answered;
        Ok(())
    }

    pub fn agent_reply(
        &self,
        pr: &PullRequestUrl,
        id: HandOffId,
        body: &str,
    ) -> Result<AgentReplyRequest, AgentReplyRefused> {
        let hand_off = self
            .0
            .iter()
            .find(|hand_off| hand_off.id == id)
            .ok_or_else(|| AgentReplyRefused::Unknown {
                id,
                known: self.0.iter().map(|hand_off| hand_off.id).collect(),
            })?;
        let thread = match (&hand_off.thread, &hand_off.state) {
            (Some(thread), _) => thread.clone(),
            (None, HandOffState::Failed(reason)) => {
                return Err(AgentReplyRefused::NoThread {
                    id,
                    reason: reason.clone(),
                });
            }
            (None, _) => {
                return Err(AgentReplyRefused::NoThread {
                    id,
                    reason: "the thread was never found".to_owned(),
                });
            }
        };
        let said = body.trim().trim_start_matches(ROBOT).trim_start();
        if said.is_empty() {
            return Err(AgentReplyRefused::Empty);
        }
        Ok(AgentReplyRequest {
            pr: pr.clone(),
            thread,
            body: format!("{ROBOT} {said}"),
        })
    }

    fn holds(&self, thread: &ThreadId) -> bool {
        self.0
            .iter()
            .any(|hand_off| hand_off.thread.as_ref() == Some(thread))
    }
}

/// Why a reply from the tour agent is not posted, before GitHub is asked.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum AgentReplyRefused {
    #[error("no hand-off {id} on this PR; {}", known_ids(.known))]
    Unknown {
        id: HandOffId,
        known: Vec<HandOffId>,
    },
    #[error("hand-off {id} has no thread to reply in: {reason}")]
    NoThread { id: HandOffId, reason: String },
    #[error("the reply is empty")]
    Empty,
}

fn known_ids(known: &[HandOffId]) -> String {
    if known.is_empty() {
        return "there are none".to_owned();
    }
    let ids: Vec<String> = known.iter().map(ToString::to_string).collect();
    format!("the known ones are {}", ids.join(", "))
}

/// Where an agent reply ended up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplyPlacement {
    /// In the reviewer's pending review, which goes out when they send it.
    JoinedPendingReview,
    PostedAtOnce,
}

/// The tour agent's answer to one hand-off, owned so it can run off the UI thread.
#[derive(Debug)]
pub struct AgentReplyRequest {
    pr: PullRequestUrl,
    thread: ThreadId,
    body: String,
}

impl AgentReplyRequest {
    /// Joins the reviewer's pending review when there is one and never sends it; with none the
    /// reply is posted at once.
    pub fn run(
        &self,
        github: &(impl ReviewThreadsPort + ReviewWritesPort + ?Sized),
    ) -> Result<ReplyPlacement, GitHubError> {
        let placement = if github.has_pending_review(&self.pr)? {
            ReplyPlacement::JoinedPendingReview
        } else {
            ReplyPlacement::PostedAtOnce
        };
        let mode = match placement {
            ReplyPlacement::JoinedPendingReview => SendMode::AddToReview,
            ReplyPlacement::PostedAtOnce => SendMode::CommentNow,
        };
        github.reply_to_thread(&self.pr, &self.thread, &self.body, mode)?;
        Ok(placement)
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
