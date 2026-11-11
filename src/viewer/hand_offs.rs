use std::fmt;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::file_path::FilePath;
use crate::pull_request::PullRequestUrl;

use super::comments::SendMode;
use super::ports::{GitHubError, ReviewThreadsPort, ReviewWritesPort};
use super::reactions::{Reaction, ReactionChange};
use super::review::StepId;
use super::threads::{CommentId, ReviewThread, ThreadId};
use super::tour_agent::{AgentStatus, SendOutcome, TourAgents};

/// Marks what the tour agent wrote on GitHub, where it shares the reviewer's account.
const ROBOT: &str = "🤖";

const WORKING_ON_IT: &str = "Working on it…";

/// Who a comment is for. The plain send actions are for the PR's author; only Ask agent also
/// reaches the tour agent.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Audience {
    #[default]
    Author,
    Agent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
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
    /// The agent's reply on GitHub says it is working on it; its answer replaces that reply.
    WorkingOnIt {
        reply: CommentId,
    },
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
            Self::WorkingOnIt { .. } => "The agent said it is working on it".to_owned(),
            Self::Answered => "The agent answered".to_owned(),
            Self::EndedWithoutAnswer => "The agent finished without answering".to_owned(),
            Self::Failed(reason) => format!("Hand-off failed: {reason}"),
        }
    }
}

/// The 👀 on the reviewer's comment, which says on GitHub that the tour agent has it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SeenMark {
    #[default]
    Off,
    On,
    NotPut(String),
    NotTakenOff(String),
}

impl SeenMark {
    fn trouble(&self) -> Option<String> {
        match self {
            Self::NotPut(reason) => Some(format!("could not put 👀 on the comment: {reason}")),
            Self::NotTakenOff(reason) => {
                Some(format!("could not take 👀 off the comment: {reason}"))
            }
            Self::Off | Self::On => None,
        }
    }

    fn put_on(github: &(impl ReviewThreadsPort + ?Sized), comment: &CommentId) -> Self {
        match github.set_reaction(comment, Reaction::Eyes, ReactionChange::Add) {
            Ok(()) => Self::On,
            Err(error) => Self::NotPut(format!("{error:#}")),
        }
    }

    fn take_off(github: &(impl ReviewThreadsPort + ?Sized), comment: &CommentId) -> Self {
        match github.set_reaction(comment, Reaction::Eyes, ReactionChange::Remove) {
            Ok(()) => Self::Off,
            Err(error) => Self::NotTakenOff(format!("{error:#}")),
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
    /// The comment on GitHub, which carries the seen mark. `None` when the threads read after the send did not show it.
    #[serde(default)]
    reviewer_comment: Option<CommentId>,
    #[serde(default)]
    seen_mark: SeenMark,
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

    pub fn seen_mark(&self) -> &SeenMark {
        &self.seen_mark
    }

    /// What the reviewer reads under the comment: where it is, and any trouble with its seen mark.
    fn line(&self) -> String {
        let state = self.state.line();
        match self.seen_mark.trouble() {
            Some(trouble) => format!("{state} · {trouble}"),
            None => state,
        }
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
            "Answer with `guided-review agent-reply {pr} {id} --body <text>`. If the comment asks for a change to the \
             PR's code, first run `guided-review agent-reply {pr} {id} --working`: it posts \"{WORKING_ON_IT}\" on the \
             thread, and your answer replaces it. Make the change in your folder when it is a clone of the PR's repo on \
             the PR's branch with none of the reviewer's own changes in it; else in a new git worktree, or a shallow \
             clone, of the PR's branch. Commit, push to the PR's branch, and name the commit in your answer. If the \
             comment shows the tour is wrong, fix the tour file, run `guided-review check {pr}` until it is clean, then \
             `guided-review open {pr}`."
        ));
        lines.join("\n")
    }
}

/// Sending the oldest queued hand-off to the tour agent, owned so it can run on a background thread.
pub struct DeliveryRequest {
    id: HandOffId,
    pr: PullRequestUrl,
    text: String,
    reviewer_comment: Option<CommentId>,
}

