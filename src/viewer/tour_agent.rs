use std::fmt;
use std::ops::ControlFlow;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, bail};
use sha2::{Digest, Sha256};

use super::active_pr::OpenOrigin;
use crate::review_subject::{LocalChangeSet, ReviewSubject};

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
    /// Claude Code runs in it, by herdr's word or by its foreground process, so a question sent to it is read by
    /// Claude, not run by a shell.
    pub runs_claude: bool,
    /// The herdr space (workspace) it lives in.
    pub space: String,
    /// The folder the terminal works in, which tells a local change set's agent from other terminals.
    pub cwd: PathBuf,
}

/// What herdr did with text sent only if the agent is idle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SendOutcome {
    Delivered,
    /// Nothing was typed; herdr's reason, such as "t7 is working, not idle".
    Refused(String),
    /// Nothing was typed: the agent was just started and its screen has not settled yet. Try again shortly.
    Starting,
    /// Nothing was typed: herdr has no state from the agent, so it cannot tell it is idle. The agent may be
    /// waiting on a question, which the reviewer sees by looking at it; only they send it anyway.
    Unreported(TerminalId),
    /// The PR has no tour agent to send to.
    NoAgent,
}

/// A tour agent the app started, as far as ending the right one needs to know it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppAgent {
    pub terminal: TerminalId,
    pub subject: Option<ReviewSubject>,
    pub status: AgentStatus,
    /// Milliseconds since the epoch when the status last changed: for an idle agent, when it last finished.
    pub status_since_ms: u64,
}

/// What a new tour agent is started with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TourAgentStart {
    pub subject: ReviewSubject,
    /// Names the terminal after the repo and the PR number.
    pub name: String,
    pub folder: PathBuf,
    pub first_task: String,
    /// The reviewer's summary instructions skill, allowed to the agent so it is not silently denied.
    pub summary_skill: Option<String>,
}

/// At most this many tour agents the app started stay alive.
const MAX_APP_AGENTS: usize = 3;

/// Why tour agents cannot be reached. Each case asks the reviewer for a different thing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentLinkOff {
    HcMissing,
    /// `hc` is there and herdr's server does not answer.
    ServerNotRunning,
    /// The server answers and a command failed, with the reason herdr gave.
    HerdrFailed(String),
}

impl fmt::Display for AgentLinkOff {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::HcMissing => write!(
                formatter,
                "`hc`, herdr's command line, is not installed or not on PATH"
            ),
            Self::ServerNotRunning => {
                write!(
                    formatter,
                    "the herdr server is not running, start herdr and retry"
                )
            }
            Self::HerdrFailed(reason) => write!(
                formatter,
                "herdr is running but failed: {reason}. `hc server restart` may fix it, but it can pause running terminals"
            ),
        }
    }
}

/// A herdr command that ran and failed. The reason is what herdr said, without the command line.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("`hc {command}` failed: {reason}")]
pub struct HerdrRefused {
    pub command: String,
    pub reason: String,
}

impl AgentLinkOff {
    fn because_of(error: &anyhow::Error) -> Self {
        Self::HerdrFailed(
            error
                .downcast_ref::<HerdrRefused>()
                .map_or_else(|| format!("{error:#}"), |refused| refused.reason.clone()),
        )
    }
}

/// Herdr behind the viewer: where a review subject's tour agent runs and what it does. One adapter,
/// `hc`, which alone knows how a subject is written as a herdr label.
pub trait TourAgentPort: Send + Sync {
    /// Whether tour agents can be reached at all; the reason when not.
    fn link(&self) -> Result<(), AgentLinkOff>;

    /// The live terminals labelled as the tour agent of the subject.
    fn terminals_of(&self, subject: &ReviewSubject) -> anyhow::Result<Vec<TerminalId>>;

    /// Every live terminal herdr lists.
    fn live_terminals(&self) -> anyhow::Result<Vec<LiveTerminal>>;

    /// Labels the terminal as the tour agent of the subject. Releasing the others is the caller's job.
    fn claim(&self, terminal: &TerminalId, subject: &ReviewSubject) -> anyhow::Result<()>;

