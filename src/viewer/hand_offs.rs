use std::fmt;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::file_path::FilePath;
use crate::pull_request::PullRequestUrl;
use crate::review_subject::{LocalChangeSet, ReviewSubject};

use super::local_threads::{LocalAuthor, LocalThreadStore, ThreadHome};

use super::comments::SendMode;
use super::ports::{GitHubError, ReviewThreadsPort, ReviewWritesPort};
use super::reactions::{Reaction, ReactionChange};
use super::review::StepId;
use super::threads::{AGENT_MARK, CommentId, ReviewThread, ThreadId};
use super::tour_agent::{AgentStatus, SendOutcome, TourAgents, TourKind};

/// The word as a shell reads it as it is, or in single quotes when a character in it would split or expand.
pub(super) fn shell_word(text: &str) -> String {
    let plain = |c: char| c.is_ascii_alphanumeric() || "_./:@%+=,-".contains(c);
    if !text.is_empty() && text.chars().all(plain) {
        text.to_owned()
    } else {
        format!("'{}'", text.replace('\'', "'\\''"))
    }
}

const WORKING_ON_IT: &str = "Working on it…";

const LOCAL_REVIEW_NOTE: &str = "This is a local review: the reviewer reads your branch and its \
     uncommitted files on this machine, and there is no PR.";

/// The tasks one Send tasks sent, in reading order, as one message: the agent sees the whole
/// review at once and answers each task on its own thread.
fn tasks_message(
    tasks: &[&HandOff],
    subject: &ReviewSubject,
    change_set: &LocalChangeSet,
) -> String {
    let folder = change_set.folder().as_path().display();
    let branch = change_set.branch().as_str();
    let key = shell_word(&subject.to_string());
    let ids: Vec<String> = tasks.iter().map(|task| task.id.to_string()).collect();
    let mut lines = vec![
        format!(
            "Review tasks {} on branch {branch} in {folder}",
            ids.join(", ")
        ),
        LOCAL_REVIEW_NOTE.to_owned(),
        "The reviewer finished reading and sends these tasks together, in reading order."
            .to_owned(),
    ];
    for task in tasks {
        let id = task.id;
        lines.push(String::new());
        lines.push(format!("Task {id}"));
        lines.extend(task.task_lines());
        lines.push(format!(
            "Answer: `guided-review agent-reply {key} {id} --body -`, after \
             `guided-review agent-reply {key} {id} --working` if it asks for a change."
        ));
    }
    lines.push(String::new());
    lines.push(format!(
        "Make the changes in {folder}, on {branch}. Commit only if you normally would, and do not \
         push for the review: the reviewer sees what changed on disk and updates. Answer each task \
         on its own, with its own id. `--working` says \"{WORKING_ON_IT}\" on the task's thread, \
         and your answer replaces it. Give each answer on stdin, in a quoted heredoc (`<<'EOF'`): \
         short paragraphs, a list for several points, code names in backticks. Read the reviewed \
         change with `guided-review outline {key}` and `guided-review show {key} <piece \
         ids> --around`."
    ));
    lines.join("\n")
}

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
    /// A local review's task added to tasks: it goes to the agent with the others at Send tasks.
    WaitingForSendTasks,
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
    /// Nothing was typed: the agent does not report its state, so it may be waiting on a question the
    /// hand-off would answer. The reviewer looks at it, then sends anyway.
    AgentUnreported,
    /// The reviewer sends it to an agent that does not report its state, without waiting for it to be idle.
    SendAnyway,
}

/// What the reviewer can do about a thread's latest hand-off from its line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HandOffButton {
    /// It failed; it is queued again.
    TryAgain(HandOffId),
    /// Its agent does not report its state; it goes without waiting for the agent to be idle.
    SendAnyway(HandOffId),
}

impl HandOffButton {
    pub fn label(self) -> &'static str {
        match self {
            Self::TryAgain(_) => "Try again",
            Self::SendAnyway(_) => "Send anyway",
        }
    }
}