impl DeliveryRequest {
    /// A delivered hand-off puts the seen mark on the reviewer's comment.
    pub fn run(
        self,
        agents: &TourAgents,
        github: &(impl ReviewThreadsPort + ?Sized),
    ) -> DeliveryOutcome {
        let how = match agents.send_if_idle(&self.pr, &self.text) {
            Ok(SendOutcome::Delivered) => Delivery::Delivered,
            Ok(SendOutcome::Refused(_)) => Delivery::Waiting,
            Ok(SendOutcome::Starting) => Delivery::AgentStarting,
            Ok(SendOutcome::NoAgent) => match agents.start(&self.pr, &self.pr) {
                Ok(()) => Delivery::AgentStarted,
                Err(error) => Delivery::Failed(format!("{error:#}")),
            },
            Err(error) => Delivery::Failed(format!("{error:#}")),
        };
        let seen_mark = match (&how, &self.reviewer_comment) {
            (Delivery::Delivered, Some(comment)) => SeenMark::put_on(github, comment),
            _ => SeenMark::Off,
        };
        DeliveryOutcome {
            id: self.id,
            how,
            seen_mark,
        }
    }
}

pub struct DeliveryOutcome {
    id: HandOffId,
    how: Delivery,
    seen_mark: SeenMark,
}

impl DeliveryOutcome {
    /// A tour agent was started, so there is a new terminal to follow.
    pub fn started_an_agent(&self) -> bool {
        matches!(self.how, Delivery::AgentStarted)
    }

    /// The agent was just started and took nothing yet; no status change may come to try again on.
    pub fn wants_another_try_soon(&self) -> bool {
        matches!(self.how, Delivery::AgentStarting)
    }
}

enum Delivery {
    Delivered,
    /// A new tour agent was started; the hand-off is sent once it is idle.
    AgentStarted,
    /// The agent is not ready; the hand-off stays queued.
    Waiting,
    /// The agent's screen has not settled since it started; the hand-off stays queued.
    AgentStarting,
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
        let (thread, comment, state) = match found {
            Ok(thread) => (
                Some(thread.id.clone()),
                last_comment(thread),
                HandOffState::Queued,
            ),
            Err(reason) => (None, None, HandOffState::Failed(reason)),
        };
        self.push(sent, thread, comment, Vec::new(), state);
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
        let mut comment = None;
        let (so_far, state) = match found {
            Ok(thread) => {
                sent.place = place_of(thread);
                let said = if ends_with(thread, sent.body) {
                    comment = last_comment(thread);
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
        self.push(&sent, Some(thread_id.clone()), comment, so_far, state);
    }

    fn push(
        &mut self,
        sent: &SentComment<'_>,
        thread: Option<ThreadId>,
        reviewer_comment: Option<CommentId>,
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
            reviewer_comment,
            seen_mark: SeenMark::Off,
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
            reviewer_comment: next.reviewer_comment.clone(),
        })
    }

    pub(super) fn apply_delivery(&mut self, outcome: DeliveryOutcome) {
        let state = match outcome.how {
            Delivery::Delivered => HandOffState::Delivered,
            Delivery::Waiting | Delivery::AgentStarting | Delivery::AgentStarted => return,
            Delivery::Failed(reason) => HandOffState::Failed(reason),
        };
        if let Some(hand_off) = self.0.iter_mut().find(|h| h.id == outcome.id) {
            hand_off.state = state;
            hand_off.seen_mark = outcome.seen_mark;
        }
    }

    /// A delivered hand-off is working once the agent is seen working; when it then goes idle with no
    /// answer, it ended without one.
    pub(super) fn agent_status_is(&mut self, status: AgentStatus) {
        for hand_off in &mut self.0 {
            hand_off.state = match (&hand_off.state, status) {
                (HandOffState::Delivered, AgentStatus::Working) => HandOffState::Working,
                (HandOffState::Working | HandOffState::WorkingOnIt { .. }, AgentStatus::Idle) => {
                    HandOffState::EndedWithoutAnswer
                }
                _ => continue,
            };
        }
    }

    /// The line under a thread: where its latest hand-off is. `None` when nothing was handed off in it.
    pub(super) fn line_for(&self, thread: &ThreadId) -> Option<Arc<str>> {
        self.0
            .iter()
            .rfind(|hand_off| hand_off.thread.as_ref() == Some(thread))
            .map(|hand_off| hand_off.line().into())
    }

    /// Hand-offs the agent ended without answering whose comment still shows the seen mark.
    pub(super) fn seen_marks_to_take_off(&self) -> Vec<SeenMarkOff> {
        self.0
            .iter()
            .filter(|hand_off| {
                hand_off.state == HandOffState::EndedWithoutAnswer
                    && hand_off.seen_mark == SeenMark::On
            })
            .filter_map(|hand_off| {
                Some(SeenMarkOff {
                    id: hand_off.id,
                    comment: hand_off.reviewer_comment.clone()?,
                })
            })
            .collect()
    }