    /// Takes the subject's label off the terminal.
    fn release(&self, terminal: &TerminalId, subject: &ReviewSubject) -> anyhow::Result<()>;

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

    /// Types the text into the agent and presses enter, whatever its state.
    fn send(&self, terminal: &TerminalId, text: &str) -> anyhow::Result<()>;

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
    LinkOff(AgentLinkOff),
    None,
    Agent {
        terminal: TerminalId,
        status: AgentStatus,
    },
    /// A local change set's folder has several Claude terminals and none is claimed: the reviewer picks.
    Choose(Vec<TerminalId>),
}

impl TourAgentState {
    pub fn can_show(&self) -> bool {
        matches!(self, Self::Agent { .. })
    }

    /// The sidebar offers Retry beside the reason tour agents are off.
    pub fn can_retry(&self) -> bool {
        matches!(self, Self::LinkOff(_))
    }

    /// There is an agent, or news about why there cannot be one; "none" and "looking" say nothing.
    pub fn is_worth_a_line(&self) -> bool {
        matches!(
            self,
            Self::Agent { .. } | Self::LinkOff(_) | Self::Choose(_)
        )
    }

    /// The terminals the sidebar offers "Use this agent" for.
    pub fn choices(&self) -> &[TerminalId] {
        match self {
            Self::Choose(terminals) => terminals,
            _ => &[],
        }
    }

    /// What the sidebar says, in the words of what the subject's agent is: a PR's tour agent, a local
    /// change set's agent.
    pub fn line_for(&self, subject: &ReviewSubject) -> String {
        let who = match subject {
            ReviewSubject::PullRequest(_) => "Tour agent",
            ReviewSubject::Local(_) => "Agent",
        };
        self.line_as(who)
    }

    fn line_as(&self, who: &str) -> String {
        match self {
            Self::Unknown => format!("{who}: looking"),
            Self::LinkOff(reason) => format!("{who} is off: {reason}"),
            Self::None => format!("{who}: none"),
            Self::Agent { terminal, status } => format!("{who} `{terminal}`: {status}"),
            Self::Choose(terminals) => {
                let named: Vec<String> = terminals
                    .iter()
                    .map(|terminal| format!("`{terminal}`"))
                    .collect();
                format!("Several agents work in this folder: {}", named.join(", "))
            }
        }
    }
}

impl fmt::Display for TourAgentState {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str(&self.line_as("Tour agent"))
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
    /// A local change set whose folder had no agent, so one was started with the Ask as its task.
    Started(TerminalId),
    /// A local change set whose folder has several agents and none is picked.
    PickAnAgent,
    /// Nothing was typed: the agent does not report its state, so it may be waiting on a question.
    Unreported(TerminalId),
}

impl OwnAgentHandOff {
    /// Held back from an agent that does not report its state: the reviewer may send it anyway.
    pub fn can_send_anyway(&self) -> bool {
        matches!(self, Self::Unreported(_))
    }

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
            Self::Started(terminal) => {
                format!("Started an agent {terminal} in this review's folder with your Ask")
            }
            Self::PickAnAgent => "Not handed: several agents work in this review's folder. Pick one with Use this agent in the sidebar".to_owned(),
            Self::Unreported(terminal) => format!(
                "Not handed: your agent {terminal} does not report its state. Look at it first, it may be waiting on a question"
            ),
        }
    }
}

