//! The tour agent port over herdr's `hc` CLI, and `herdr-app` to show a terminal.

use std::ffi::OsStr;
use std::io::{BufRead, BufReader};
use std::ops::ControlFlow;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::Duration;

use anyhow::{Context, bail};
use serde_json::Value;

use crate::claude_cli::tour_agent_tools;
use crate::json::at;
use crate::pull_request::PullRequestUrl;
use crate::viewer::{
    AgentStatus, AppAgent, LiveTerminal, SendOutcome, TerminalId, TourAgentPort, TourAgentStart,
};

const APP_LABEL: &str = "app=guided-review";
const STARTED_BY_APP_LABEL: &str = "gr.started-by=app";

/// The herdr-app space every agent the app starts is shown in, one tab each. Herdr-app shows a terminal
/// labelled `herdr.space=<id>`, making a space or tab for an id it does not know, so the agents are
/// placed while the window is closed too.
const AGENTS_SPACE: &str = "guided-review";
/// Not plain "guided-review": a space nobody named is called after its repo, and the maintainer's own
/// space of this repo already is.
const AGENTS_SPACE_NAME: &str = "guided-review agents";

/// Herdr-app takes up a newly labelled terminal a moment after `hc run` returns; until then it knows
/// no space or tab to name.
const ADOPTION_TRIES: u32 = 30;
const ADOPTION_WAIT: Duration = Duration::from_millis(100);

pub struct HcTourAgents {
    hc: PathBuf,
    herdr_app: PathBuf,
    /// The `PATH` an agent the app starts runs with.
    agents_path: String,
}

impl HcTourAgents {
    pub fn new() -> Self {
        let agents_path = std::env::current_exe().map_or_else(
            |_| std::env::var("PATH").unwrap_or_default(),
            |own| Self::path_for_agents(&own, std::env::var_os("PATH").as_deref()),
        );
        Self::with_binaries(PathBuf::from("hc"), PathBuf::from("herdr-app"))
            .giving_agents_path(&agents_path)
    }

    pub fn with_binaries(hc: PathBuf, herdr_app: PathBuf) -> Self {
        Self {
            hc,
            herdr_app,
            agents_path: String::new(),
        }
    }

    pub fn giving_agents_path(mut self, path: &str) -> Self {
        path.clone_into(&mut self.agents_path);
        self
    }

    /// Herdr starts a terminal with its server's environment, not the app's, and Claude's Bash tool
    /// looks commands up in it: without the folder of the running `guided-review` in front, an agent
    /// may find no `guided-review` at all (docs/wiki/agent-runs.md).
    pub fn path_for_agents(guided_review: &Path, path: Option<&OsStr>) -> String {
        let mut folders: Vec<PathBuf> = path
            .map(|path| std::env::split_paths(path).collect())
            .unwrap_or_default();
        if let Some(own) = guided_review.parent()
            && !folders.iter().any(|folder| folder == own)
        {
            folders.insert(0, own.to_owned());
        }
        folders
            .iter()
            .map(|folder| folder.display().to_string())
            .collect::<Vec<_>>()
            .join(":")
    }

    fn hc(&self, args: &[&str]) -> anyhow::Result<Value> {
        let output = Command::new(&self.hc)
            .args(args)
            .stdin(Stdio::null())
            .output()
            .with_context(|| format!("could not run `hc {}`", args.join(" ")))?;
        if !output.status.success() {
            bail!(
                "`hc {}` failed: {}",
                args.join(" "),
                herdr_words(&output.stderr)
            );
        }
        if output.stdout.iter().all(u8::is_ascii_whitespace) {
            return Ok(Value::Null);
        }
        serde_json::from_slice(&output.stdout)
            .with_context(|| format!("`hc {}` did not answer JSON", args.join(" ")))
    }
}

impl Default for HcTourAgents {
    fn default() -> Self {
        Self::new()
    }
}

/// What `hc ls --json` names the Claude Code program in `agent`.
const CLAUDE_AGENT: &str = "claude-code";

/// `where` reads `<space> / <tab>`.
fn space_of(place: &str) -> &str {
    place
        .rsplit_once(" / ")
        .map_or(place, |(space, _tab)| space)
}

fn label_of(pr: &PullRequestUrl) -> String {
    format!("gr.pr={pr}")
}

