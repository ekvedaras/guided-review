use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use anyhow::{Context, bail};
use serde_json::{Value, json};

use crate::viewer::{AgentLaunch, AgentLauncher, AgentPort};

const READ_ONLY_TOOLS: &str = "Read,Grep,Glob,Bash(guided-review show:*)";

/// Runs the user's own `claude` CLI, so answers use their subscription. Never `--bare`: it skips the login.
pub struct ClaudeCli {
    binary: PathBuf,
}

impl ClaudeCli {
    pub fn new() -> Self {
        Self::with_binary(PathBuf::from("claude"))
    }

    pub fn with_binary(binary: PathBuf) -> Self {
        Self { binary }
    }
}

impl Default for ClaudeCli {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentLauncher for ClaudeCli {
    fn start(&self, launch: &AgentLaunch) -> anyhow::Result<Box<dyn AgentPort>> {
        let mut command = Command::new(&self.binary);
        command
            .args(["-p", "--input-format", "stream-json"])
            .args(["--output-format", "stream-json"])
            .args(["--verbose", "--include-partial-messages"])
            .args(["--permission-mode", "dontAsk"])
            .args(["--allowedTools", READ_ONLY_TOOLS])
            .args(["--model", &launch.model])
            .args(["--effort", &launch.effort])
            .args(["--append-system-prompt", &launch.system_prompt]);
        if let Some(session) = &launch.resume {
            command.args(["--resume", session]);
        }
        let mut child = command
            .current_dir(&launch.cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .with_context(|| format!("could not start `{}`", self.binary.display()))?;
        let stdin = child.stdin.take().context("no stdin on claude")?;
        let stdout = BufReader::new(child.stdout.take().context("no stdout on claude")?);
        Ok(Box::new(ClaudeConversation {
            child,
            stdin,
            stdout,
        }))
    }
}

struct ClaudeConversation {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl AgentPort for ClaudeConversation {
    fn ask(&mut self, message: &str, on_line: &mut dyn FnMut(&str)) -> anyhow::Result<()> {
        let input = json!({"type": "user", "message": {"role": "user", "content": message}});
        writeln!(self.stdin, "{input}").context("claude is no longer running")?;
        self.stdin.flush()?;
        let mut line = String::new();
        loop {
            line.clear();
            if self.stdout.read_line(&mut line)? == 0 {
                bail!("claude stopped before it finished answering");
            }
            let line = line.trim_end();
            on_line(line);
            if serde_json::from_str::<Value>(line).is_ok_and(|json| json["type"] == "result") {
                return Ok(());
            }
        }
    }
}

impl Drop for ClaudeConversation {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
