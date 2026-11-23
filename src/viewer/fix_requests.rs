//! Findings the reviewer sent to the tour agent to fix, and where each is: waiting, with the agent,
//! fixed in a commit, or not fixed with the reason. A fixed finding is closed: it is decided like a
//! used or dismissed one, and this keeps the commit.

use crate::viewer::kept_values::KeptEntries;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::findings::{Finding, FindingId};
use crate::review_subject::{LocalChangeSet, ReviewSubject};

use super::hand_offs::shell_word;
use super::tour_agent::{AgentStatus, SendOutcome, TourAgents, TourKind};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FixState {
    /// Added to fixes: it goes to the tour agent with the others at Send fixes.
    ToSend,
    /// Waits for the tour agent to be idle: the batch goes as one message.
    Queued,
    Delivered,
    /// The tour agent was seen working since the message went.
    Working,
    /// The tour agent said it started on this finding.
    Fixing,
    /// The tour agent asked the reviewer something before fixing, and waits for the answer.
    Asked,
    /// The reviewer answered; the answer waits for the tour agent to be idle.
    AnswerQueued {
        answer: String,
    },
    Fixed {
        commit: String,
        /// What the tour agent says it did, when it said.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        summary: Option<String>,
    },
    NotFixed {
        reason: String,
    },
    Failed(String),
    EndedWithoutAnswer,
}

impl FixState {
    /// The fix is still asked for or under way, so asking again would send it twice.
    pub const fn is_in_progress(&self) -> bool {
        matches!(
            self,
            Self::Queued
                | Self::Delivered
                | Self::Working
                | Self::Fixing
                | Self::Asked
                | Self::AnswerQueued { .. }
        )
    }

    /// Use, Use as is and Dismiss wait while the tour agent works on the fix; a question it asked
    /// leaves the reviewer free to drop the finding instead of answering.
    pub const fn blocks_decisions(&self) -> bool {
        self.is_in_progress() && !matches!(self, Self::Asked)
    }

    /// What the reviewer reads under the finding.
    pub fn line(&self) -> String {
        match self {
            Self::ToSend => "To fix: goes with Send fixes".to_owned(),
            Self::Queued => "Fix requested: waits for the tour agent".to_owned(),
            Self::Delivered => "Fix requested: sent to the tour agent".to_owned(),
            Self::Working => "Fix requested: the tour agent is working".to_owned(),
            Self::Fixing => "Fixing…".to_owned(),
            Self::Asked => "The tour agent asks you something: answer it below".to_owned(),
            Self::AnswerQueued { .. } => "Your answer waits for the tour agent".to_owned(),
            Self::Fixed { commit, .. } => format!("Fixed in {commit}"),
            Self::NotFixed { reason } => format!("Not fixed: {reason}"),
            Self::Failed(reason) => format!("Fix request failed: {reason}"),
            Self::EndedWithoutAnswer => {
                "The tour agent stopped without saying if it fixed this".to_owned()
            }
        }
    }
}

/// What the reviewer tells the tour agent about how to fix a finding: never blank.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FixNote(String);

impl FixNote {
    pub fn new(text: &str) -> Option<Self> {
        let text = text.trim();
        (!text.is_empty()).then(|| Self(text.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// When a fix with a note goes to the tour agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FixTiming {
    Now,
    WithSendFixes,
}

/// What the tour agent says about one finding of a fix request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FixReply {
    Working,
    /// It needs a decision from the reviewer before it fixes.
    Question {
        text: String,
    },
    Fixed {
        commit: String,
        /// What the tour agent says it did, when it said.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        summary: Option<String>,
    },
    NotFixed {
        reason: String,
    },
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("{0} has no fix request to answer")]
pub struct NoFixRequest(FindingId);

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("the tour agent asked nothing on {0} that waits for an answer")]
pub struct NoFixQuestion(pub FindingId);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FixRequest {
    finding: Finding,
    batch: u32,
    state: FixState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    note: Option<FixNote>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FixRequests(KeptEntries<FixRequest>);

impl FixRequests {
    /// Entries a newer build wrote in a shape this build does not read; kept in the file as written.
    pub fn unread_entries(&self) -> usize {
        self.0.unread_count()
    }

    pub const fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// A re-pin moved the findings: a request not delivered yet tells the tour agent their new place.
    pub(super) fn follow(&mut self, moved: &[Finding]) {
        for request in self.0.iter_mut() {
            request.finding.follow(moved);
        }
    }

    pub fn state_of(&self, id: &FindingId) -> Option<&FixState> {
        self.0
            .iter()
            .rfind(|request| request.finding.id == *id)
            .map(|request| &request.state)
    }

