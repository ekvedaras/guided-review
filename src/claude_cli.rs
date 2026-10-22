use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use anyhow::{Context, bail};
use serde_json::{Value, json};

use crate::eval::{EvalWriter, JudgeInput, TourJudge, Verdicts, WriterBrief};
use crate::pull_request::PullRequestUrl;
use crate::viewer::{AgentLaunch, AgentLauncher, AgentPort, TourWriter};

const READ_ONLY_TOOLS: &str = "Read,Grep,Glob,Bash(guided-review show:*)";

/// Reads and writes only what the tour skill needs: its own CLI, `gh pr view`, and the tour files.
const TOUR_WRITER_TOOLS: &str = "Skill(tour),Read,Grep,Glob,Bash(guided-review outline:*),Bash(guided-review show:*),Bash(guided-review check:*),Bash(guided-review repin:*),Bash(guided-review refresh:*),Bash(gh pr view:*),Write(~/.guided-review/tours/**),Edit(~/.guided-review/tours/**)";

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

/// Runs the tour skill as a Refresh through the user's own `claude`, in one non-interactive run.
pub struct ClaudeTourWriter {
    binary: PathBuf,
    cwd: PathBuf,
}

impl ClaudeTourWriter {
    /// `cwd` is where the skill may read code: the repo clone when there is one.
    pub fn new(cwd: PathBuf) -> Self {
        Self::with_binary(PathBuf::from("claude"), cwd)
    }

    pub fn with_binary(binary: PathBuf, cwd: PathBuf) -> Self {
        Self { binary, cwd }
    }
}

impl TourWriter for ClaudeTourWriter {
    fn refresh(&self, pr: &PullRequestUrl) -> anyhow::Result<()> {
        let prompt = format!(
            "Run the tour skill as `/tour {pr}` on a PR that already has a tour, which makes it a Refresh."
        );
        let output = Command::new(&self.binary)
            .args(["-p", &prompt])
            .args(["--permission-mode", "dontAsk"])
            .args(["--allowedTools", TOUR_WRITER_TOOLS])
            .current_dir(&self.cwd)
            .stdin(Stdio::null())
            .output()
            .with_context(|| format!("could not start `{}`", self.binary.display()))?;
        if !output.status.success() {
            bail!(
                "claude stopped: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Ok(())
    }
}

/// What the skill needs: its CLI, `gh pr view`, reading code, and writing the tour file. Nothing else.
const EVAL_WRITER_TOOLS: &str = "Bash(guided-review outline:*),Bash(guided-review show:*),Bash(guided-review check:*),Bash(gh pr view:*),Bash(mkdir -p ~/.guided-review/tours/*),Read,Grep,Glob,Write(~/.guided-review/tours/**)";

/// Reviewer notes live here; the writer must not read them.
const EVAL_WRITER_BLOCKED: &str = "Read(~/.guided-review/evals/**),Grep(~/.guided-review/evals/**),Glob(~/.guided-review/evals/**)";

pub struct ClaudeEvalWriter {
    binary: PathBuf,
}

impl ClaudeEvalWriter {
    pub fn new() -> Self {
        Self::with_binary(PathBuf::from("claude"))
    }

    pub fn with_binary(binary: PathBuf) -> Self {
        Self { binary }
    }
}

impl Default for ClaudeEvalWriter {
    fn default() -> Self {
        Self::new()
    }
}

impl EvalWriter for ClaudeEvalWriter {
    fn write_tour(&self, brief: &WriterBrief) -> anyhow::Result<()> {
        let mut command = Command::new(&self.binary);
        command
            .args(["-p", "--output-format", "json"])
            .args(["--permission-mode", "dontAsk"])
            .args(["--allowedTools", EVAL_WRITER_TOOLS])
            .args(["--disallowedTools", EVAL_WRITER_BLOCKED])
            .args(["--append-system-prompt", brief.skill])
            .current_dir(std::env::temp_dir());
        run_headless(
            command,
            &format!(
                "Write the tour of {} by following the /tour instructions in your system prompt.",
                brief.pr
            ),
        )
        .map(drop)
    }
}

pub struct ClaudeTourJudge {
    binary: PathBuf,
}

impl ClaudeTourJudge {
    pub fn new() -> Self {
        Self::with_binary(PathBuf::from("claude"))
    }

    pub fn with_binary(binary: PathBuf) -> Self {
        Self { binary }
    }
}

impl Default for ClaudeTourJudge {
    fn default() -> Self {
        Self::new()
    }
}

const JUDGE_RUBRIC: &str = r#"You judge a guided-review tour of a pull request against the reviewer's note.
The tour is JSON: steps (nested), each with title, why, optional check, tldr, diagram, skim flag and pieces (pointers to code). You cannot see the code; judge the structure and text.

Score each rule pass, partial or fail, with a one-line reason:
- order: the main flow comes first and matches the note's flow.
- grouping: one idea per step; tests sit with their subject.
- diagram: diagrams appear where the note asks for them, and nowhere else.
- text: `why` says why, not what; risky steps carry a `check` hint.
- skim: moves, formatting-only changes and generated code are marked skim.

Answer with only this JSON, no other text:
{"order":{"score":"pass|partial|fail","reason":"..."},"grouping":{...},"diagram":{...},"text":{...},"skim":{...}}
"#;

impl TourJudge for ClaudeTourJudge {
    fn judge(&self, input: &JudgeInput) -> anyhow::Result<Verdicts> {
        let mut command = Command::new(&self.binary);
        command
            .args(["-p", "--output-format", "json"])
            .args(["--tools", ""])
            .current_dir(std::env::temp_dir());
        let answer = run_headless(
            command,
            &format!(
                "{JUDGE_RUBRIC}\nReviewer note:\n{}\n\nTour:\n{}",
                input.note, input.tour_json
            ),
        )?;
        let (start, end) = (answer.find('{'), answer.rfind('}'));
        let (Some(start), Some(end)) = (start, end) else {
            bail!("the judge did not answer with JSON: {answer}");
        };
        serde_json::from_str(&answer[start..=end])
            .with_context(|| format!("could not read the judge's verdicts: {answer}"))
    }
}

/// Runs one headless `claude -p` call and returns the text of its answer.
fn run_headless(mut command: Command, prompt: &str) -> anyhow::Result<String> {
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("could not start `claude`")?;
    let mut stdin = child.stdin.take().context("no stdin on claude")?;
    stdin.write_all(prompt.as_bytes())?;
    drop(stdin);
    let output = child.wait_with_output()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let answer: Value = serde_json::from_str(stdout.trim()).with_context(|| {
        format!(
            "claude gave no JSON answer: {}",
            String::from_utf8_lossy(&output.stderr)
        )
    })?;
    let text = answer["result"].as_str().unwrap_or_default().to_string();
    if answer["is_error"].as_bool().unwrap_or(false) || !output.status.success() {
        bail!("claude failed: {text}");
    }
    Ok(text)
}