fn state_of(terminal: &Value) -> AgentStatus {
    let reported = match at(terminal, "/status") {
        Value::String(state) => Some(state.as_str()),
        status => at(status, "/state").as_str(),
    };
    AgentStatus::from_herdr(reported)
}

/// The status an event line reports for the terminal: the snapshot `hc events` starts with lists terminals,
/// and each change after it is a JSON-RPC `status` event naming one.
fn status_in(line: &Value, address: &str) -> Option<AgentStatus> {
    let snapshot = at(line, "/terminals")
        .as_array()
        .and_then(|terminals| terminals.iter().find(|t| at(t, "/address") == address))
        .map(state_of);
    let change = (at(line, "/params/type") == "status" && at(line, "/params/terminal") == address)
        .then(|| AgentStatus::from_herdr(at(line, "/params/data/status/state").as_str()));
    snapshot.or(change)
}

impl TourAgentPort for HcTourAgents {
    fn link(&self) -> Result<(), String> {
        match self.hc(&["server", "status", "--json"]) {
            Ok(_) => Ok(()),
            Err(error) if self.hc_is_missing() => Err(format!(
                "`{}` is not on PATH ({error:#})",
                self.hc.display()
            )),
            Err(error) => Err(format!("the herdr server does not answer: {error:#}")),
        }
    }