    /// What a snapshot scene shows of fixes, and its removal.
    #[cfg(feature = "snapshots")]
    pub(super) fn staged(findings: &[Finding]) -> Self {
        let state = |position: usize| match position {
            0 => FixState::Fixing,
            _ => FixState::ToSend,
        };
        let note = |position: usize| {
            (position > 0)
                .then(|| FixNote::new("Keep the old name in the docs."))
                .flatten()
        };
        Self(
            findings
                .iter()
                .enumerate()
                .map(|(position, finding)| FixRequest {
                    finding: finding.clone(),
                    batch: 0,
                    state: state(position),
                    note: note(position),
                })
                .collect(),
        )
    }

    /// What a snapshot scene shows of findings the tour agent fixed.
    #[cfg(feature = "snapshots")]
    pub(super) fn staged_fixed(findings: &[Finding]) -> Self {
        Self(
            findings
                .iter()
                .map(|finding| FixRequest {
                    finding: finding.clone(),
                    batch: 0,
                    state: FixState::Fixed {
                        commit: "abc1234".to_owned(),
                        summary: Some(
                            "Returns early when the list is empty, with a test for it.".to_owned(),
                        ),
                    },
                    note: None,
                })
                .collect(),
        )
    }

    pub fn is_in_progress(&self, id: &FindingId) -> bool {
        self.state_of(id).is_some_and(FixState::is_in_progress)
    }

    /// The commits of the fixes made, oldest first.
    pub fn fixed_commits(&self) -> impl Iterator<Item = &str> {
        self.0.iter().filter_map(|request| match &request.state {
            FixState::Fixed { commit, .. } => Some(commit.as_str()),
            _ => None,
        })
    }

    /// One batch, in the order given. An earlier answer for a finding is replaced; a note given when
    /// it was added to fixes goes with it.
    pub(super) fn request(&mut self, findings: Vec<(Finding, Option<FixNote>)>) {
        self.put(findings, &FixState::Queued);
    }

    /// Added to fixes: each waits for Send fixes. An earlier answer for a finding is replaced.
    pub(super) fn add_to_send(&mut self, findings: Vec<(Finding, Option<FixNote>)>) {
        self.put(findings, &FixState::ToSend);
    }

    /// The note the reviewer gave with a finding that waits for Send fixes.
    pub fn note_to_send(&self, id: &FindingId) -> Option<&FixNote> {
        self.0
            .iter()
            .find(|request| request.finding.id == *id && request.state == FixState::ToSend)
            .and_then(|request| request.note.as_ref())
    }

    /// Nothing went to the tour agent yet, or it waits for the reviewer: a decided finding drops it.
    pub(super) fn forget_decided(&mut self, id: &FindingId) {
        self.0.retain(|request| {
            request.finding.id != *id
                || !matches!(request.state, FixState::ToSend | FixState::Asked)
        });
    }

    fn put(&mut self, findings: Vec<(Finding, Option<FixNote>)>, state: &FixState) {
        let batch = self
            .0
            .iter()
            .map(|request| request.batch)
            .max()
            .map_or(0, |last| last + 1);
        for (finding, note) in findings {
            let note = note.or_else(|| self.note_to_send(&finding.id).cloned());
            self.0.retain(|request| request.finding.id != finding.id);
            self.0.push(FixRequest {
                finding,
                batch,
                state: state.clone(),
                note,
            });
        }
    }

    /// Takes the finding out of the fixes to send; false when it was not among them.
    pub(super) fn remove_to_send(&mut self, id: &FindingId) -> bool {
        let before = self.0.len();
        self.0
            .retain(|request| !(request.finding.id == *id && request.state == FixState::ToSend));
        self.0.len() != before
    }

    /// What waits for Send fixes, in the order it was added.
    pub fn to_send(&self) -> impl Iterator<Item = &FindingId> {
        self.0
            .iter()
            .filter(|request| request.state == FixState::ToSend)
            .map(|request| &request.finding.id)
    }

    /// Takes in the reviewer's answer to the tour agent's question, which then waits to be sent.
    pub(super) fn answered(
        &mut self,
        id: &FindingId,
        answer: &str,
    ) -> Result<Finding, NoFixQuestion> {
        let request = self
            .0
            .iter_mut()
            .rfind(|request| request.finding.id == *id && request.state == FixState::Asked)
            .ok_or_else(|| NoFixQuestion(id.clone()))?;
        request.state = FixState::AnswerQueued {
            answer: answer.to_owned(),
        };
        Ok(request.finding.clone())
    }

