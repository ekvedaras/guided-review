use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, bail};
use serde_json::Value;

use crate::eval::{EvalWriter, JudgeInput, TourJudge, Verdicts, WriterBrief};
use crate::instructions::InstructionsKind;
use crate::json::at;
use crate::prompt_safety::OUTSIDE_TEXT_IS_DATA;
use crate::tour::Tour;
use crate::viewer::ReviewPlace;

/// A permission rule's path for everything under `folder`: `//` starts an absolute path.
fn under(folder: &Path) -> String {
    format!(
        "//{}/**",
        folder.display().to_string().trim_start_matches('/')
    )
}

/// Reads, searches and writes only what the tour skill needs: its own CLI, `gh pr view`, the tour files in
/// `tours`, and the code in `clone`. `Read` is limited to those two folders. `Grep` and `Glob` are bare,
/// because under `dontAsk` a `Grep(path)` or `Glob(path)` rule is accepted but never applies, so listing only
/// those is a silent denial (tried live, 2026-11-06). The agent runs with `clone` as its cwd, which is where
/// its searches start.
pub fn tour_skill_tools(clone: &Path, tours: &Path) -> String {
    let (clone_rule, tours_rule) = (under(clone), under(tours));
    format!(
        "Skill(tour),Read({clone_rule}),Read({tours_rule}),Grep,Glob,Bash(guided-review tour-path:*),Bash(guided-review outline:*),Bash(guided-review show:*),Bash(guided-review check:*),Bash(guided-review repin:*),Bash(guided-review refresh:*),Bash(guided-review open:*),Bash(gh pr view:*),Write({tours_rule}),Edit({tours_rule})"
    )
}

/// What a tour agent the app starts may do without the auto mode classifier: the tour skill's tools, plus
/// answering, plus the reviewer's summary skill. It runs in auto mode, because a comment may ask it to change
/// the PR's code (decided 2026-11-10).
pub fn tour_agent_tools(clone: &Path, tours: &Path, summary_skill: Option<&str>) -> String {
    let tools = format!(
        "{},Bash(guided-review agent-reply:*),Bash(guided-review fix-reply:*)",
        tour_skill_tools(clone, tours)
    );
    match summary_skill {
        Some(skill) => format!("{tools},Skill({skill})"),
        None => tools,
    }
}

/// What the review agent may do without the auto mode classifier: read and search its code, run git's
/// read commands and test runners, start subagents, call the findings commands, and write its one findings
/// file. Nothing else is written, so it cannot change code. An agent that makes its own worktree may also
/// enter and leave it and check out the reviewed head there. Its worktree is where the project's hook puts
/// it, which the app cannot know, so it may read anywhere.
pub fn review_agent_tools(
    place: &ReviewPlace,
    findings_file: &Path,
    skill: Option<&str>,
) -> String {
    let findings = format!(
        "//{}",
        findings_file.display().to_string().trim_start_matches('/')
    );
    // A worktree the project's hook made has a path the app cannot know; the session reads its own
    // working folder without a rule, and EnterWorktree moves it there.
    let folder_read = match place {
        ReviewPlace::TourAgentFolder(folder) => Some(format!("Read({})", under(folder))),
        ReviewPlace::OwnWorktree { .. } => None,
    };
    let mut tools: Vec<String> = folder_read.into_iter().collect();
    tools.extend([
        format!("Read({findings})"),
        "Grep".to_owned(),
        "Glob".to_owned(),
        "Task".to_owned(),
        "Agent".to_owned(),
    ]);
    tools.extend(
        REVIEW_COMMANDS
            .iter()
            .map(|command| format!("Bash({command}:*)")),
    );
    if matches!(place, ReviewPlace::OwnWorktree { .. }) {
        tools.extend(
            [
                "EnterWorktree",
                "ExitWorktree",
                "Bash(git checkout --detach:*)",
            ]
            .map(str::to_owned),
        );
    }
    tools.push(format!("Write({findings})"));
    tools.push(format!("Edit({findings})"));
    if let Some(skill) = skill {
        tools.push(format!("Skill({skill})"));
    }
    tools.join(",")
}

