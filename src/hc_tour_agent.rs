//! The tour agent port over herdr's `hc` CLI, and `herdr-app` to show a terminal.

use std::io::{BufRead, BufReader};
use std::ops::ControlFlow;
use std::path::PathBuf;
use std::process::{Command, Stdio};

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

pub struct HcTourAgents {
    hc: PathBuf,
    herdr_app: PathBuf,
}

impl HcTourAgents {
    pub fn new() -> Self {
        Self::with_binaries(PathBuf::from("hc"), PathBuf::from("herdr-app"))
    }

    pub fn with_binaries(hc: PathBuf, herdr_app: PathBuf) -> Self {
        Self { hc, herdr_app }
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
                String::from_utf8_lossy(&output.stderr).trim()
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
        if at(&refusal, "/error/kind") != "status_mismatch" {
            bail!(
                "`hc send` failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
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
        let output = Command::new(&self.herdr_app)
            .args(["do", "agent.focus", "--terminal", terminal.as_str()])
            .stdin(Stdio::null())
            .output()
            .with_context(|| format!("could not run `{}`", self.herdr_app.display()))?;
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
            "--",
            "claude",
            &start.first_task,
            "--permission-mode",
            "dontAsk",
            "--allowedTools",
            &tour_agent_tools(),
        ];
        let started = self.hc(&args)?;
        let address = at(&started, "/address")
            .as_str()
            .context("`hc run` did not say which terminal it started")?;
        Ok(TerminalId::new(address))
    }

    fn kill(&self, terminal: &TerminalId) -> anyhow::Result<()> {
        self.hc(&["kill", terminal.as_str()]).map(drop)
    }
}

impl HcTourAgents {
    fn hc_is_missing(&self) -> bool {
        Command::new(&self.hc)
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound)
    }
}
