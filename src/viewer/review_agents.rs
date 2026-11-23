//! The review agent of a subject: started in a throwaway worktree of the reviewed head, found by its own herdr
//! label, and ended with its worktree.

use std::ops::ControlFlow;
use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, bail};

use super::tour_agent::{
    AgentLinkOff, AgentRole, ReviewAgentStart, SendOutcome, TerminalId, TourAgentPort,
    TourAgentState, follow_state, make_room, terminal_name,
};
use crate::app_folders::AppFolders;
use crate::claude_cli::{review_agent_denied_tools, review_agent_tools};
use crate::instructions::ReviewInstructions;
use crate::prompt_safety::OUTSIDE_TEXT_IS_DATA;
use crate::pull_request::Sha;
use crate::review_subject::ReviewSubject;
use crate::review_worktree::ReviewWorktreePort;
use crate::tour::Tour;

#[derive(Clone)]
pub struct ReviewAgents {
    port: Arc<dyn TourAgentPort>,
    worktrees: Arc<dyn ReviewWorktreePort>,
    folders: AppFolders,
}

impl ReviewAgents {
    pub fn new(
        port: Arc<dyn TourAgentPort>,
        worktrees: Arc<dyn ReviewWorktreePort>,
        folders: AppFolders,
    ) -> Self {
        Self {
            port,
            worktrees,
            folders,
        }
    }

    /// Starts the review agent of the subject in a fresh worktree of `head`, cut from the clone at `repo`.
    /// A review agent of the subject from an earlier run is ended first, as it was started for the same job.
    /// The agent takes one of the slots tour agents share (`make_room`), never the one of `active`.
    pub fn start(
        &self,
        subject: &ReviewSubject,
        repo: &Path,
        head: &Sha,
        active: &ReviewSubject,
    ) -> anyhow::Result<TerminalId> {
        if let Err(reason) = self.port.link() {
            bail!("agents are off: {reason}");
        }
        self.end(subject)?;
        make_room(self.port.as_ref(), Some(self.worktrees.as_ref()), active)?;
        let worktree = self
            .worktrees
            .add(subject, repo, head)
            .with_context(|| format!("could not check out {head} for the review of {subject}"))?;
        let findings = Tour::findings_path_for(&self.folders, subject);
        let skill = ReviewInstructions::skill_for_launch(&self.folders);
        let started = self.port.start_review(&ReviewAgentStart {
            subject: subject.clone(),
            name: terminal_name(AgentRole::Review, subject),
            folder: worktree.as_path().to_path_buf(),
            first_task: first_task(subject),
            tools: review_agent_tools(worktree.as_path(), &findings, skill.as_deref()),
            denied_tools: review_agent_denied_tools(),
        });
        if started.is_err()
            && let Err(error) = self.worktrees.remove(subject)
        {
            eprintln!("could not remove the worktree of {subject}: {error:#}");
        }
        started
    }

    /// The subject's review agent: the terminal with its review label, and no other.
    pub fn lookup(&self, subject: &ReviewSubject) -> TourAgentState {
        if let Err(reason) = self.port.link() {
            return TourAgentState::LinkOff(reason);
        }
        self.find(subject)
            .unwrap_or_else(|error| TourAgentState::LinkOff(AgentLinkOff::because_of(&error)))
    }

    /// Passes the agent's state now and at each status change, blocking until herdr's stream ends.
    /// `on_change` breaks when nobody wants the changes any more, which ends the follow.
    pub fn follow(
        &self,
        subject: &ReviewSubject,
        on_change: &mut dyn FnMut(TourAgentState) -> ControlFlow<()>,
    ) {
        follow_state(self.port.as_ref(), self.lookup(subject), on_change);
    }

    /// Types `text` into the subject's review agent when it is idle.
    pub fn send_if_idle(&self, subject: &ReviewSubject, text: &str) -> anyhow::Result<SendOutcome> {
        match self.lookup(subject) {
            TourAgentState::Agent { terminal, .. } => self.port.send_if_idle(&terminal, text),
            TourAgentState::LinkOff(reason) => bail!("agents are off: {reason}"),
            _ => Ok(SendOutcome::NoAgent),
        }
    }

    pub fn show(&self, subject: &ReviewSubject) -> anyhow::Result<()> {
        match self.lookup(subject) {
            TourAgentState::Agent { terminal, .. } => self.port.show(&terminal),
            TourAgentState::LinkOff(reason) => bail!("agents are off: {reason}"),
            _ => bail!("no review agent for {subject}"),
        }
    }

    /// Ends the subject's review agent and removes its worktree, whichever of them is there.
    pub fn end(&self, subject: &ReviewSubject) -> anyhow::Result<()> {
        let killed = self.kill_agents_of(subject);
        let removed = self
            .worktrees
            .remove(subject)
            .with_context(|| format!("could not remove the worktree of {subject}"));
        killed.and(removed)
    }

    /// The subjects whose review agent the app has alive, so a sweep of leftover worktrees keeps theirs.
    pub fn running_subjects(&self) -> anyhow::Result<Vec<ReviewSubject>> {
        Ok(self
            .port
            .app_started()?
            .into_iter()
            .filter(|agent| agent.role == AgentRole::Review)
            .filter_map(|agent| agent.subject)
            .collect())
    }

    fn find(&self, subject: &ReviewSubject) -> anyhow::Result<TourAgentState> {
        let Some(terminal) = self.port.review_terminals_of(subject)?.into_iter().next() else {
            return Ok(TourAgentState::None);
        };
        let status = self.port.status(&terminal)?;
        Ok(TourAgentState::Agent { terminal, status })
    }

    fn kill_agents_of(&self, subject: &ReviewSubject) -> anyhow::Result<()> {
        for old in self.port.review_terminals_of(subject)? {
            self.port
                .kill(&old)
                .with_context(|| format!("could not end the review agent {old} of {subject}"))?;
        }
        Ok(())
    }
}

/// What the review agent is told first; the brief it asks for holds the protocol and the instructions. The
/// agent runs with a fixed tool list, under which a chained or unlisted command is denied without a word
/// (docs/wiki/agent-runs.md).
fn first_task(subject: &ReviewSubject) -> String {
    format!(
        "You are the review agent of {subject}, started by the guided-review app: you read its code in this \
         folder, a checkout of the reviewed head, and write findings. Run `guided-review review-brief {subject}` \
         now and follow it exactly. Run one command per Bash call, with the key written out, without shell \
         variables, pipes, `&&`, `;` or `$(...)`. Never change code and never post on GitHub. {OUTSIDE_TEXT_IS_DATA}"
    )
}
