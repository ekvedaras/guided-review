use std::fmt;
use std::ops::ControlFlow;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, bail};

use super::active_pr::OpenOrigin;
use crate::pull_request::PullRequestUrl;

/// A herdr terminal, as `hc` names it (`t7`).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct TerminalId(String);

impl TerminalId {
    pub fn new(address: impl Into<String>) -> Self {
        Self(address.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for TerminalId {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// What a tour agent is doing, as herdr reports it for the terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentStatus {
    /// Waiting for input, or done with its last turn.
    Idle,
    Working,
    /// Blocked on a prompt only the reviewer can answer.
    WaitingForYou,
    Failed,
    /// The terminal runs a program that reports no status, such as a plain shell.
    Unreported,
}

impl AgentStatus {
    /// From the `state` herdr reports (`idle`, `working`, `done`, `blocked`, `error`), `None` when it reports none.
    pub fn from_herdr(state: Option<&str>) -> Self {
        match state {
            Some("idle" | "done") => Self::Idle,
            Some("working") => Self::Working,
            Some("blocked") => Self::WaitingForYou,
            Some("error") => Self::Failed,
            _ => Self::Unreported,
        }
    }
}

impl fmt::Display for AgentStatus {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str(match self {
            Self::Idle => "idle",
            Self::Working => "working",
            Self::WaitingForYou => "waiting for you",
            Self::Failed => "failed",
            Self::Unreported => "status unknown",
        })
    }
}

/// A live herdr terminal, as far as deciding whether it can be a tour agent needs to know it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveTerminal {
    pub terminal: TerminalId,
    /// Herdr recognises the program in it as Claude Code, so a question sent to it is read by Claude, not run by a shell.
    pub runs_claude: bool,
    /// The herdr space (workspace) it lives in.
    pub space: String,
}

/// What herdr did with text sent only if the agent is idle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SendOutcome {
    Delivered,
    /// Nothing was typed; herdr's reason, such as "t7 is working, not idle".
    Refused(String),
    /// Nothing was typed: the agent was just started and its screen has not settled yet. Try again shortly.
    Starting,
    /// The PR has no tour agent to send to.
    NoAgent,
}

/// A tour agent the app started, as far as ending the right one needs to know it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppAgent {
    pub terminal: TerminalId,
    pub pr: Option<PullRequestUrl>,
    pub status: AgentStatus,
    /// Milliseconds since the epoch when the status last changed: for an idle agent, when it last finished.
    pub status_since_ms: u64,
}

/// What a new tour agent is started with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TourAgentStart {
    pub pr: PullRequestUrl,
    /// Names the terminal after the repo and the PR number.
    pub name: String,
    pub folder: PathBuf,
    pub first_task: String,
    /// The reviewer's summary instructions skill, allowed to the agent so it is not silently denied.
    pub summary_skill: Option<String>,
}

/// At most this many tour agents the app started stay alive.
const MAX_APP_AGENTS: usize = 3;

/// Herdr behind the viewer: where a PR's tour agent runs and what it does. One adapter, `hc`.
pub trait TourAgentPort: Send + Sync {
    /// Whether tour agents can be reached at all; the reason when not.
    fn link(&self) -> Result<(), String>;

    /// The live terminals labelled as the tour agent of the PR.
    fn terminals_of(&self, pr: &PullRequestUrl) -> anyhow::Result<Vec<TerminalId>>;

    /// Every live terminal herdr lists.
    fn live_terminals(&self) -> anyhow::Result<Vec<LiveTerminal>>;

    /// Labels the terminal as the tour agent of the PR. Releasing the others is the caller's job.
    fn claim(&self, terminal: &TerminalId, pr: &PullRequestUrl) -> anyhow::Result<()>;

    /// Takes the PR label off the terminal.
    fn release(&self, terminal: &TerminalId) -> anyhow::Result<()>;

    fn status(&self, terminal: &TerminalId) -> anyhow::Result<AgentStatus>;