    fn terminals_of(&self, pr: &PullRequestUrl) -> anyhow::Result<Vec<TerminalId>> {
        let label = label_of(pr);
        let listed = self.hc(&["ls", "--label", APP_LABEL, "--label", &label, "--json"])?;
        Ok(at(&listed, "/terminals")
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|terminal| at(terminal, "/address").as_str())
            .map(TerminalId::new)
            .collect())
    }

    fn live_terminals(&self) -> anyhow::Result<Vec<LiveTerminal>> {
        let listed = self.hc(&["ls", "--json"])?;
        Ok(at(&listed, "/terminals")
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|terminal| {
                Some(LiveTerminal {
                    terminal: TerminalId::new(at(terminal, "/address").as_str()?),
                    runs_claude: at(terminal, "/agent") == CLAUDE_AGENT,
                    space: space_of(at(terminal, "/where").as_str()?).to_owned(),
                })
            })
            .collect())
    }

    fn claim(&self, terminal: &TerminalId, pr: &PullRequestUrl) -> anyhow::Result<()> {
        self.hc(&["label", terminal.as_str(), APP_LABEL, &label_of(pr)])
            .map(drop)
    }

    fn release(&self, terminal: &TerminalId) -> anyhow::Result<()> {
        self.hc(&["label", terminal.as_str(), "gr.pr="]).map(drop)
    }

    fn status(&self, terminal: &TerminalId) -> anyhow::Result<AgentStatus> {
        let got = self.hc(&["get", terminal.as_str(), "--json"])?;
        Ok(state_of(&got))
    }

    fn follow(
        &self,
        terminal: &TerminalId,
        on_status: &mut dyn FnMut(AgentStatus) -> ControlFlow<()>,
    ) -> anyhow::Result<()> {
        let mut events = Command::new(&self.hc)
            .args(["events", "--json", "--terminal", terminal.as_str()])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .context("could not run `hc events`")?;
        let stdout = events
            .stdout
            .take()
            .context("`hc events` gave no output to read")?;
        let mut last = None;
        for line in BufReader::new(stdout).lines() {
            let line = line.context("could not read `hc events`")?;
            let Ok(json) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            let Some(status) = status_in(&json, terminal.as_str()) else {
                continue;
            };
            if last != Some(status) {
                last = Some(status);
                if on_status(status).is_break() {
                    events.kill().context("could not stop `hc events`")?;
                    events.wait().context("`hc events` did not end")?;
                    return Ok(());
                }
            }
        }
        events.wait().context("`hc events` did not end cleanly")?;
        Ok(())
    }

    fn send_if_idle(&self, terminal: &TerminalId, text: &str) -> anyhow::Result<SendOutcome> {
        let output = Command::new(&self.hc)
            .args(["send", terminal.as_str(), "--if", "idle", text])
            .stdin(Stdio::null())
            .output()
            .context("could not run `hc send`")?;
        if output.status.success() {
            return Ok(SendOutcome::Delivered);
        }
        let refusal: Value = serde_json::from_slice(&output.stderr).unwrap_or(Value::Null);
        // Herdr waits for a new terminal's screen to settle before it types into it; a Claude busy with
        // the first task from its command line can outlast that wait.
        if at(&refusal, "/error/kind") == "unavailable" {
            return Ok(SendOutcome::Starting);
        }
        if at(&refusal, "/error/kind") != "status_mismatch" {
            bail!(
                "herdr did not type it into {terminal}: {}",
                herdr_words(&output.stderr)
            );
        }
        let reason = at(&refusal, "/error/message")
            .as_str()
            .unwrap_or("the agent is not idle");
        // Herdr keeps an agent at `done` after a turn until someone looks at the terminal, and `--if idle`
        // refuses it, so an agent running in the background would never take a comment.
        if reason.contains(" is done, not idle") {
            self.hc(&["send", terminal.as_str(), text])?;
            return Ok(SendOutcome::Delivered);
        }
        Ok(SendOutcome::Refused(reason.to_owned()))
    }

    fn show(&self, terminal: &TerminalId) -> anyhow::Result<()> {
        let got = self.hc(&["get", terminal.as_str(), "--json"])?;
        if at(&got, "/labels/gr.started-by") == "app" {
            self.place_in_agents_space(terminal, &got)?;
        }
        let output = self.herdr_app(&["do", "agent.focus", "--terminal", terminal.as_str()])?;
        if !output.status.success() {
            bail!(
                "`herdr-app do agent.focus` failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Ok(())
    }

    fn app_started(&self) -> anyhow::Result<Vec<AppAgent>> {
        let listed = self.hc(&[
            "ls",
            "--label",
            APP_LABEL,
            "--label",
            STARTED_BY_APP_LABEL,
            "--json",
        ])?;
        at(&listed, "/terminals")
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|terminal| at(terminal, "/address").as_str())
            .map(|address| {
                let got = self.hc(&["get", address, "--json"])?;
                Ok(AppAgent {
                    terminal: TerminalId::new(address),
                    pr: at(&got, "/labels/gr.pr")
                        .as_str()
                        .and_then(|pr| pr.parse().ok()),
                    status: state_of(&got),
                    status_since_ms: at(&got, "/status_ms").as_u64().unwrap_or(0),
                })
            })
            .collect()
    }

    fn start(&self, start: &TourAgentStart) -> anyhow::Result<TerminalId> {
        let folder = start.folder.display().to_string();
        let pr_label = label_of(&start.pr);
        let path = format!("PATH={}", self.agents_path);
        let tab = &start.name;
        let space_label = format!("herdr.space={AGENTS_SPACE}");
        let tab_label = format!("herdr.tab={tab}");
        let args = [
            "run",
            "--bg",
            "--json",
            "--outlive-parent",
            "--no-parent",
            "--name",
            &start.name,
            "--cwd",
            &folder,
            "--label",
            APP_LABEL,
            "--label",
            &pr_label,
            "--label",
            STARTED_BY_APP_LABEL,
            "--label",
            &space_label,
            "--label",
            &tab_label,
            "--",
            "env",
            &path,
            "claude",
            &start.first_task,
            "--permission-mode",
            "dontAsk",
            "--allowedTools",
            &tour_agent_tools(&start.folder),
        ];
        let started = self.hc(&args)?;
        let address = at(&started, "/address")
            .as_str()
            .context("`hc run` did not say which terminal it started")?;
        self.name_place(tab, &start.pr);
        Ok(TerminalId::new(address))
    }

    fn kill(&self, terminal: &TerminalId) -> anyhow::Result<()> {
        self.hc(&["kill", terminal.as_str()]).map(drop)
    }

    fn screen_lines(&self, terminal: &TerminalId, count: usize) -> anyhow::Result<Vec<String>> {
        let read = self.hc(&[
            "read",
            terminal.as_str(),
            "--json",
            "--lines",
            &count.to_string(),
        ])?;
        let text = at(&read, "/text").as_str().unwrap_or_default();
        let lines: Vec<&str> = text.lines().collect();
        let work: Vec<&str> = above_claudes_prompt_box(&lines)
            .iter()
            .map(|line| line.trim_end())
            .filter(|line| !line.is_empty())
            .collect();
        Ok(work
            .iter()
            .skip(work.len().saturating_sub(count))
            .map(|line| (*line).to_owned())
            .collect())
    }
}