/// The tour agents of review subjects, found by a herdr label (`gr.pr=<url>` for a PR), never by a
/// stored terminal id.
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
    pub fn start(
        &self,
        subject: &ReviewSubject,
        active: &ReviewSubject,
        tour: TourKind,
    ) -> anyhow::Result<()> {
        if let Err(reason) = self.port.link() {
            bail!("tour agents are off: {reason}");
        }
        self.launch(subject, active, first_task(subject, tour))
            .map(drop)
    }

    fn launch(
        &self,
        subject: &ReviewSubject,
        active: &ReviewSubject,
        first_task: String,
    ) -> anyhow::Result<TerminalId> {
        let folder = match subject {
            ReviewSubject::Local(change_set) => Some(change_set.folder().as_path().to_owned()),
            ReviewSubject::PullRequest(_) => self.folder.clone(),
        };
        let Some(folder) = folder else {
            bail!("no folder to start a tour agent for {subject} in");
        };
        let name = terminal_name(subject);
        self.make_room(active)?;
        self.port.start(&TourAgentStart {
            subject: subject.clone(),
            name,
            folder,
            first_task,
            summary_skill: self.summary_skill.clone(),
        })
    }

    /// Starts the tour agent that writes the PR's tour with the tour skill. It replaces any agent the PR had:
    /// one the app started is ended, one the reviewer started only loses the PR's label.
    pub fn write_tour(
        &self,
        subject: &ReviewSubject,
        active: &ReviewSubject,
    ) -> anyhow::Result<TerminalId> {
        if let Err(reason) = self.port.link() {
            bail!("tour agents are off: {reason}");
        }
        refuse_replacing_the_agent_of_local(subject)?;
        let app_started: Vec<TerminalId> = self
            .port
            .app_started()?
            .into_iter()
            .map(|agent| agent.terminal)
            .collect();
        for old in self.port.terminals_of(subject)? {
            if app_started.contains(&old) {
                self.port.kill(&old).with_context(|| {
                    format!("could not end the old tour agent {old} of {subject}")
                })?;
            } else {
                self.port
                    .release(&old, subject)
                    .with_context(|| format!("could not take the label of {subject} off {old}"))?;
            }
        }
        self.launch(subject, active, writer_task(subject))
    }

    /// Hands the writer task of a basic tour's guided tour to the PR's tour agent: typed in when the
    /// agent is idle, never into a working one and never ending it; with no agent, one is started with
    /// the task. Anything else means the task waits and this is tried again.
    pub fn hand_writer_task(
        &self,
        subject: &ReviewSubject,
        active: &ReviewSubject,
    ) -> anyhow::Result<WriterHandOver> {
        if let Err(reason) = self.port.link() {
            bail!("tour agents are off: {reason}");
        }
        let task = basic_writer_task(subject);
        match self.lookup(subject) {
            TourAgentState::Agent {
                terminal,
                status: AgentStatus::Working | AgentStatus::WaitingForYou,
            } => Ok(WriterHandOver::Waiting(terminal)),
            TourAgentState::Agent { terminal, .. } => {
                match self.port.send_if_idle(&terminal, &task)? {
                    SendOutcome::Delivered => Ok(WriterHandOver::Delivered(terminal)),
                    SendOutcome::Refused(_)
                    | SendOutcome::Starting
                    | SendOutcome::NoAgent
                    | SendOutcome::Unreported(_) => Ok(WriterHandOver::Waiting(terminal)),
                }
            }
            TourAgentState::LinkOff(reason) => bail!("tour agents are off: {reason}"),
            TourAgentState::Choose(_) => bail!(SEVERAL_AGENTS),
            TourAgentState::None | TourAgentState::Unknown => self
                .launch(subject, active, task)
                .map(WriterHandOver::Started),
        }
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
    pub fn claim(&self, subject: &ReviewSubject, terminal: &TerminalId) -> anyhow::Result<()> {
        if let Err(reason) = self.port.link() {
            bail!("tour agents are off: {reason}");
        }
        for other in self.port.terminals_of(subject)? {
            if &other != terminal {
                self.port.release(&other, subject).with_context(|| {
                    format!("could not take the label of {subject} off {other}")
                })?;
            }
        }
        self.port
            .claim(terminal, subject)
            .with_context(|| format!("could not make {terminal} the tour agent of {subject}"))
    }

    /// What an `open` request does with the terminal it came from. Only a terminal running Claude is ever
    /// claimed, because `hc send` types into it: a terminal running Claude is claimed, a plain shell hands over
    /// to the one Claude terminal of its herdr space, and with none or several (a guess would put the PR's
    /// questions into an unrelated session) nothing is claimed. A request from outside herdr claims nothing.
    pub fn claim_for_open(
        &self,
        subject: &ReviewSubject,
        terminal: Option<&str>,
    ) -> anyhow::Result<()> {
        if let Some(change_set) = subject.local() {
            return self.claim_for_open_of_local(subject, change_set, terminal);
        }
        let Some(address) = terminal else {
            return Ok(());
        };
        let live = self.port.live_terminals()?;
        let Some(origin) = live.iter().find(|live| live.terminal.as_str() == address) else {
            return Ok(());
        };
        if origin.runs_claude {
            return self.claim(subject, &origin.terminal);
        }
        let mut beside = live
            .iter()
            .filter(|other| other.runs_claude && other.space == origin.space);
        match (beside.next(), beside.next()) {
            (Some(only), None) => self.claim(subject, &only.terminal),
            _ => Ok(()),
        }
    }

    /// A local change set's agent is the terminal that ran `open` when it runs Claude, else the only
    /// Claude terminal working in the folder, wherever the request came from. With several there, nothing
    /// is claimed: the reviewer picks.
    fn claim_for_open_of_local(
        &self,
        subject: &ReviewSubject,
        change_set: &LocalChangeSet,
        terminal: Option<&str>,
    ) -> anyhow::Result<()> {
        let live = self.port.live_terminals()?;
        let caller = terminal.and_then(|address| {
            live.iter()
                .find(|live| live.runs_claude && live.terminal.as_str() == address)
        });
        if let Some(caller) = caller {
            return self.claim(subject, &caller.terminal);
        }
        match claude_terminals_in(&live, change_set).as_slice() {
            [only] => self.claim(subject, only),
            _ => Ok(()),
        }
    }

    fn make_room(&self, active: &ReviewSubject) -> anyhow::Result<()> {
        let mut agents = self.port.app_started()?;
        if agents.len() < MAX_APP_AGENTS {
            return Ok(());
        }
        agents.sort_by_key(|agent| agent.status_since_ms);
        let to_end = agents.iter().find(|agent| {
            agent.status == AgentStatus::Idle && agent.subject.as_ref() != Some(active)
        });
        let Some(to_end) = to_end else {
            let busy: Vec<String> = agents
                .iter()
                .filter(|agent| agent.status != AgentStatus::Idle)
                .map(|agent| {
                    agent
                        .subject
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
        let state = self.lookup(subject);
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
            let _ended = on_change(TourAgentState::LinkOff(AgentLinkOff::because_of(&error)));
        }
    }

    /// Types the text into the PR's tour agent only when it is idle: never into a working agent and never
    /// past one that waits for the reviewer.
    pub fn send_if_idle(&self, subject: &ReviewSubject, text: &str) -> anyhow::Result<SendOutcome> {
        match self.lookup(subject) {
            TourAgentState::Agent {
                terminal,
                status: AgentStatus::Working,
            } => Ok(SendOutcome::Refused(format!("{terminal} is working"))),
            TourAgentState::Agent {
                terminal,
                status: AgentStatus::WaitingForYou,
            } => Ok(SendOutcome::Refused(format!("{terminal} waits for you"))),
            TourAgentState::Agent { terminal, .. } => self.port.send_if_idle(&terminal, text),
            TourAgentState::Choose(_) => Ok(SendOutcome::Refused(SEVERAL_AGENTS.to_owned())),
            TourAgentState::LinkOff(reason) => Ok(SendOutcome::Refused(format!(
                "tour agents are off: {reason}"
            ))),
            TourAgentState::None | TourAgentState::Unknown => Ok(SendOutcome::NoAgent),
        }
    }

    /// Types the text into the subject's tour agent whatever its state, once the reviewer has looked at an
    /// agent that does not report its state and chose to send anyway.
    pub fn send_anyway(&self, subject: &ReviewSubject, text: &str) -> anyhow::Result<SendOutcome> {
        match self.lookup(subject) {
            TourAgentState::Agent { terminal, .. } => {
                self.port.send(&terminal, text)?;
                Ok(SendOutcome::Delivered)
            }
            TourAgentState::Choose(_) => Ok(SendOutcome::Refused(SEVERAL_AGENTS.to_owned())),
            TourAgentState::LinkOff(reason) => Ok(SendOutcome::Refused(format!(
                "tour agents are off: {reason}"
            ))),
            TourAgentState::None | TourAgentState::Unknown => Ok(SendOutcome::NoAgent),
        }
    }

    /// Types an Ask into the PR's tour agent, but only one the reviewer started: an app-started agent
    /// cannot change files, and the reviewer's own one may.
    pub fn hand_to_own_agent(&self, subject: &ReviewSubject, text: &str) -> OwnAgentHandOff {
        self.hand_to_own(subject, text, false)
    }

    /// Types an Ask the reviewer chose to send anyway into their agent, whatever its state.
    pub fn hand_to_own_agent_anyway(&self, subject: &ReviewSubject, text: &str) -> OwnAgentHandOff {
        self.hand_to_own(subject, text, true)
    }

    fn hand_to_own(&self, subject: &ReviewSubject, text: &str, anyway: bool) -> OwnAgentHandOff {
        let terminal = match self.lookup(subject) {
            TourAgentState::Agent { terminal, .. } => terminal,
            TourAgentState::LinkOff(reason) => {
                return OwnAgentHandOff::NotSent(format!("tour agents are off: {reason}"));
            }
            TourAgentState::Choose(_) => return OwnAgentHandOff::PickAnAgent,
            TourAgentState::None | TourAgentState::Unknown => {
                return match subject.local() {
                    Some(change_set) => self.start_with_ask(subject, change_set, text),
                    None => OwnAgentHandOff::NoAgent,
                };
            }
        };
        // A PR's app-started agent only reads the PR; a change set's agent changes the code in its folder.
        if subject.pull_request().is_some() {
            match self.port.app_started() {
                Ok(by_app) if by_app.iter().any(|agent| agent.terminal == terminal) => {
                    return OwnAgentHandOff::StartedByApp(terminal);
                }
                Ok(_) => {}
                Err(error) => return OwnAgentHandOff::NotSent(format!("{error:#}")),
            }
        }
        let sent = if anyway {
            self.send_anyway(subject, text)
        } else {
            self.send_if_idle(subject, text)
        };
        match sent {
            Ok(SendOutcome::Delivered) => OwnAgentHandOff::Delivered(terminal),
            Ok(SendOutcome::Refused(reason)) => OwnAgentHandOff::NotSent(reason),
            Ok(SendOutcome::Starting) => {
                OwnAgentHandOff::NotSent(format!("{terminal} is still starting"))
            }
            Ok(SendOutcome::NoAgent) => OwnAgentHandOff::NoAgent,
            Ok(SendOutcome::Unreported(terminal)) => OwnAgentHandOff::Unreported(terminal),
            Err(error) => OwnAgentHandOff::NotSent(format!("{error:#}")),
        }
    }

    fn start_with_ask(
        &self,
        subject: &ReviewSubject,
        change_set: &LocalChangeSet,
        text: &str,
    ) -> OwnAgentHandOff {
        let first_task = format!(
            "{}\n\nThe reviewer's first task:\n{text}",
            change_set_first_task(subject, change_set)
        );
        match self.launch(subject, subject, first_task) {
            Ok(terminal) => OwnAgentHandOff::Started(terminal),
            Err(error) => OwnAgentHandOff::NotSent(format!("{error:#}")),
        }
    }

    pub fn show(&self, subject: &ReviewSubject) -> anyhow::Result<()> {
        match self.lookup(subject) {
            TourAgentState::Agent { terminal, .. } => self.port.show(&terminal),
            TourAgentState::LinkOff(reason) => bail!("tour agents are off: {reason}"),
            TourAgentState::Choose(_) => bail!(SEVERAL_AGENTS),
            TourAgentState::None | TourAgentState::Unknown => bail!("no tour agent for {subject}"),
        }
    }

    fn find(&self, subject: &ReviewSubject) -> anyhow::Result<TourAgentState> {
        let Some(terminal) = self.port.terminals_of(subject)?.into_iter().next() else {
            return self.find_unclaimed_in_folder(subject);
        };
        let status = self.port.status(&terminal)?;
        Ok(TourAgentState::Agent { terminal, status })
    }
}

const SEVERAL_AGENTS: &str = "several agents work in this folder: pick one with Use this agent";

impl TourAgents {
    /// A local change set nobody claimed an agent for: the only Claude terminal working in its folder
    /// becomes its agent, several are offered to the reviewer. A PR has no folder to look in.
    fn find_unclaimed_in_folder(&self, subject: &ReviewSubject) -> anyhow::Result<TourAgentState> {
        let Some(change_set) = subject.local() else {
            return Ok(TourAgentState::None);
        };
        let mut candidates = claude_terminals_in(&self.port.live_terminals()?, change_set);
        match candidates.len() {
            0 => Ok(TourAgentState::None),
            1 => {
                let terminal = candidates.remove(0);
                self.claim(subject, &terminal)?;
                let status = self.port.status(&terminal)?;
                Ok(TourAgentState::Agent { terminal, status })
            }
            _ => Ok(TourAgentState::Choose(candidates)),
        }
    }
}

/// Whether a terminal works in the folder itself or a plain subfolder of it. A subfolder that is the root
/// of another git worktree (`.git` there) belongs to that worktree's branch, not to the folder's.
fn works_in(cwd: &Path, folder: &Path) -> bool {
    cwd.starts_with(folder)
        && cwd
            .ancestors()
            .take_while(|ancestor| *ancestor != folder)
            .all(|ancestor| !ancestor.join(".git").exists())
}

fn claude_terminals_in(live: &[LiveTerminal], change_set: &LocalChangeSet) -> Vec<TerminalId> {
    live.iter()
        .filter(|live| live.runs_claude && works_in(&live.cwd, change_set.folder().as_path()))
        .map(|live| live.terminal.clone())
        .collect()
}

/// Herdr names are 1-32 of a-z 0-9 - _, so a long repo name is cut and the PR number stays. A local change
/// set's name ends on a short hash of its key, so a long folder or branch name stays unique.
fn terminal_name(subject: &ReviewSubject) -> String {
    let (head, tail) = match subject {
        ReviewSubject::PullRequest(pr) => (
            format!("tour-{}-{}", pr.repo.owner(), pr.repo.name()),
            format!("-{}", pr.number),
        ),
        ReviewSubject::Local(change_set) => {
            let digest = Sha256::digest(subject.to_string().as_bytes());
            (
                format!(
                    "local-{}-{}",
                    change_set.folder().name(),
                    change_set.branch().as_str()
                ),
                format!(
                    "-{}",
                    crate::hex::lower(digest.iter().take(NAME_HASH_BYTES))
                ),
            )
        }
    };
    let head: String = head
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '-'
            }
        })
        .take(32 - tail.len())
        .collect();
    format!("{head}{tail}")
}