    pub(super) fn apply_seen_mark_off(&mut self, outcome: SeenMarkOffOutcome) {
        if let Some(hand_off) = self.0.iter_mut().find(|h| h.id == outcome.id) {
            hand_off.seen_mark = outcome.seen_mark;
        }
    }

    /// The thread's latest hand-off, when it failed and so can be tried again.
    /// The agent's "Working on it…" reply, while the line under the thread says so.
    pub(super) fn working_on_it_in(&self, thread: &ThreadId) -> Option<&CommentId> {
        self.0
            .iter()
            .rfind(|hand_off| hand_off.thread.as_ref() == Some(thread))
            .and_then(|hand_off| match &hand_off.state {
                HandOffState::WorkingOnIt { reply } => Some(reply),
                _ => None,
            })
    }

    pub(super) fn failed_in(&self, thread: &ThreadId) -> Option<HandOffId> {
        self.0
            .iter()
            .rfind(|hand_off| hand_off.thread.as_ref() == Some(thread))
            .filter(|hand_off| matches!(hand_off.state, HandOffState::Failed(_)))
            .map(|hand_off| hand_off.id)
    }

    /// Queues a failed hand-off again. One with no thread cannot go: the agent would have nowhere to answer.
    pub(super) fn try_again(&mut self, id: HandOffId) -> anyhow::Result<()> {
        let hand_off = self
            .0
            .iter_mut()
            .find(|hand_off| hand_off.id == id)
            .ok_or_else(|| anyhow::anyhow!("no hand-off {id} on this PR"))?;
        match (&hand_off.state, &hand_off.thread) {
            (HandOffState::Failed(_), Some(_)) => {
                hand_off.state = HandOffState::Queued;
                Ok(())
            }
            (HandOffState::Failed(reason), None) => {
                anyhow::bail!("hand-off {id} has no thread to answer in: {reason}")
            }
            _ => Ok(()),
        }
    }

    /// The agent's reply is on GitHub, whatever the hand-off was doing before.
    pub fn replied(
        &mut self,
        id: HandOffId,
        replied: &AgentReplied,
    ) -> Result<(), AgentReplyRefused> {
        let known = self.0.iter().map(|hand_off| hand_off.id).collect();
        let hand_off = self
            .0
            .iter_mut()
            .find(|hand_off| hand_off.id == id)
            .ok_or(AgentReplyRefused::Unknown { id, known })?;
        hand_off.state = match &replied.working_on_it {
            Some(reply) => HandOffState::WorkingOnIt {
                reply: reply.clone(),
            },
            None => HandOffState::Answered,
        };
        hand_off.seen_mark = replied.seen_mark.clone();
        Ok(())
    }

    /// The agent's answer. It replaces the agent's "Working on it…" reply when there is one.
    pub fn agent_reply(
        &self,
        pr: &PullRequestUrl,
        id: HandOffId,
        body: &str,
    ) -> Result<AgentReplyRequest, AgentReplyRefused> {
        let said = body.trim().trim_start_matches(ROBOT).trim_start();
        if said.is_empty() {
            return Err(AgentReplyRefused::Empty);
        }
        let hand_off = self.find(id)?;
        let replacing = match &hand_off.state {
            HandOffState::WorkingOnIt { reply } => Some(reply.clone()),
            _ => None,
        };
        Self::reply_request(
            pr,
            hand_off,
            AgentReplyKind::Answer {
                body: format!("{ROBOT} {said}"),
                replacing,
            },
        )
    }

    /// The agent's "Working on it…" reply, which its answer replaces later.
    pub fn agent_working_on_it(
        &self,
        pr: &PullRequestUrl,
        id: HandOffId,
    ) -> Result<AgentReplyRequest, AgentReplyRefused> {
        let hand_off = self.find(id)?;
        if matches!(hand_off.state, HandOffState::WorkingOnIt { .. }) {
            return Err(AgentReplyRefused::AlreadyWorkingOnIt { id });
        }
        Self::reply_request(pr, hand_off, AgentReplyKind::WorkingOnIt)
    }

    fn find(&self, id: HandOffId) -> Result<&HandOff, AgentReplyRefused> {
        self.0
            .iter()
            .find(|hand_off| hand_off.id == id)
            .ok_or_else(|| AgentReplyRefused::Unknown {
                id,
                known: self.0.iter().map(|hand_off| hand_off.id).collect(),
            })
    }