    /// Blocks, passing every status the terminal reports from now on, until herdr's stream ends or
    /// `on_status` breaks.
    fn follow(
        &self,
        terminal: &TerminalId,
        on_status: &mut dyn FnMut(AgentStatus) -> ControlFlow<()>,
    ) -> anyhow::Result<()>;

    /// Types the text into the agent and presses enter, but only when the agent is idle: text sent
    /// into a working agent would arrive mid-turn as input.
    fn send_if_idle(&self, terminal: &TerminalId, text: &str) -> anyhow::Result<SendOutcome>;

    /// Brings the terminal up in the herdr app.
    fn show(&self, terminal: &TerminalId) -> anyhow::Result<()>;

    /// The live tour agents the app started, never the ones the reviewer started.
    fn app_started(&self) -> anyhow::Result<Vec<AppAgent>>;

    /// Starts an interactive Claude in a new terminal that outlives the app, labelled as the app's tour
    /// agent of the PR.
    fn start(&self, start: &TourAgentStart) -> anyhow::Result<TerminalId>;

    fn kill(&self, terminal: &TerminalId) -> anyhow::Result<()>;

    /// The last `count` non-blank lines the terminal shows, as plain text; of a Claude terminal, the ones above its prompt box.
    fn screen_lines(&self, terminal: &TerminalId, count: usize) -> anyhow::Result<Vec<String>>;
}

/// What the sidebar says of a PR's tour agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TourAgentState {
    /// Not looked yet.
    Unknown,
    /// Tour agents cannot be reached, for this reason.
    LinkOff(String),
    None,
    Agent {
        terminal: TerminalId,
        status: AgentStatus,
    },
}

impl TourAgentState {
    pub fn can_show(&self) -> bool {
        matches!(self, Self::Agent { .. })
    }

    /// There is an agent, or news about why there cannot be one; "none" and "looking" say nothing.
    pub fn is_worth_a_line(&self) -> bool {
        matches!(self, Self::Agent { .. } | Self::LinkOff(_))
    }
}

impl fmt::Display for TourAgentState {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Unknown => write!(formatter, "Tour agent: looking"),
            Self::LinkOff(reason) => write!(
                formatter,
                "Tour agent is off, it needs herdr (`hc`) and its server running: {reason}"
            ),
            Self::None => write!(formatter, "Tour agent: none"),
            Self::Agent { terminal, status } => {
                write!(formatter, "Tour agent {terminal}: {status}")
            }
        }
    }
}

/// What became of an Ask handed to the reviewer's own tour agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OwnAgentHandOff {
    Delivered(TerminalId),
    StartedByApp(TerminalId),
    /// Nothing was typed, for this reason.
    NotSent(String),
    NoAgent,
}

impl OwnAgentHandOff {
    /// What the Ask panel says under the Ask.
    pub fn line(&self) -> String {
        const MAKE_YOURS: &str =
            "Run `guided-review open` from your own Claude in herdr to make it the tour agent";
        match self {
            Self::Delivered(terminal) => format!("Handed to your agent {terminal}"),
            Self::StartedByApp(terminal) => format!(
                "Not handed: the tour agent {terminal} was started by the app and cannot change files. {MAKE_YOURS}"
            ),
            Self::NotSent(reason) => format!("Not handed: {reason}. Try again when it is free"),
            Self::NoAgent => format!("Not handed: this PR has no tour agent. {MAKE_YOURS}"),
        }
    }
}

/// The tour agents of PRs, found by the herdr label `gr.pr=<url>`, never by a stored terminal id.
#[derive(Clone)]
pub struct TourAgents {
    port: Arc<dyn TourAgentPort>,
    /// Where a tour agent the app starts works. `None`: the app starts none.
    folder: Option<PathBuf>,
    summary_skill: Option<String>,
}

impl TourAgents {
    pub fn new(port: Arc<dyn TourAgentPort>) -> Self {
        Self {
            port,
            folder: None,
            summary_skill: None,
        }
    }

    pub fn with_summary_skill(mut self, skill: Option<String>) -> Self {
        self.summary_skill = skill;
        self
    }