    /// An answer to the tour agent's question first, since the agent waits for it; else the oldest
    /// queued batch as one message. Nothing goes while a message is not yet taken up.
    pub(super) fn delivery_request(
        &self,
        subject: &ReviewSubject,
        tour: TourKind,
    ) -> Option<FixDeliveryRequest> {
        if self
            .0
            .iter()
            .any(|request| request.state == FixState::Delivered)
        {
            return None;
        }
        if let Some(request) = self
            .0
            .iter()
            .find(|request| matches!(request.state, FixState::AnswerQueued { .. }))
        {
            return Some(FixDeliveryRequest {
                ids: vec![request.finding.id.clone()],
                subject: subject.clone(),
                text: answer_message(request, subject),
                tour,
            });
        }
        let batch = self
            .0
            .iter()
            .find(|request| request.state == FixState::Queued)?
            .batch;
        let requests: Vec<&FixRequest> = self
            .0
            .iter()
            .filter(|request| request.batch == batch && request.state == FixState::Queued)
            .collect();
        Some(FixDeliveryRequest {
            ids: requests
                .iter()
                .map(|request| request.finding.id.clone())
                .collect(),
            subject: subject.clone(),
            text: fixes_message(&requests, subject),
            tour,
        })
    }

    pub(super) fn apply_delivery(&mut self, outcome: FixDeliveryOutcome) {
        let state = match outcome.how {
            FixDelivery::Delivered => FixState::Delivered,
            FixDelivery::Waiting | FixDelivery::Started => return,
            FixDelivery::Failed(reason) => FixState::Failed(reason),
        };
        for request in self
            .0
            .iter_mut()
            .filter(|request| outcome.ids.contains(&request.finding.id))
        {
            request.state = state.clone();
        }
    }

    /// A delivered batch is working once the agent is seen working; when the agent then goes idle,
    /// each finding it did not answer ended without an answer.
    pub(super) fn agent_status_is(&mut self, status: AgentStatus) {
        for request in self.0.iter_mut() {
            request.state = match (&request.state, status) {
                (FixState::Delivered, AgentStatus::Working) => FixState::Working,
                (FixState::Working | FixState::Fixing, AgentStatus::Idle) => {
                    FixState::EndedWithoutAnswer
                }
                _ => continue,
            };
        }
    }