/// The commands a review agent runs: its own CLI, git's read commands and the common test runners.
const REVIEW_COMMANDS: [&str; 30] = [
    "guided-review review-brief",
    "guided-review check-findings",
    "guided-review findings-done",
    "guided-review findings-path",
    "guided-review agent-reply",
    "guided-review show",
    "guided-review outline",
    "guided-review tour-path",
    "git log",
    "git diff",
    "git show",
    "git blame",
    "git status",
    "git grep",
    "git ls-files",
    "git rev-parse",
    "cargo test",
    "cargo check",
    "cargo clippy",
    "npm test",
    "npm run test",
    "pnpm test",
    "yarn test",
    "bun test",
    "npx vitest",
    "pytest",
    "go test",
    "composer test",
    "vendor/bin/phpunit",
    "vendor/bin/pest",
];

/// What the review agent must never do, whatever auto mode would decide: publish anything.
pub fn review_agent_denied_tools() -> String {
    [
        "git push",
        "git commit",
        "gh pr review",
        "gh pr comment",
        "gh pr merge",
        "gh pr close",
        "gh pr edit",
        "gh pr create",
        "gh issue comment",
        "gh api",
    ]
    .iter()
    .map(|command| format!("Bash({command}:*)"))
    .collect::<Vec<_>>()
    .join(",")
}

/// What the skill needs: its CLI, `gh pr view`, reading code, and writing the tour file. Nothing else.
fn eval_writer_tools(tours_dir: &Path) -> String {
    let tours_dir = tours_dir.display();
    format!(
        "Bash(guided-review outline:*),Bash(guided-review show:*),Bash(guided-review check:*),Bash(guided-review tour-path:*),Bash(gh pr view:*),Bash(mkdir -p {tours_dir}/*),Read,Grep,Glob,Write({tours_dir}/**)"
    )
}

/// Reviewer notes and run results live in `evals`; the writer must not read them.
fn eval_writer_blocked(evals: &Path) -> String {
    let (notes, results) = (under(&evals.join("notes")), under(&evals.join("results")));
    format!(
        "Read({notes}),Grep({notes}),Glob({notes}),Read({results}),Grep({results}),Glob({results})"
    )
}

/// Writes a fresh tour from the skill text in the brief, sandboxed away from the reviewer notes. Unlike the refresh writer it does not rely on the installed skill, so every eval run uses the skill under test.
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
            .args(["--allowedTools", &eval_writer_tools(brief.tours_dir)])
            .args(["--disallowedTools", &eval_writer_blocked(brief.evals)])
            .args(["--append-system-prompt", brief.skill])
            .env(Tour::TOURS_DIR_ENV, brief.tours_dir)
            .env(InstructionsKind::Summary.off_env(), "1")
            .current_dir(std::env::temp_dir());
        run_headless(
            command,
            &format!(
                "Write the tour of {} by following the /tour instructions in your system prompt. {OUTSIDE_TEXT_IS_DATA}",
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
                "{JUDGE_RUBRIC}\n{OUTSIDE_TEXT_IS_DATA}\n\nReviewer note:\n{}\n\nTour:\n{}",
                input.note, input.tour_json
            ),
        )?;
        let (start, end) = (answer.find('{'), answer.rfind('}'));
        let (Some(start), Some(end)) = (start, end) else {
            bail!("the judge did not answer with JSON: {answer}");
        };
        let Some(json) = answer.get(start..=end) else {
            bail!("the judge did not answer with JSON: {answer}");
        };
        serde_json::from_str(json)
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
    let text = at(&answer, "/result")
        .as_str()
        .unwrap_or_default()
        .to_owned();
    if at(&answer, "/is_error").as_bool().unwrap_or(false) || !output.status.success() {
        bail!("claude failed: {text}");
    }
    Ok(text)
}