/// Three bytes are the 6 hex characters that end a local change set's agent name.
const NAME_HASH_BYTES: usize = 3;

/// `write_tour` replaces the subject's agent, which for a local change set is the reviewer's own agent
/// that wrote the code. A local review always has its basic tour, so its guided tour is written through
/// `hand_writer_task`, which keeps that agent.
fn refuse_replacing_the_agent_of_local(subject: &ReviewSubject) -> anyhow::Result<()> {
    if subject.local().is_some() {
        bail!("the agent of a local change set is never replaced to write its guided tour");
    }
    Ok(())
}

/// The first task of an agent that writes a tour. It starts with the skill, so Claude runs it; the rest
/// reaches the skill as its arguments. The agent runs with a fixed tool list, under which a chained or
/// unlisted command is denied without a word (docs/wiki/agent-runs.md).
fn writer_task(subject: &ReviewSubject) -> String {
    format!(
        "/tour {subject} (you are the tour agent of this PR, started by the guided-review app because the reviewer \
         picked it and it has no tour). Only the listed tools are allowed and anything else is denied, never \
         asked: run one command per Bash call, with the PR url written out, without shell variables, pipes, \
         `&&`, `;` or `$(...)`. The Write tool makes the tour's folder. When `guided-review check {subject}` is clean, \
         run `guided-review open {subject}`, which shows the tour to the reviewer. Afterwards the reviewer's comments \
         reach you here as messages: answer each as its message says, fix the tour when a comment shows it is \
         wrong, and never change the PR's code."
    )
}

