//! Ask why: the reviewer's questions about a finding and the review agent's answers, on a thread of the
//! finding's own. It lives in the review's state file for a PR too, since a PR's threads are GitHub's.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::findings::{Finding, FindingId};
use crate::review_subject::ReviewSubject;

use super::hand_offs::shell_word;
use super::review_agents::ReviewAgents;
use super::tour_agent::{AgentStatus, SendOutcome};

/// Who said a message on a finding's thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WhyAuthor {
    Reviewer,
    ReviewAgent,
    /// The tour agent, on a fix it was asked for.
    TourAgent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WhyMessage {
    pub author: WhyAuthor,
    pub body: String,
}

/// Where the latest question on a finding is with the review agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum WhyState {
    /// Waits for the review agent to be idle.
    Queued,
    Delivered,
    /// The review agent was seen working since the question went, or said it works on it.
    Working,
    Answered,
    EndedWithoutAnswer,
    Failed(String),
    /// The thread only holds what was said about a fix: no question was asked.
    NotAsked,
}

/// What the review agent says to a question on a finding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum WhyReply {
    Working,
    Answer(String),
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("{0} has no question to answer")]
pub struct NoQuestion(pub FindingId);

/// The thread under one finding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FindingThread {
    finding: Finding,
    messages: Vec<WhyMessage>,
    state: WhyState,
}

impl WhyState {
    /// The question is asked and not answered yet, so another would cross it.
    pub const fn is_in_progress(&self) -> bool {
        matches!(self, Self::Queued | Self::Delivered | Self::Working)
    }

    /// What the reviewer reads under the thread.
    pub fn line(&self) -> String {
        match self {
            Self::Queued => "Asked: waits for the review agent".to_owned(),
            Self::Delivered => "Asked: sent to the review agent".to_owned(),
            Self::Working => "The review agent is looking into it".to_owned(),
            Self::Answered => "The review agent answered".to_owned(),
            Self::EndedWithoutAnswer => "The review agent stopped without answering".to_owned(),
            Self::Failed(reason) => format!("Not asked: {reason}"),
            Self::NotAsked => String::new(),
        }
    }
}

impl FindingThread {
    pub fn messages(&self) -> &[WhyMessage] {
        &self.messages
    }

    pub const fn state(&self) -> &WhyState {
        &self.state
    }

    /// Where the latest question is, when one was asked.
    pub fn asked_line(&self) -> Option<String> {
        (self.state != WhyState::NotAsked).then(|| self.state.line())
    }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FindingThreads(Vec<FindingThread>);

impl FindingThreads {
    pub const fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// A re-pin moved the findings: a question not delivered yet tells the review agent their new place.
    pub(super) fn follow(&mut self, moved: &[Finding]) {
        for thread in &mut self.0 {
            thread.finding.follow(moved);
        }
    }

    pub fn thread_of(&self, id: &FindingId) -> Option<&FindingThread> {
        self.0.iter().find(|thread| thread.finding.id == *id)
    }

    /// What a snapshot scene shows of Ask why: an answered question on the first finding, one being looked
    /// into on the second.
    #[cfg(feature = "snapshots")]
    pub(super) fn staged(findings: &[Finding]) -> Self {
        let said = |author, body: &str| WhyMessage {
            author,
            body: body.to_owned(),
        };
        Self(
            findings
                .iter()
                .take(2)
                .enumerate()
                .map(|(position, finding)| match position {
                    0 => FindingThread {
                        finding: finding.clone(),
                        messages: vec![
                            said(WhyAuthor::Reviewer, "Why is this a finding? What did you see in the code?"),
                            said(
                                WhyAuthor::ReviewAgent,
                                "`items` comes from the request as is, and `first()` is unwrapped two lines down. An empty list panics there.",
                            ),
                        ],
                        state: WhyState::Answered,
                    },
                    _ => FindingThread {
                        finding: finding.clone(),
                        messages: vec![said(WhyAuthor::Reviewer, "Is the longer name not clearer?")],
                        state: WhyState::Working,
                    },
                })
                .collect(),
        )
    }

    /// Something said on the finding's thread that is no question: it starts the thread when there is none.
    pub(super) fn say(&mut self, finding: Finding, author: WhyAuthor, body: String) {
        let said = WhyMessage { author, body };
        match self
            .0
            .iter_mut()
            .find(|thread| thread.finding.id == finding.id)
        {
            Some(thread) => thread.messages.push(said),
            None => self.0.push(FindingThread {
                finding,
                messages: vec![said],
                state: WhyState::NotAsked,
            }),
        }
    }

    pub fn is_asking(&self, id: &FindingId) -> bool {
        self.thread_of(id)
            .is_some_and(|thread| thread.state.is_in_progress())
    }

    /// The question goes onto the finding's thread, which starts with this question when it is the first.
    pub(super) fn ask(&mut self, finding: Finding, question: &str) {
        let asked = WhyMessage {
            author: WhyAuthor::Reviewer,
            body: question.trim().to_owned(),
        };
        match self
            .0
            .iter_mut()
            .find(|thread| thread.finding.id == finding.id)
        {
            Some(thread) => {
                thread.finding = finding;
                thread.messages.push(asked);
                thread.state = WhyState::Queued;
            }
            None => self.0.push(FindingThread {
                finding,
                messages: vec![asked],
                state: WhyState::Queued,
            }),
        }
    }

    /// Every queued question as one message, unless one went and the agent has not started on it.
    pub(super) fn delivery_request(&self, subject: &ReviewSubject) -> Option<WhyDeliveryRequest> {
        if self
            .0
            .iter()
            .any(|thread| thread.state == WhyState::Delivered)
        {
            return None;
        }
        let queued: Vec<&FindingThread> = self
            .0
            .iter()
            .filter(|thread| thread.state == WhyState::Queued)
            .collect();
        if queued.is_empty() {
            return None;
        }
        Some(WhyDeliveryRequest {
            ids: queued
                .iter()
                .map(|thread| thread.finding.id.clone())
                .collect(),
            subject: subject.clone(),
            text: questions_message(&queued, subject),
        })
    }