    pub fn starting_in(mut self, folder: PathBuf) -> Self {
        self.folder = Some(folder);
        self
    }

    /// Starts the tour agent of a PR that has none, ending the idle app-started agent used longest ago when
    /// three live, never the one of `active`. Refuses, naming the busy ones, when none is idle.
    pub fn start(&self, pr: &PullRequestUrl, active: &PullRequestUrl) -> anyhow::Result<()> {
        if let Err(reason) = self.port.link() {
            bail!("tour agents are off: {reason}");
        }
        self.launch(pr, active, first_task(pr)).map(drop)
    }

    fn launch(
        &self,
        pr: &PullRequestUrl,
        active: &PullRequestUrl,
        first_task: String,
    ) -> anyhow::Result<TerminalId> {
        let Some(folder) = &self.folder else {
            bail!("no folder to start a tour agent for {pr} in");
        };
        self.make_room(active)?;
        self.port.start(&TourAgentStart {
            pr: pr.clone(),
            name: terminal_name(pr),
            folder: folder.clone(),
            first_task,
            summary_skill: self.summary_skill.clone(),
        })
    }

    /// Starts the tour agent that writes the PR's tour with the tour skill. It replaces any agent the PR had:
    /// one the app started is ended, one the reviewer started only loses the PR's label.
    pub fn write_tour(
        &self,
        pr: &PullRequestUrl,
        active: &PullRequestUrl,
    ) -> anyhow::Result<TerminalId> {
        if let Err(reason) = self.port.link() {
            bail!("tour agents are off: {reason}");
        }
        let app_started: Vec<TerminalId> = self
            .port
            .app_started()?
            .into_iter()
            .map(|agent| agent.terminal)
            .collect();
        for old in self.port.terminals_of(pr)? {
            if app_started.contains(&old) {
                self.port
                    .kill(&old)
                    .with_context(|| format!("could not end the old tour agent {old} of {pr}"))?;
            } else {
                self.port
                    .release(&old)
                    .with_context(|| format!("could not take the label of {pr} off {old}"))?;
            }
        }
        self.launch(pr, active, writer_task(pr))
    }

    /// Who an `open` from the terminal came from. Without herdr no agent of the app can be running.
    pub fn origin_of(&self, terminal: Option<&str>) -> OpenOrigin {
        let Some(address) = terminal else {
            return OpenOrigin::Reviewer;
        };
        if self.port.link().is_err() {
            return OpenOrigin::Reviewer;
        }
        // Unreadable, the open is taken as the reviewer's: switching wrongly costs a click, while
        // swallowing the reviewer's own open would look like the app ignored them.
        let app_started = self.port.app_started().unwrap_or_default();
        if app_started
            .iter()
            .any(|agent| agent.terminal.as_str() == address)
        {
            OpenOrigin::AppStartedAgent
        } else {
            OpenOrigin::Reviewer
        }
    }

    /// Makes the terminal the one tour agent of the PR: every other terminal with the PR's label loses it,
    /// so a terminal reused for another PR's tour stops being this PR's agent.
    pub fn claim(&self, pr: &PullRequestUrl, terminal: &TerminalId) -> anyhow::Result<()> {
        if let Err(reason) = self.port.link() {
            bail!("tour agents are off: {reason}");
        }
        for other in self.port.terminals_of(pr)? {
            if &other != terminal {
                self.port
                    .release(&other)
                    .with_context(|| format!("could not take the label of {pr} off {other}"))?;
            }
        }
        self.port
            .claim(terminal, pr)
            .with_context(|| format!("could not make {terminal} the tour agent of {pr}"))
    }

