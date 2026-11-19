//! Findings the reviewer sent to the tour agent to fix, and where each is: waiting, with the agent,
//! fixed in a commit, or not fixed with the reason. A fixed finding is closed: it is decided like a
//! used or dismissed one, and this keeps the commit.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::findings::{Finding, FindingId};
use crate::review_subject::{LocalChangeSet, ReviewSubject};

use super::hand_offs::shell_word;
use super::tour_agent::{AgentStatus, SendOutcome, TourAgents, TourKind};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FixState {
    /// Waits for the tour agent to be idle: the batch goes as one message.
    Queued,
    Delivered,
    /// The tour agent was seen working since the message went.
    Working,
    /// The tour agent said it started on this finding.
    Fixing,
    Fixed {
        commit: String,
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
            Self::Queued | Self::Delivered | Self::Working | Self::Fixing
        )
    }

    /// What the reviewer reads under the finding.
    pub fn line(&self) -> String {
        match self {
            Self::Queued => "Fix requested: waits for the tour agent".to_owned(),
            Self::Delivered => "Fix requested: sent to the tour agent".to_owned(),
            Self::Working => "Fix requested: the tour agent is working".to_owned(),
            Self::Fixing => "Fixing…".to_owned(),
            Self::Fixed { commit } => format!("Fixed in {commit}"),
            Self::NotFixed { reason } => format!("Not fixed: {reason}"),
            Self::Failed(reason) => format!("Fix request failed: {reason}"),
            Self::EndedWithoutAnswer => {
                "The tour agent stopped without saying if it fixed this".to_owned()
            }
        }
    }
}

/// What the tour agent says about one finding of a fix request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FixReply {
    Working,
    Fixed { commit: String },
    NotFixed { reason: String },
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("{0} has no fix request to answer")]
pub struct NoFixRequest(FindingId);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FixRequest {
    finding: Finding,
    batch: u32,
    state: FixState,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FixRequests(Vec<FixRequest>);

impl FixRequests {
    pub const fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// A re-pin moved the findings: a request not delivered yet tells the tour agent their new place.
    pub(super) fn follow(&mut self, moved: &[Finding]) {
        for request in &mut self.0 {
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
            _ => FixState::NotFixed {
                reason: "The caller checks the list first, on purpose.".to_owned(),
            },
        };
        Self(
            findings
                .iter()
                .enumerate()
                .map(|(position, finding)| FixRequest {
                    finding: finding.clone(),
                    batch: 0,
                    state: state(position),
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
            FixState::Fixed { commit } => Some(commit.as_str()),
            _ => None,
        })
    }

    /// One batch, in the order given. An earlier answer for a finding is replaced.
    pub(super) fn request(&mut self, findings: Vec<Finding>) {
        let batch = self
            .0
            .iter()
            .map(|request| request.batch)
            .max()
            .map_or(0, |last| last + 1);
        for finding in findings {
            self.0.retain(|request| request.finding.id != finding.id);
            self.0.push(FixRequest {
                finding,
                batch,
                state: FixState::Queued,
            });
        }
    }

    /// The oldest queued batch as one message, unless one went and its agent has not started on it.
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
        for request in &mut self.0 {
            request.state = match (&request.state, status) {
                (FixState::Delivered, AgentStatus::Working) => FixState::Working,
                (FixState::Working | FixState::Fixing, AgentStatus::Idle) => {
                    FixState::EndedWithoutAnswer
                }
                _ => continue,
            };
        }
    }

    /// Takes in the tour agent's word on one finding. Returns the finding when it is fixed, which closes it.
    pub(super) fn replied(
        &mut self,
        id: &FindingId,
        reply: &FixReply,
    ) -> Result<Option<Finding>, NoFixRequest> {
        let request = self
            .0
            .iter_mut()
            .rfind(|request| request.finding.id == *id)
            .ok_or_else(|| NoFixRequest(id.clone()))?;
        request.state = match reply {
            FixReply::Working => FixState::Fixing,
            FixReply::Fixed { commit } => FixState::Fixed {
                commit: commit.clone(),
            },
            FixReply::NotFixed { reason } => FixState::NotFixed {
                reason: reason.clone(),
            },
        };
        Ok(matches!(reply, FixReply::Fixed { .. }).then(|| request.finding.clone()))
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
        lines.push(format!(
            "Answer: `guided-review fix-reply {key} {id} --working` when you start on it, then \
             `guided-review fix-reply {key} {id} --fixed <commit sha>` once it is committed, or \
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
            Self::Fixed { commit } => write!(f, "fixed in {commit}"),
            Self::NotFixed { .. } => f.write_str("not fixed"),
        }
    }
}