    pub(super) fn apply_delivery(&mut self, outcome: WhyDeliveryOutcome) {
        let state = match outcome.how {
            WhyDelivery::Delivered => WhyState::Delivered,
            WhyDelivery::Waiting => return,
            WhyDelivery::Failed(reason) => WhyState::Failed(reason),
        };
        for thread in self
            .0
            .iter_mut()
            .filter(|thread| outcome.ids.contains(&thread.finding.id))
        {
            thread.state = state.clone();
        }
    }

    /// A delivered question is being worked on once the agent is seen working; when the agent then goes
    /// idle, a question it did not answer ended without an answer. Says whether any question moved.
    pub(super) fn agent_status_is(&mut self, status: AgentStatus) -> bool {
        let mut moved = false;
        for thread in &mut self.0 {
            thread.state = match (&thread.state, status) {
                (WhyState::Delivered, AgentStatus::Working) => WhyState::Working,
                (WhyState::Working, AgentStatus::Idle) => WhyState::EndedWithoutAnswer,
                _ => continue,
            };
            moved = true;
        }
        moved
    }

    /// The review agent ended: nothing it was asked can be answered any more. Says whether any question moved.
    pub(super) fn agent_gone(&mut self) -> bool {
        let mut moved = false;
        for thread in self
            .0
            .iter_mut()
            .filter(|thread| thread.state.is_in_progress())
        {
            thread.state = WhyState::EndedWithoutAnswer;
            moved = true;
        }
        moved
    }

    pub fn replied(&mut self, id: &FindingId, reply: &WhyReply) -> Result<(), NoQuestion> {
        let thread = self
            .0
            .iter_mut()
            .find(|thread| thread.finding.id == *id)
            .ok_or_else(|| NoQuestion(id.clone()))?;
        match reply {
            WhyReply::Working => thread.state = WhyState::Working,
            WhyReply::Answer(body) => {
                thread.messages.push(WhyMessage {
                    author: WhyAuthor::ReviewAgent,
                    body: body.trim().to_owned(),
                });
                thread.state = WhyState::Answered;
            }
        }
        Ok(())
    }
}

/// What the review agent is typed: each question with its finding and the thread so far, and how to answer.
fn questions_message(threads: &[&FindingThread], subject: &ReviewSubject) -> String {
    let key = shell_word(&subject.to_string());
    let ids: Vec<String> = threads
        .iter()
        .map(|thread| thread.finding.id.to_string())
        .collect();
    let mut lines = vec![
        format!("Questions on findings {} of {subject}", ids.join(", ")),
        "The reviewer asks about findings you reported in your AI review. Answer from what you saw \
         in the code, and check it again where you need to. Do not change code."
            .to_owned(),
    ];
    for thread in threads {
        let finding = &thread.finding;
        let id = shell_word(&finding.id.to_string());
        lines.push(String::new());
        lines.push(format!("Finding {} ({})", finding.id, finding.severity));
        lines.push(format!("Place: {}", finding.anchor));
        lines.push(format!("Finding: {}", finding.body));
        let (question, earlier) = thread
            .messages
            .split_last()
            .map_or(("", &[][..]), |(last, earlier)| {
                (last.body.as_str(), earlier)
            });
        for said in earlier {
            let who = match said.author {
                WhyAuthor::Reviewer => "Reviewer",
                WhyAuthor::ReviewAgent => "You",
                WhyAuthor::TourAgent => "Tour agent",
            };
            lines.push(format!("{who} said before: {}", said.body));
        }
        lines.push(format!("Question: {question}"));
        lines.push(format!(
            "Answer: `guided-review agent-reply {key} {id} --body -` with the answer on stdin, in a \
             quoted heredoc (`<<'EOF'`)."
        ));
    }
    lines.push(String::new());
    lines.push(
        "Answer each finding on its own, with its own id: short paragraphs, code names in backticks."
            .to_owned(),
    );
    lines.join("\n")
}

/// Typing the waiting questions into the review agent, owned so it can run on a background thread.
#[derive(Debug)]
pub struct WhyDeliveryRequest {
    ids: Vec<FindingId>,
    subject: ReviewSubject,
    text: String,
}

impl WhyDeliveryRequest {
    pub fn run(self, agents: &ReviewAgents) -> WhyDeliveryOutcome {
        let how = match agents.send_if_idle(&self.subject, &self.text) {
            Ok(SendOutcome::Delivered) => WhyDelivery::Delivered,
            Ok(SendOutcome::Unreported(_) | SendOutcome::Refused(_) | SendOutcome::Starting) => {
                WhyDelivery::Waiting
            }
            Ok(SendOutcome::NoAgent) => WhyDelivery::Failed(
                "the review agent is gone: ask again for the Ask panel".to_owned(),
            ),
            Err(error) => WhyDelivery::Failed(format!("{error:#}")),
        };
        WhyDeliveryOutcome { ids: self.ids, how }
    }
}

#[derive(Debug)]
pub struct WhyDeliveryOutcome {
    ids: Vec<FindingId>,
    how: WhyDelivery,
}

#[derive(Debug)]
enum WhyDelivery {
    Delivered,
    /// The agent is busy or starting; the questions stay queued.
    Waiting,
    Failed(String),
}

impl fmt::Display for WhyReply {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Working => f.write_str("is being looked into"),
            Self::Answer(_) => f.write_str("is answered"),
        }
    }
}