    /// What an `open` request does with the terminal it came from. Only a terminal running Claude is ever
    /// claimed, because `hc send` types into it: a terminal running Claude is claimed, a plain shell hands over
    /// to the one Claude terminal of its herdr space, and with none or several (a guess would put the PR's
    /// questions into an unrelated session) nothing is claimed. A request from outside herdr claims nothing.
    pub fn claim_for_open(
        &self,
        pr: &PullRequestUrl,
        terminal: Option<&str>,
    ) -> anyhow::Result<()> {
        let Some(address) = terminal else {
            return Ok(());
        };
        let live = self.port.live_terminals()?;
        let Some(origin) = live.iter().find(|live| live.terminal.as_str() == address) else {
            return Ok(());
        };
        if origin.runs_claude {
            return self.claim(pr, &origin.terminal);
        }
        let mut beside = live
            .iter()
            .filter(|other| other.runs_claude && other.space == origin.space);
        match (beside.next(), beside.next()) {
            (Some(only), None) => self.claim(pr, &only.terminal),
            _ => Ok(()),
        }
    }

    fn make_room(&self, active: &PullRequestUrl) -> anyhow::Result<()> {
        let mut agents = self.port.app_started()?;
        if agents.len() < MAX_APP_AGENTS {
            return Ok(());
        }
        agents.sort_by_key(|agent| agent.status_since_ms);
        let to_end = agents
            .iter()
            .find(|agent| agent.status == AgentStatus::Idle && agent.pr.as_ref() != Some(active));
        let Some(to_end) = to_end else {
            let busy: Vec<String> = agents
                .iter()
                .filter(|agent| agent.status != AgentStatus::Idle)
                .map(|agent| {
                    agent
                        .pr
                        .as_ref()
                        .map_or_else(|| agent.terminal.to_string(), ToString::to_string)
                })
                .collect();
            bail!(
                "{MAX_APP_AGENTS} tour agents are already running and none can be ended; busy: {}",
                busy.join(", ")
            );
        };
        self.port
            .kill(&to_end.terminal)
            .with_context(|| format!("could not end the tour agent {}", to_end.terminal))
    }

    pub fn lookup(&self, pr: &PullRequestUrl) -> TourAgentState {
        if let Err(reason) = self.port.link() {
            return TourAgentState::LinkOff(reason);
        }
        self.find(pr)
            .unwrap_or_else(|error| TourAgentState::LinkOff(format!("{error:#}")))
    }

    /// Passes the agent's state now and at each status change, blocking until herdr's stream ends.
    /// `on_change` breaks when nobody wants the changes any more, which ends the follow.
    pub fn follow(
        &self,
        pr: &PullRequestUrl,
        on_change: &mut dyn FnMut(TourAgentState) -> ControlFlow<()>,
    ) {
        let state = self.lookup(pr);
        let followed = match &state {
            TourAgentState::Agent { terminal, .. } => Some(terminal.clone()),
            _ => None,
        };
        if on_change(state).is_break() {
            return;
        }
        let Some(terminal) = followed else {
            return;
        };
        let followed = self.port.follow(&terminal, &mut |status| {
            on_change(TourAgentState::Agent {
                terminal: terminal.clone(),
                status,
            })
        });
        if let Err(error) = followed {
            // The stream is over either way, so there is nothing left to break.
            let _ended = on_change(TourAgentState::LinkOff(format!("{error:#}")));
        }
    }

    /// Types the text into the PR's tour agent only when it is idle: never into a working agent and never
    /// past one that waits for the reviewer.
    pub fn send_if_idle(&self, pr: &PullRequestUrl, text: &str) -> anyhow::Result<SendOutcome> {
        match self.lookup(pr) {
            TourAgentState::Agent {
                terminal,
                status: AgentStatus::Working,
            } => Ok(SendOutcome::Refused(format!("{terminal} is working"))),
            TourAgentState::Agent {
                terminal,
                status: AgentStatus::WaitingForYou,
            } => Ok(SendOutcome::Refused(format!("{terminal} waits for you"))),
            TourAgentState::Agent { terminal, .. } => self.port.send_if_idle(&terminal, text),
            TourAgentState::LinkOff(reason) => Ok(SendOutcome::Refused(format!(
                "tour agents are off: {reason}"
            ))),
            TourAgentState::None | TourAgentState::Unknown => Ok(SendOutcome::NoAgent),
        }
    }

