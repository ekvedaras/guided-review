//! The review agent of a subject: started where `ReviewPlace` says, found by its own herdr label.

use std::ops::ControlFlow;
use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, bail};

use super::review_place::{CheckoutPort, ReviewPlace};
use super::tour_agent::{
    AgentLinkOff, AgentRole, ReviewAgentStart, SendOutcome, TerminalId, TourAgentPort,
    TourAgentState, follow_state, make_room, terminal_name,
};
use crate::agent_cli::AgentCli;
use crate::app_folders::AppFolders;
use crate::config::Config;
use crate::instructions::Reviewer;
use crate::prompt_safety::OUTSIDE_TEXT_IS_DATA;
use crate::pull_request::Sha;
use crate::review_subject::ReviewSubject;
use crate::tour::Tour;

#[derive(Clone)]
pub struct ReviewAgents {
    port: Arc<dyn TourAgentPort>,
    checkouts: Arc<dyn CheckoutPort>,
    folders: AppFolders,
}

impl ReviewAgents {
    pub fn new(
        port: Arc<dyn TourAgentPort>,
        checkouts: Arc<dyn CheckoutPort>,
        folders: AppFolders,
    ) -> Self {
        Self {
            port,
            checkouts,
            folders,
        }
    }

    /// Starts the review agent of the subject at `head`, in the place `ReviewPlace::choose` picks from the
    /// subject's tour agent folder and the clone at `clone`. A review agent of the subject from an earlier run is ended first, as it was started for the same job.
    /// The agent takes one of the slots tour agents share (`make_room`), never the one of `active`.
    pub fn start(
        &self,
        subject: &ReviewSubject,
        tour_agent_folder: Option<&Path>,
        clone: &Path,
        head: &Sha,
        active: &ReviewSubject,
    ) -> anyhow::Result<TerminalId> {
        if let Err(reason) = self.port.link() {
            bail!("agents are off: {reason}");
        }
        self.end(subject)?;
        make_room(self.port.as_ref(), active)?;
        let place = ReviewPlace::choose(
            subject,
            tour_agent_folder,
            clone,
            head,
            self.checkouts.as_ref(),
        )?;
        self.port.start_review(&ReviewAgentStart {
            subject: subject.clone(),
            name: terminal_name(AgentRole::Review, subject),
            first_task: first_task(subject, &place, head, &self.agent_cli()),
            place,
            findings: Tour::findings_path_for(&self.folders, subject),
            skill: Config::skill_for_launch::<Reviewer>(&self.folders),
        })
    }

    /// How these agents run guided-review, which every message to them names.
    pub(super) fn agent_cli(&self) -> AgentCli {
        self.port.agent_cli()
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

    /// Makes the terminal the subject's review agent, the reviewer's pick: the review label moves to
    /// it from any other terminal, which keeps running.
    pub fn claim(&self, subject: &ReviewSubject, terminal: &TerminalId) -> anyhow::Result<()> {
        if let Err(reason) = self.port.link() {
            bail!("agents are off: {reason}");
        }
        for other in self.port.review_terminals_of(subject)? {
            if &other != terminal {
                self.port
                    .release_review(&other, subject)
                    .with_context(|| format!("could not take the review label off {other}"))?;
            }
        }
        self.port
            .claim_review(terminal, subject)
            .with_context(|| format!("could not make {terminal} the review agent of {subject}"))
    }

    /// Ends the subject's review agent. A worktree it made is its own to remove (it leaves with
    /// `ExitWorktree` when done).
    pub fn end(&self, subject: &ReviewSubject) -> anyhow::Result<()> {
        self.kill_agents_of(subject)
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

    /// Ends the review agents the app started; one the reviewer picked is their own session, so it
    /// only loses the label.
    fn kill_agents_of(&self, subject: &ReviewSubject) -> anyhow::Result<()> {
        let app_started: Vec<TerminalId> = self
            .port
            .app_started()?
            .into_iter()
            .map(|agent| agent.terminal)
            .collect();
        for old in self.port.review_terminals_of(subject)? {
            if app_started.contains(&old) {
                self.port.kill(&old).with_context(|| {
                    format!("could not end the review agent {old} of {subject}")
                })?;
            } else {
                self.port
                    .release_review(&old, subject)
                    .with_context(|| format!("could not take the review label off {old}"))?;
            }
        }
        Ok(())
    }
}

/// What the review agent is told first; the brief it asks for holds the protocol and the instructions. The
/// agent runs with a fixed tool list, under which a chained or unlisted command is denied without a word
/// (docs/wiki/agent-runs.md).
fn first_task(subject: &ReviewSubject, place: &ReviewPlace, head: &Sha, cli: &AgentCli) -> String {
    let (where_to_work, never_change) = match place {
        ReviewPlace::TourAgentFolder(_) => (
            format!(
                "You work in this folder as it is: it is the tour agent's, and it stands at the reviewed head {head} \
                 with nothing uncommitted, so read code here and make no worktree. "
            ),
            "Never change code",
        ),
        ReviewPlace::OwnWorktree { .. } => (
            format!(
                "This folder is the reviewer's clone and may stand on another commit: do not read or run anything \
                 in it. First make your own worktree with Claude Code's EnterWorktree tool, never with \
                 `git worktree add`, so the project's own worktree hooks run when it has them. Then run `git checkout --detach {head}` inside it and work only there. \
                 When you are done, after `{cli} findings-done`, leave with ExitWorktree, action \
                 \"remove\" with discard_changes true. "
            ),
            "Never change the reviewed code",
        ),
    };
    let host = subject.host_name();
    format!(
        "You are the review agent of {subject}, started by the guided-review app: you read the code of its \
         reviewed head {head} and write findings. {where_to_work}Run `{cli} review-brief {subject}` \
         now and follow it exactly. Run one command per Bash call, with the key written out, without shell \
         variables, pipes, `&&`, `;` or `$(...)`. {never_change} and never post on {host}. {OUTSIDE_TEXT_IS_DATA}"
    )
}