/// What became of the writer task handed to a PR's tour agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriterHandOver {
    /// Typed into the idle agent.
    Delivered(TerminalId),
    /// The agent is busy or not ready; the task waits for it.
    Waiting(TerminalId),
    /// The PR had no agent, so one was started with the task as its first.
    Started(TerminalId),
}

/// The writer task for a PR the reviewer reads as a basic tour. The basic tour is not a tour file, and the
/// guided tour never replaces it on disk, so the agent writes a whole tour at the guided path.
fn basic_writer_task(subject: &ReviewSubject) -> String {
    if subject.local().is_some() {
        return change_set_writer_task(subject);
    }
    format!(
        "/tour {subject} (you are the tour agent of this PR. The reviewer reads it as a basic tour in the app, with no \
         tour text, and asked for a guided tour, which they keep reading the basic tour meanwhile. Write a full new \
         tour at the path `guided-review tour-path {subject}` prints: the basic tour is not a tour file and needs no \
         edit.) Run one command per Bash call, with the PR url written out, without shell variables, pipes, `&&`, \
         `;` or `$(...)`. When `guided-review check {subject}` is clean, run `guided-review open {subject}`, which tells the \
         app the guided tour is ready and does not move the reviewer's screen. Afterwards the reviewer's comments \
         reach you here as messages: answer each as its message says, fix the tour when a comment shows it is \
         wrong, and never change the PR's code."
    )
}