    /// Takes in the tour agent's word on one finding, and returns the finding.
    pub(super) fn replied(
        &mut self,
        id: &FindingId,
        reply: &FixReply,
    ) -> Result<Finding, NoFixRequest> {
        let request = self
            .0
            .iter_mut()
            .rfind(|request| request.finding.id == *id)
            .ok_or_else(|| NoFixRequest(id.clone()))?;
        request.state = match reply {
            FixReply::Working => FixState::Fixing,
            FixReply::Question { .. } => FixState::Asked,
            FixReply::Fixed { commit, summary } => FixState::Fixed {
                commit: commit.clone(),
                summary: summary.clone(),
            },
            FixReply::NotFixed { reason } => FixState::NotFixed {
                reason: reason.clone(),
            },
        };
        Ok(request.finding.clone())
    }
}

/// What the tour agent is typed for one batch: everything it needs to fix and answer without asking back.
fn fixes_message(requests: &[&FixRequest], subject: &ReviewSubject) -> String {
    let key = shell_word(&subject.to_string());
    let ids: Vec<String> = requests
        .iter()
        .map(|request| request.finding.id.to_string())
        .collect();
    let mut lines = vec![
        format!("Fix requests {} on {subject}", ids.join(", ")),
        "These findings come from an AI review of the change. The reviewer wants them fixed, and \
         sends them together, in reading order. Start one subagent for all of them, and have it fix \
         them one after another, with one commit per finding. Do not start one subagent per finding: \
         they would edit the same files at once."
            .to_owned(),
        where_to_fix(subject),
    ];
    for request in requests {
        let finding = &request.finding;
        let id = &finding.id;
        lines.push(String::new());
        lines.push(format!("Finding {id} ({})", finding.severity));
        lines.push(format!("Place: {}", finding.anchor));
        lines.push(format!("Finding: {}", finding.body));
        if let Some(note) = &request.note {
            lines.push(format!(
                "The reviewer's note on how to fix it: {}",
                note.as_str()
            ));
        }
        lines.push(format!(
            "Needs the reviewer's decision first? Ask through the app, not in your terminal, where they \
             may not look: `guided-review fix-reply {key} {id} --question --body -` with the question on \
             stdin, in a quoted heredoc. Then go on with the next finding, or stop if it was the last: \
             the reviewer's answer is typed to you as a new message."
        ));
        lines.push(format!(
            "Answer: `guided-review fix-reply {key} {id} --working` when you start on it, then \
             `guided-review fix-reply {key} {id} --fixed <commit sha> --body -` once it is committed, with \
             what you changed and why on stdin, in a quoted heredoc, for the reviewer to judge, or \
             `guided-review fix-reply {key} {id} --not-fixed --body -` with the reason on stdin, in a \
             quoted heredoc (`<<'EOF'`), when the finding is wrong or cannot be fixed."
        ));
    }
    lines.push(String::new());
    lines.push(format!(
        "Answer each finding on its own, with its own id, in this order. Read the reviewed change with \
         `guided-review outline {key}` and `guided-review show {key} <piece ids> --around`."
    ));
    lines.join("\n")
}

/// What the tour agent is typed once the reviewer answered its question: the finding again, since its
/// context may have moved on, and how to go on.
fn answer_message(request: &FixRequest, subject: &ReviewSubject) -> String {
    let key = shell_word(&subject.to_string());
    let finding = &request.finding;
    let id = &finding.id;
    let answer = match &request.state {
        FixState::AnswerQueued { answer } => answer.as_str(),
        _ => "",
    };
    [
        format!("The reviewer answered your question on finding {id} of {subject}"),
        format!("Place: {}", finding.anchor),
        format!("Finding: {}", finding.body),
        format!("Answer: {answer}"),
        format!(
            "Go on: `guided-review fix-reply {key} {id} --working`, then `guided-review fix-reply {key} {id} --fixed <commit sha>` \
             once it is committed, or `guided-review fix-reply {key} {id} --not-fixed --body -` with the reason on stdin, in a \
             quoted heredoc (`<<'EOF'`). Ask again with `--question --body -` if you still need a decision."
        ),
    ]
    .join("\n")
}

fn where_to_fix(subject: &ReviewSubject) -> String {
    match subject {
        ReviewSubject::PullRequest(_) => "Make the changes in your folder when it is a clone of the PR's \
             repo on the PR's branch with none of the reviewer's own changes in it; else in a new git \
             worktree, or a shallow clone, of the PR's branch. Push the commits to the PR's branch."
            .to_owned(),
        ReviewSubject::Local(change_set) => local_where_to_fix(change_set),
    }
}

fn local_where_to_fix(change_set: &LocalChangeSet) -> String {
    format!(
        "This is a local review: there is no PR. Make the changes in {}, on {}. Do not push for the \
         review: the reviewer sees what changed on disk and updates.",
        change_set.folder().as_path().display(),
        change_set.branch().as_str()
    )
}

/// Sending the oldest queued batch to the tour agent, owned so it can run on a background thread.
pub(super) struct FixDeliveryRequest {
    ids: Vec<FindingId>,
    subject: ReviewSubject,
    text: String,
    /// What a tour agent started for this batch has to read.
    tour: TourKind,
}

impl FixDeliveryRequest {
    pub(super) fn run(self, agents: &TourAgents) -> FixDeliveryOutcome {
        let how = match agents.send_if_idle(&self.subject, &self.text) {
            Ok(SendOutcome::Delivered) => FixDelivery::Delivered,
            Ok(SendOutcome::Unreported(_) | SendOutcome::Refused(_) | SendOutcome::Starting) => {
                FixDelivery::Waiting
            }
            Ok(SendOutcome::NoAgent) => {
                match agents.start(&self.subject, &self.subject, self.tour) {
                    Ok(()) => FixDelivery::Started,
                    Err(error) => FixDelivery::Failed(format!("{error:#}")),
                }
            }
            Err(error) => FixDelivery::Failed(format!("{error:#}")),
        };
        FixDeliveryOutcome { ids: self.ids, how }
    }
}

pub(super) struct FixDeliveryOutcome {
    ids: Vec<FindingId>,
    how: FixDelivery,
}

impl FixDeliveryOutcome {
    /// A tour agent was started, so there is a new terminal to follow.
    pub(super) const fn started_an_agent(&self) -> bool {
        matches!(self.how, FixDelivery::Started)
    }
}

enum FixDelivery {
    Delivered,
    /// A new tour agent was started; the batch is sent once it is idle.
    Started,
    /// The agent is not ready; the batch stays queued.
    Waiting,
    Failed(String),
}

impl fmt::Display for FixReply {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Working => f.write_str("is being fixed"),
            Self::Question { .. } => f.write_str("waits for your answer"),
            Self::Fixed { commit, .. } => write!(f, "fixed in {commit}"),
            Self::NotFixed { .. } => f.write_str("not fixed"),
        }
    }
}