/// What herdr said on stderr, in words: the message of its JSON error, or the text as it is.
fn herdr_words(stderr: &[u8]) -> String {
    let said = String::from_utf8_lossy(stderr);
    serde_json::from_str::<Value>(&said)
        .ok()
        .and_then(|error| at(&error, "/error/message").as_str().map(str::to_owned))
        .unwrap_or_else(|| said.trim().to_owned())
}

/// Claude keeps its prompt box and status bar at the bottom of a screen of mostly blank rows, so the
/// last lines of its screen show none of its work. The box starts with a rule over the `❯` line.
fn above_claudes_prompt_box<'a>(lines: &'a [&'a str]) -> &'a [&'a str] {
    let is_rule = |line: &str| {
        let line = line.trim();
        !line.is_empty() && line.chars().all(|c| c == '─')
    };
    let box_top = lines.windows(2).rposition(|pair| match pair {
        [rule, prompt] => is_rule(rule) && prompt.trim_start().starts_with('❯'),
        _ => false,
    });
    box_top.and_then(|top| lines.get(..top)).unwrap_or(lines)
}

/// What came of asking herdr-app to name an agent's space and tab.
enum Naming {
    Named,
    NotTakenUpYet,
    /// No window to ask, or it refused: the agent runs on unnamed, which is no reason to fail its start.
    GivenUp,
}

impl HcTourAgents {
    fn herdr_app(&self, args: &[&str]) -> anyhow::Result<Output> {
        Command::new(&self.herdr_app)
            .args(args)
            .stdin(Stdio::null())
            .output()
            .with_context(|| format!("could not run `{}`", self.herdr_app.display()))
    }

    /// An agent started before the app placed its agents runs hidden from herdr-app; labelling it puts it
    /// in the window. One placed while the window was closed has no names yet.
    fn place_in_agents_space(&self, terminal: &TerminalId, got: &Value) -> anyhow::Result<()> {
        let space = at(got, "/labels/herdr.space");
        let tab = if space.is_null() {
            let tab = at(got, "/name").as_str().unwrap_or(terminal.as_str());
            self.hc(&[
                "label",
                terminal.as_str(),
                &format!("herdr.space={AGENTS_SPACE}"),
                &format!("herdr.tab={tab}"),
            ])?;
            tab
        } else if space == AGENTS_SPACE
            && let Some(tab) = at(got, "/labels/herdr.tab").as_str()
        {
            tab
        } else {
            return Ok(());
        };
        if let Some(pr) = at(got, "/labels/gr.pr")
            .as_str()
            .and_then(|pr| pr.parse().ok())
        {
            self.name_place(tab, &pr);
        }
        Ok(())
    }

    /// Names the agents' space and the agent's tab once herdr-app has taken the terminal up.
    fn name_place(&self, tab: &str, pr: &PullRequestUrl) {
        for _ in 0..ADOPTION_TRIES {
            match self.try_naming(tab, pr) {
                Naming::Named | Naming::GivenUp => return,
                Naming::NotTakenUpYet => std::thread::sleep(ADOPTION_WAIT),
            }
        }
    }

    fn try_naming(&self, tab: &str, pr: &PullRequestUrl) -> Naming {
        let renames = [
            [
                "do",
                "space.rename",
                "--id",
                AGENTS_SPACE,
                "--name",
                AGENTS_SPACE_NAME,
            ],
            ["do", "tab.rename", "--id", tab, "--name", &pr.short()],
        ];
        for rename in renames {
            let Ok(output) = self.herdr_app(&rename) else {
                return Naming::GivenUp;
            };
            if output.status.success() {
                continue;
            }
            let said = String::from_utf8_lossy(&output.stdout);
            return if said.contains("no space") || said.contains("no tab") {
                Naming::NotTakenUpYet
            } else {
                Naming::GivenUp
            };
        }
        Naming::Named
    }

    fn hc_is_missing(&self) -> bool {
        Command::new(&self.hc)
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound)
    }
}