impl HandOffState {
    pub fn is_working(&self) -> bool {
        matches!(self, Self::Working | Self::WorkingOnIt { .. })
    }

    /// What the reviewer reads under the comment.
    pub fn line(&self) -> String {
        match self {
            Self::WaitingForSendTasks => {
                "Queued task: goes to the agent with Send tasks".to_owned()
            }
            Self::Queued => "Waiting for the agent to be free".to_owned(),
            Self::Delivered => "Delivered to the agent".to_owned(),
            Self::Working => "The agent is working on it".to_owned(),
            Self::WorkingOnIt { .. } => "The agent said it is working on it".to_owned(),
            Self::Answered => "The agent answered".to_owned(),
            Self::EndedWithoutAnswer => "The agent finished without answering".to_owned(),
            Self::Failed(reason) => format!("Hand-off failed: {reason}"),
            Self::AgentUnreported => "The agent does not report its state: look at it first, it may be waiting on a question this task answers".to_owned(),
            Self::SendAnyway => "Sending to the agent anyway".to_owned(),
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
    /// The file screen of a basic tour the comment was sent from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    file: Option<FilePath>,
    /// The lines the comment is on, as the diff shows them; only a task of a local review carries them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    code: Option<String>,
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
    /// The tasks one Send tasks sent: they reach the agent together, as one message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    batch: Option<TaskBatch>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct TaskBatch(u32);

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

    /// The step of a guided tour it was sent from.
    pub(super) fn step(&self) -> Option<&StepId> {
        self.step.as_ref()
    }

    /// Takes the comment's text and what came before it in its thread as `threads` have them now,
    /// since the reviewer may have edited a queued task. `None` when they do not show the comment.
    fn as_in<'a>(&mut self, threads: &'a [ReviewThread]) -> Option<&'a ReviewThread> {
        let thread = threads
            .iter()
            .find(|thread| Some(&thread.id) == self.thread.as_ref())?;
        let at = thread
            .comments
            .iter()
            .position(|said| Some(&said.id) == self.reviewer_comment.as_ref())?;
        let (before, from) = thread.comments.split_at_checked(at)?;
        let comment = from.first()?;
        self.comment.clone_from(&comment.body);
        self.thread_so_far = before
            .iter()
            .map(|said| ThreadMessage {
                author: said.author.clone(),
                body: said.body.clone(),
            })
            .collect();
        Some(thread)
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
    fn message(&self, subject: &ReviewSubject) -> String {
        match subject {
            ReviewSubject::PullRequest(_) => self.pull_request_message(subject),
            ReviewSubject::Local(change_set) => self.task_message(subject, change_set),
        }
    }

    /// A task of a local review, for the change set's agent, which wrote the code in its own folder.
    fn task_message(&self, subject: &ReviewSubject, change_set: &LocalChangeSet) -> String {
        let id = self.id;
        let folder = change_set.folder().as_path().display();
        let branch = change_set.branch().as_str();
        let key = shell_word(&subject.to_string());
        let mut lines = vec![
            format!("Review task {id} on branch {branch} in {folder}"),
            LOCAL_REVIEW_NOTE.to_owned(),
        ];
        lines.extend(self.task_lines());
        lines.push(format!(
            "Make the change in {folder}, on {branch}. Commit only if you normally would, and do not push \
             for the review: the reviewer sees what changed on disk and updates. If the task asks for a \
             change, first run `guided-review agent-reply {key} {id} --working`: it says \
             \"{WORKING_ON_IT}\" on the task's thread, and your answer replaces it. Answer with \
             `guided-review agent-reply {key} {id} --body -` and the answer on stdin, in a quoted \
             heredoc (`<<'EOF'`): short paragraphs, a list for several points, code names in \
             backticks. Read the reviewed change with `guided-review outline {key}` and \
             `guided-review show {key} <piece ids> --around`."
        ));
        lines.join("\n")
    }

    /// Where the task is and what it asks, without how to answer it.
    fn task_lines(&self) -> Vec<String> {
        let mut lines = vec![format!("Place: {}", self.place)];
        lines.extend(self.file.iter().map(|file| format!("File: {file}")));
        if let Some(code) = &self.code {
            lines.push(format!("Code:\n{}", code.trim_end()));
        }
        if !self.thread_so_far.is_empty() {
            lines.push("Thread so far:".to_owned());
            lines.extend(
                self.thread_so_far
                    .iter()
                    .map(|said| format!("- {}: {}", said.author, said.body)),
            );
        }
        lines.push(format!("Task: {}", self.comment));
        lines
    }

    fn pull_request_message(&self, subject: &ReviewSubject) -> String {
        let id = self.id;
        let mut lines = vec![
            format!("Reviewer comment {id} on {subject}"),
            format!("Place: {}", self.place),
        ];
        lines.extend(self.step.iter().map(|step| format!("Step: {step}")));
        lines.extend(self.file.iter().map(|file| format!("File: {file}")));
        lines.push(
            match self.mode {
                SendMode::AddToReview => "Sent: added to the pending review",
                SendMode::CommentNow | SendMode::SendNow | SendMode::AddToTasks => {
                    "Sent: posted at once"
                }
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
        let tour_note = if self.file.is_some() {
            format!(
                "The tour is a basic tour: there is no tour file to fix. Read the changed code with \
                 `guided-review outline {subject}` and `guided-review show {subject} <piece ids> --around`."
            )
        } else {
            format!(
                "If the comment shows the tour is wrong, fix the tour file, run `guided-review check {subject}` until it \
                 is clean, then `guided-review open {subject}`."
            )
        };
        lines.push(format!(
            "Answer with `guided-review agent-reply {subject} {id} --body -` and the answer on stdin, in a quoted heredoc \
             (`<<'EOF'`). Write it in GitHub markdown: answer the question in the first sentence, then at most 3 bullets, a \
             blank line between paragraphs, code names in backticks. If the comment asks for a change to the \
             PR's code, first run `guided-review agent-reply {subject} {id} --working`: it posts \"{WORKING_ON_IT}\" on the \
             thread, and your answer replaces it. Make the change in your folder when it is a clone of the PR's repo on \
             the PR's branch with none of the reviewer's own changes in it; else in a new git worktree, or a shallow \
             clone, of the PR's branch. Commit, push to the PR's branch, and name the commit in your answer. {tour_note}"
        ));
        lines.join("\n")
    }
}

/// Sending the oldest queued hand-off to the tour agent, owned so it can run on a background thread.
pub(super) struct DeliveryRequest {
    /// One hand-off, or the tasks of one Send tasks, which go as one message.
    ids: Vec<HandOffId>,
    subject: ReviewSubject,
    text: String,
    /// What a tour agent started for this hand-off has to read.
    tour: TourKind,
    reviewer_comment: Option<CommentId>,
    /// The reviewer chose to send it without waiting for the agent to be idle.
    anyway: bool,
}

impl DeliveryRequest {
    /// A delivered hand-off puts the seen mark on the reviewer's comment.
    pub(super) fn run(
        self,
        agents: &TourAgents,
        github: &(impl ReviewThreadsPort + ?Sized),
    ) -> DeliveryOutcome {
        let sent = if self.anyway {
            agents.send_anyway(&self.subject, &self.text)
        } else {
            agents.send_if_idle(&self.subject, &self.text)
        };
        let how = match sent {
            Ok(SendOutcome::Delivered) => Delivery::Delivered,
            Ok(SendOutcome::Unreported(_)) => Delivery::AgentUnreported,
            Ok(SendOutcome::Refused(_)) => Delivery::Waiting,
            Ok(SendOutcome::Starting) => Delivery::AgentStarting,
            Ok(SendOutcome::NoAgent) => match agents.start(&self.subject, &self.subject, self.tour)
            {
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
            ids: self.ids,
            how,
            seen_mark,
        }
    }
}

pub(super) struct DeliveryOutcome {
    ids: Vec<HandOffId>,
    how: Delivery,
    seen_mark: SeenMark,
}

impl DeliveryOutcome {
    /// A tour agent was started, so there is a new terminal to follow.
    pub(super) fn started_an_agent(&self) -> bool {
        matches!(self.how, Delivery::AgentStarted)
    }

    /// The agent was just started and took nothing yet; no status change may come to try again on.
    pub(super) fn wants_another_try_soon(&self) -> bool {
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
    /// The agent does not report its state; the hand-off waits for the reviewer to send it anyway.
    AgentUnreported,
    Failed(String),
}

/// The screen a comment was sent from, as far as the tour agent needs to know it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum SentFrom {
    Step(StepId),
    /// A basic tour's file screen.
    File(FilePath),
    Elsewhere,
}

/// What a hand-off is made from: the comment as GitHub has it now, in the threads read right after the send.
pub(super) struct SentComment<'a> {
    pub place: String,
    pub from: SentFrom,
    pub mode: SendMode,
    pub body: &'a str,
    /// The lines the comment is on, for a task of a local review.
    pub code: Option<String>,
}

/// The hand-offs of one PR, oldest first. Kept in the review state.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandOffs(Vec<HandOff>);

impl HandOffs {
    pub fn all(&self) -> &[HandOff] {
        &self.0
    }

    pub(super) fn waiting_for_send_tasks(&self) -> impl Iterator<Item = &HandOff> {
        self.0
            .iter()
            .filter(|hand_off| hand_off.state == HandOffState::WaitingForSendTasks)
    }

    /// Send tasks: each queued task, with its text and thread as `threads` have them now, goes to
    /// the agent with the others, ordered by `reading_place`. A task whose comment `threads` do not
    /// show stays queued: they may be older than the task.
    pub(super) fn send_queued_tasks<Place: Ord>(
        &mut self,
        threads: &[ReviewThread],
        reading_place: impl Fn(&HandOff, &ReviewThread) -> Place,
    ) {
        let batch = TaskBatch(
            self.0
                .iter()
                .filter_map(|hand_off| hand_off.batch)
                .map(|batch| batch.0 + 1)
                .max()
                .unwrap_or(0),
        );
        let (queued, others): (Vec<HandOff>, Vec<HandOff>) = std::mem::take(&mut self.0)
            .into_iter()
            .partition(|hand_off| hand_off.state == HandOffState::WaitingForSendTasks);
        self.0 = others;
        let mut sent = Vec::new();
        for mut task in queued {
            match task.as_in(threads) {
                Some(thread) => {
                    task.state = HandOffState::Queued;
                    task.batch = Some(batch);
                    sent.push((reading_place(&task, thread), task));
                }
                None => self.0.push(task),
            }
        }
        sent.sort_by(|(one, _), (other, _)| one.cmp(other));
        self.0.extend(sent.into_iter().map(|(_, task)| task));
    }

    /// The reviewer deleted the comment of a queued task, so there is no task left to send.
    pub(super) fn forget_queued_task(&mut self, comment: &CommentId) {
        self.0.retain(|hand_off| {
            hand_off.state != HandOffState::WaitingForSendTasks
                || hand_off.reviewer_comment.as_ref() != Some(comment)
        });
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
        let state = match state {
            HandOffState::Queued if sent.mode == SendMode::AddToTasks => {
                HandOffState::WaitingForSendTasks
            }
            other => other,
        };
        self.0.push(HandOff {
            id,
            place: sent.place.clone(),
            step: match &sent.from {
                SentFrom::Step(step) => Some(step.clone()),
                SentFrom::File(_) | SentFrom::Elsewhere => None,
            },
            file: match &sent.from {
                SentFrom::File(path) => Some(path.clone()),
                SentFrom::Step(_) | SentFrom::Elsewhere => None,
            },
            code: sent.code.clone(),
            mode: sent.mode,
            comment: sent.body.to_owned(),
            thread,
            thread_so_far,
            state,
            reviewer_comment,
            seen_mark: SeenMark::Off,
            batch: None,
        });
    }

    /// The oldest queued hand-off, unless one was delivered and its agent has not started on it:
    /// they go one at a time, in the order they were sent.
    pub(super) fn delivery_request(&self, subject: &ReviewSubject) -> Option<DeliveryRequest> {
        if self.0.iter().any(|h| h.state == HandOffState::Delivered) {
            return None;
        }
        let next = self
            .0
            .iter()
            .find(|h| matches!(h.state, HandOffState::Queued | HandOffState::SendAnyway))?;
        let (ids, text) = match (next.batch, subject) {
            (Some(batch), ReviewSubject::Local(change_set)) => {
                let tasks: Vec<&HandOff> = self
                    .0
                    .iter()
                    .filter(|h| h.state == next.state && h.batch == Some(batch))
                    .collect();
                (
                    tasks.iter().map(|task| task.id).collect(),
                    tasks_message(&tasks, subject, change_set),
                )
            }
            _ => (vec![next.id], next.message(subject)),
        };
        Some(DeliveryRequest {
            ids,
            subject: subject.clone(),
            text,
            tour: if next.file.is_some() {
                TourKind::Basic
            } else {
                TourKind::Guided
            },
            reviewer_comment: next
                .reviewer_comment
                .clone()
                .filter(|_| subject.has_pending_review()),
            anyway: next.state == HandOffState::SendAnyway,
        })
    }

    pub(super) fn apply_delivery(&mut self, outcome: DeliveryOutcome) {
        let state = match outcome.how {
            Delivery::Delivered => HandOffState::Delivered,
            Delivery::Waiting | Delivery::AgentStarting | Delivery::AgentStarted => return,
            Delivery::AgentUnreported => HandOffState::AgentUnreported,
            Delivery::Failed(reason) => HandOffState::Failed(reason),
        };
        for hand_off in self.0.iter_mut().filter(|h| outcome.ids.contains(&h.id)) {
            hand_off.state = state.clone();
            hand_off.seen_mark = outcome.seen_mark.clone();
        }
    }

    /// A delivered hand-off is working once the agent is seen working; when it then goes idle with no
    /// answer, it ended without one. One held back from an agent that did not report its state goes
    /// once the agent says it is idle.
    pub(super) fn agent_status_is(&mut self, status: AgentStatus) {
        for hand_off in &mut self.0 {
            hand_off.state = match (&hand_off.state, status) {
                (HandOffState::Delivered, AgentStatus::Working) => HandOffState::Working,
                (HandOffState::AgentUnreported, AgentStatus::Idle) => HandOffState::Queued,
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

    pub(super) fn is_working_on(&self, thread: &ThreadId) -> bool {
        self.0
            .iter()
            .rfind(|hand_off| hand_off.thread.as_ref() == Some(thread))
            .is_some_and(|hand_off| hand_off.state.is_working())
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

    pub(super) fn button_in(&self, thread: &ThreadId) -> Option<HandOffButton> {
        let latest = self
            .0
            .iter()
            .rfind(|hand_off| hand_off.thread.as_ref() == Some(thread))?;
        match latest.state {
            HandOffState::Failed(_) => Some(HandOffButton::TryAgain(latest.id)),
            HandOffState::AgentUnreported => Some(HandOffButton::SendAnyway(latest.id)),
            _ => None,
        }
    }

    /// Sends a hand-off held back from an agent that does not report its state, with the tasks sent
    /// together with it, without waiting for the agent to be idle.
    pub(super) fn send_anyway(&mut self, id: HandOffId) -> anyhow::Result<()> {
        let held = self
            .0
            .iter()
            .find(|hand_off| hand_off.id == id)
            .ok_or_else(|| anyhow::anyhow!("no hand-off {id} in this review"))?;
        if held.state != HandOffState::AgentUnreported {
            return Ok(());
        }
        let batch = held.batch;
        for hand_off in &mut self.0 {
            let sent_with_it = hand_off.id == id || (batch.is_some() && hand_off.batch == batch);
            if sent_with_it && hand_off.state == HandOffState::AgentUnreported {
                hand_off.state = HandOffState::SendAnyway;
            }
        }
        Ok(())
    }

    /// Queues a failed hand-off again. One with no thread cannot go: the agent would have nowhere to answer.
    /// The latest hand-off held back from an agent that does not report its state, as a delivery to such
    /// an agent leaves it, for a snapshot scene.
    #[cfg(feature = "snapshots")]
    pub(super) fn hold_back_latest(&mut self) {
        if let Some(latest) = self.0.last_mut() {
            latest.state = HandOffState::AgentUnreported;
        }
    }

    pub(super) fn try_again(&mut self, id: HandOffId) -> anyhow::Result<()> {
        let hand_off = self
            .0
            .iter_mut()
            .find(|hand_off| hand_off.id == id)
            .ok_or_else(|| anyhow::anyhow!("no hand-off {id} in this review"))?;
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
    pub(super) fn replied(
        &mut self,
        id: HandOffId,
        replied: &AgentReplied,
    ) -> Result<(), AgentReplyRefused> {
        let known = self.0.iter().map(|hand_off| hand_off.id).collect();
        let hand_off = self.0.iter_mut().find(|hand_off| hand_off.id == id).ok_or(
            AgentReplyRefused::Unknown {
                id,
                known,
                reviewed: Reviewed::Either,
            },
        )?;
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
        home: &ThreadHome,
        id: HandOffId,
        body: &str,
    ) -> Result<AgentReplyRequest, AgentReplyRefused> {
        let said = body.trim().trim_start_matches(AGENT_MARK).trim_start();
        if said.is_empty() {
            return Err(AgentReplyRefused::Empty);
        }
        let hand_off = self.find(home, id)?;
        let replacing = match &hand_off.state {
            HandOffState::WorkingOnIt { reply } => Some(reply.clone()),
            _ => None,
        };
        Self::reply_request(
            home,
            hand_off,
            AgentReplyKind::Answer {
                said: said.to_owned(),
                replacing,
            },
        )
    }

    /// The agent's "Working on it…" reply, which its answer replaces later.
    pub fn agent_working_on_it(
        &self,
        home: &ThreadHome,
        id: HandOffId,
    ) -> Result<AgentReplyRequest, AgentReplyRefused> {
        let hand_off = self.find(home, id)?;
        if matches!(hand_off.state, HandOffState::WorkingOnIt { .. }) {
            return Err(AgentReplyRefused::AlreadyWorkingOnIt { id });
        }
        Self::reply_request(home, hand_off, AgentReplyKind::WorkingOnIt)
    }

    fn find(&self, home: &ThreadHome, id: HandOffId) -> Result<&HandOff, AgentReplyRefused> {
        self.0
            .iter()
            .find(|hand_off| hand_off.id == id)
            .ok_or_else(|| AgentReplyRefused::Unknown {
                id,
                known: self.0.iter().map(|hand_off| hand_off.id).collect(),
                reviewed: Reviewed::from(home),
            })
    }

    fn reply_request(
        home: &ThreadHome,
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
            home: home.clone(),
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
    #[error("no hand-off {id} {reviewed}; {}", known_ids(.known))]
    Unknown {
        id: HandOffId,
        known: Vec<HandOffId>,
        reviewed: Reviewed,
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

/// Which kind of review a hand-off was looked for in, as an error says it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reviewed {
    PullRequest,
    Local,
    /// The caller has no subject at hand.
    Either,
}

impl From<&ThreadHome> for Reviewed {
    fn from(home: &ThreadHome) -> Self {
        match home {
            ThreadHome::GitHub(_) => Self::PullRequest,
            ThreadHome::StateFile(_) => Self::Local,
        }
    }
}

impl fmt::Display for Reviewed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::PullRequest => "on this PR",
            Self::Local => "in this local review",
            Self::Either => "in this review",
        })
    }
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
        /// What the agent said, without the robot GitHub needs in front of it.
        said: String,
        replacing: Option<CommentId>,
    },
    WorkingOnIt,
}

/// The tour agent's reply to one hand-off, owned so it can run off the UI thread.
#[derive(Debug)]
pub struct AgentReplyRequest {
    home: ThreadHome,
    thread: ThreadId,
    kind: AgentReplyKind,
    seen_mark: SeenMark,
    /// The reviewer's comment, when it shows the seen mark: the reply takes it off.
    seen_mark_on: Option<CommentId>,
}

impl AgentReplyRequest {
    /// The subject the reply is for: the PR, or the local review whose state file keeps the thread.
    pub fn home(&self) -> &ThreadHome {
        &self.home
    }

    pub fn run(
        &self,
        github: &(impl ReviewThreadsPort + ReviewWritesPort + ?Sized),
    ) -> Result<AgentReplyDone, GitHubError> {
        let (placement, working_on_it) = match &self.home {
            ThreadHome::GitHub(pr) => self.post_on_github(github, pr)?,
            ThreadHome::StateFile(store) => self.keep_locally(store).map_err(|error| {
                GitHubError::Other(error.context("could not write the local thread"))
            })?,
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

    fn post_on_github(
        &self,
        github: &(impl ReviewThreadsPort + ReviewWritesPort + ?Sized),
        pr: &PullRequestUrl,
    ) -> Result<(ReplyPlacement, Option<CommentId>), GitHubError> {
        Ok(match &self.kind {
            AgentReplyKind::Answer {
                said,
                replacing: Some(reply),
            } => {
                github.edit_comment(reply, &format!("{AGENT_MARK} {said}"))?;
                (ReplyPlacement::ReplacedWorkingOnIt, None)
            }
            AgentReplyKind::Answer {
                said,
                replacing: None,
            } => (
                self.post(github, pr, &format!("{AGENT_MARK} {said}"))?.0,
                None,
            ),
            AgentReplyKind::WorkingOnIt => {
                let (placement, reply) =
                    self.post(github, pr, &format!("{AGENT_MARK} {WORKING_ON_IT}"))?;
                (placement, Some(reply))
            }
        })
    }

    /// A local thread knows its authors, so the agent's text goes in as it is.
    fn keep_locally(
        &self,
        store: &LocalThreadStore,
    ) -> anyhow::Result<(ReplyPlacement, Option<CommentId>)> {
        Ok(match &self.kind {
            AgentReplyKind::Answer {
                said,
                replacing: Some(reply),
            } => {
                store.edit(reply, said)?;
                (ReplyPlacement::ReplacedWorkingOnIt, None)
            }
            AgentReplyKind::Answer {
                said,
                replacing: None,
            } => {
                store.reply(&self.thread, said, LocalAuthor::Agent)?;
                (ReplyPlacement::PostedAtOnce, None)
            }
            AgentReplyKind::WorkingOnIt => {
                let reply = store.reply(&self.thread, WORKING_ON_IT, LocalAuthor::Agent)?;
                (ReplyPlacement::PostedAtOnce, Some(reply))
            }
        })
    }

    /// Joins the reviewer's pending review when there is one and never sends it; with none the
    /// reply is posted at once.
    fn post(
        &self,
        github: &(impl ReviewThreadsPort + ReviewWritesPort + ?Sized),
        pr: &PullRequestUrl,
        body: &str,
    ) -> Result<(ReplyPlacement, CommentId), GitHubError> {
        let (placement, mode) = if github.has_pending_review(pr)? {
            (ReplyPlacement::JoinedPendingReview, SendMode::AddToReview)
        } else {
            (ReplyPlacement::PostedAtOnce, SendMode::CommentNow)
        };
        let reply = github.reply_to_thread(pr, &self.thread, body, mode)?;
        Ok((placement, reply))
    }
}

/// Taking the seen mark off a hand-off the agent ended without answering, owned so it can run off the UI thread.
pub(super) struct SeenMarkOff {
    id: HandOffId,
    comment: CommentId,
}

pub(super) struct SeenMarkOffOutcome {
    id: HandOffId,
    seen_mark: SeenMark,
}

impl SeenMarkOff {
    pub(super) fn run(self, github: &(impl ReviewThreadsPort + ?Sized)) -> SeenMarkOffOutcome {
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