    fn reply_request(
        pr: &PullRequestUrl,
        hand_off: &HandOff,
        kind: AgentReplyKind,
    ) -> Result<AgentReplyRequest, AgentReplyRefused> {
        let id = hand_off.id;
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
        let seen_mark_on = hand_off
            .reviewer_comment
            .clone()
            .filter(|_| hand_off.seen_mark == SeenMark::On);
        Ok(AgentReplyRequest {
            pr: pr.clone(),
            thread,
            kind,
            seen_mark: hand_off.seen_mark.clone(),
            seen_mark_on,
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
    #[error(
        "hand-off {id} has its \"{WORKING_ON_IT}\" reply already; answer with --body, which replaces it"
    )]
    AlreadyWorkingOnIt { id: HandOffId },
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
    /// In place of the agent's "Working on it…" reply, wherever that went.
    ReplacedWorkingOnIt,
}

/// What an agent reply changed, for the hand-off to take on.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentReplied {
    /// The agent's "Working on it…" reply, when that is what it posted.
    working_on_it: Option<CommentId>,
    seen_mark: SeenMark,
}

#[derive(Debug)]
pub struct AgentReplyDone {
    placement: ReplyPlacement,
    replied: AgentReplied,
}

impl AgentReplyDone {
    pub fn placement(&self) -> ReplyPlacement {
        self.placement
    }

    pub fn replied(&self) -> &AgentReplied {
        &self.replied
    }
}

#[derive(Debug)]
enum AgentReplyKind {
    Answer {
        body: String,
        replacing: Option<CommentId>,
    },
    WorkingOnIt,
}

/// The tour agent's reply to one hand-off, owned so it can run off the UI thread.
#[derive(Debug)]
pub struct AgentReplyRequest {
    pr: PullRequestUrl,
    thread: ThreadId,
    kind: AgentReplyKind,
    seen_mark: SeenMark,
    /// The reviewer's comment, when it shows the seen mark: the reply takes it off.
    seen_mark_on: Option<CommentId>,
}

impl AgentReplyRequest {
    pub fn run(
        &self,
        github: &(impl ReviewThreadsPort + ReviewWritesPort + ?Sized),
    ) -> Result<AgentReplyDone, GitHubError> {
        let (placement, working_on_it) = match &self.kind {
            AgentReplyKind::Answer {
                body,
                replacing: Some(reply),
            } => {
                github.edit_comment(reply, body)?;
                (ReplyPlacement::ReplacedWorkingOnIt, None)
            }
            AgentReplyKind::Answer {
                body,
                replacing: None,
            } => (self.post(github, body)?.0, None),
            AgentReplyKind::WorkingOnIt => {
                let (placement, reply) = self.post(github, &format!("{ROBOT} {WORKING_ON_IT}"))?;
                (placement, Some(reply))
            }
        };
        let seen_mark = match &self.seen_mark_on {
            Some(comment) => SeenMark::take_off(github, comment),
            None => self.seen_mark.clone(),
        };
        Ok(AgentReplyDone {
            placement,
            replied: AgentReplied {
                working_on_it,
                seen_mark,
            },
        })
    }

    /// Joins the reviewer's pending review when there is one and never sends it; with none the
    /// reply is posted at once.
    fn post(
        &self,
        github: &(impl ReviewThreadsPort + ReviewWritesPort + ?Sized),
        body: &str,
    ) -> Result<(ReplyPlacement, CommentId), GitHubError> {
        let (placement, mode) = if github.has_pending_review(&self.pr)? {
            (ReplyPlacement::JoinedPendingReview, SendMode::AddToReview)
        } else {
            (ReplyPlacement::PostedAtOnce, SendMode::CommentNow)
        };
        let reply = github.reply_to_thread(&self.pr, &self.thread, body, mode)?;
        Ok((placement, reply))
    }
}

/// Taking the seen mark off a hand-off the agent ended without answering, owned so it can run off the UI thread.
pub struct SeenMarkOff {
    id: HandOffId,
    comment: CommentId,
}

pub struct SeenMarkOffOutcome {
    id: HandOffId,
    seen_mark: SeenMark,
}

impl SeenMarkOff {
    pub fn run(self, github: &(impl ReviewThreadsPort + ?Sized)) -> SeenMarkOffOutcome {
        SeenMarkOffOutcome {
            id: self.id,
            seen_mark: SeenMark::take_off(github, &self.comment),
        }
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

fn last_comment(thread: &ReviewThread) -> Option<CommentId> {
    thread.comments.last().map(|comment| comment.id.clone())
}

fn place_of(thread: &ReviewThread) -> String {
    match thread.line {
        Some(line) if !thread.is_file_comment => format!("{} line {line}", thread.path),
        _ => thread.path.to_string(),
    }
}