    /// Types an Ask into the PR's tour agent, but only one the reviewer started: an app-started agent
    /// cannot change files, and the reviewer's own one may.
    pub fn hand_to_own_agent(&self, pr: &PullRequestUrl, text: &str) -> OwnAgentHandOff {
        let terminal = match self.lookup(pr) {
            TourAgentState::Agent { terminal, .. } => terminal,
            TourAgentState::LinkOff(reason) => {
                return OwnAgentHandOff::NotSent(format!("tour agents are off: {reason}"));
            }
            TourAgentState::None | TourAgentState::Unknown => return OwnAgentHandOff::NoAgent,
        };
        match self.port.app_started() {
            Ok(by_app) if by_app.iter().any(|agent| agent.terminal == terminal) => {
                return OwnAgentHandOff::StartedByApp(terminal);
            }
            Ok(_) => {}
            Err(error) => return OwnAgentHandOff::NotSent(format!("{error:#}")),
        }
        match self.send_if_idle(pr, text) {
            Ok(SendOutcome::Delivered) => OwnAgentHandOff::Delivered(terminal),
            Ok(SendOutcome::Refused(reason)) => OwnAgentHandOff::NotSent(reason),
            Ok(SendOutcome::Starting) => {
                OwnAgentHandOff::NotSent(format!("{terminal} is still starting"))
            }
            Ok(SendOutcome::NoAgent) => OwnAgentHandOff::NoAgent,
            Err(error) => OwnAgentHandOff::NotSent(format!("{error:#}")),
        }
    }

    pub fn show(&self, pr: &PullRequestUrl) -> anyhow::Result<()> {
        match self.lookup(pr) {
            TourAgentState::Agent { terminal, .. } => self.port.show(&terminal),
            TourAgentState::LinkOff(reason) => bail!("tour agents are off: {reason}"),
            TourAgentState::None | TourAgentState::Unknown => bail!("no tour agent for {pr}"),
        }
    }

    fn find(&self, pr: &PullRequestUrl) -> anyhow::Result<TourAgentState> {
        let Some(terminal) = self.port.terminals_of(pr)?.into_iter().next() else {
            return Ok(TourAgentState::None);
        };
        let status = self.port.status(&terminal)?;
        Ok(TourAgentState::Agent { terminal, status })
    }
}

/// Herdr names are 1-32 of a-z 0-9 - _, so a long repo name is cut and the PR number stays.
fn terminal_name(pr: &PullRequestUrl) -> String {
    let number = format!("-{}", pr.number);
    let repo: String = format!("tour-{}-{}", pr.repo.owner(), pr.repo.name())
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '-'
            }
        })
        .take(32 - number.len())
        .collect();
    format!("{repo}{number}")
}

/// The first task of an agent that writes a tour. It starts with the skill, so Claude runs it; the rest
/// reaches the skill as its arguments. The agent runs with a fixed tool list, under which a chained or
/// unlisted command is denied without a word (docs/wiki/agent-runs.md).
fn writer_task(pr: &PullRequestUrl) -> String {
    format!(
        "/tour {pr} (you are the tour agent of this PR, started by the guided-review app because the reviewer \
         picked it and it has no tour). Only the listed tools are allowed and anything else is denied, never \
         asked: run one command per Bash call, with the PR url written out, without shell variables, pipes, \
         `&&`, `;` or `$(...)`. The Write tool makes the tour's folder. When `guided-review check {pr}` is clean, \
         run `guided-review open {pr}`, which shows the tour to the reviewer. Afterwards the reviewer's comments \
         reach you here as messages: answer each as its message says, fix the tour when a comment shows it is \
         wrong, and never change the PR's code."
    )
}

/// What a tour agent the app started is told first. Hand-offs follow as messages that carry the rest of the protocol.
fn first_task(pr: &PullRequestUrl) -> String {
    format!(
        "You are the tour agent of {pr}: you wrote or know its guided-review tour, and the reviewer's comments on it \
         reach you here as messages. Find the tour file with `guided-review tour-path {pr}` and read it now, so you \
         can answer. Answer each comment as its message says, fix the tour when a comment shows it is wrong, and never \
         change the PR's code. Then wait for the next message."
    )
}