/// The writer task for a local change set's agent, which wrote the code and is not on a fixed tool list.
fn change_set_writer_task(subject: &ReviewSubject) -> String {
    format!(
        "/tour {subject} (you are the agent of this local change set. The reviewer reads it as a basic tour in the \
         app, with no tour text, and asked for a guided tour, which they keep reading the basic tour meanwhile. Write a \
         full new tour at the path `guided-review tour-path {subject}` prints: the basic tour is not a tour file and \
         needs no edit.) Run one command per Bash call, with the key written out, without shell variables, pipes, \
         `&&`, `;` or `$(...)`. When `guided-review check {subject}` is clean, run `guided-review open {subject}`, \
         which tells the app the guided tour is ready and does not move the reviewer's screen. Do not change the \
         code while you write the tour. Afterwards the reviewer's comments reach you here as tasks: act on each as \
         its message says."
    )
}

/// What a tour agent the app started is told first. Hand-offs follow as messages that carry the rest of the protocol.
fn first_task(subject: &ReviewSubject, tour: TourKind) -> String {
    if let Some(change_set) = subject.local() {
        return change_set_first_task(subject, change_set);
    }
    match tour {
        TourKind::Guided => format!(
            "You are the tour agent of {subject}: you wrote or know its guided-review tour, and the reviewer's comments on it \
             reach you here as messages. Find the tour file with `guided-review tour-path {subject}` and read it now, so you \
             can answer. Answer each comment as its message says, fix the tour when a comment shows it is wrong, and never \
             change the PR's code. Then wait for the next message."
        ),
        TourKind::Basic => format!(
            "You are the tour agent of {subject}: the reviewer reads it as a basic tour, its changed files with no tour text, \
             and their comments on the code reach you here as messages. Read the changed code now with \
             `guided-review outline {subject}`, and `guided-review show {subject} <piece ids> --around` for more of it, so you can \
             answer. Answer each comment as its message says, and never change the PR's code. Then wait for the next message."
        ),
    }
}

/// What the change set's agent the app starts is told first. Unlike a PR's tour agent it changes code: in its
/// own folder, and it pushes nothing.
fn change_set_first_task(subject: &ReviewSubject, change_set: &LocalChangeSet) -> String {
    format!(
        "You are the agent of the local change set {subject}: the reviewer reads the changes of branch {} in {} \
         (commits not on the default branch, and uncommitted files) in guided-review, and their comments reach \
         you here as tasks. Read the change now with `guided-review outline {subject}`, and \
         `guided-review show {subject} <piece ids> --around` for more of it, so you can act on a task. Change \
         code in this folder when a task asks for it, do not push, and commit only if your session normally \
         would. Then wait for the next message.",
        change_set.branch().as_str(),
        change_set.folder().as_path().display()
    )
}

/// What a tour agent started by the app is told the reviewer reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TourKind {
    Guided,
    Basic,
}
