use std::path::Path;

use guided_review::app_folders::AppFolders;
use guided_review::claude_cli::{review_agent_denied_tools, review_agent_tools};
use guided_review::review_brief::ReviewBrief;
use guided_review::review_subject::ReviewSubject;

const WORKTREE: &str = "/cache/review-worktrees/acme/shop/7";
const FINDINGS: &str = "/state/tours/acme/shop/7.findings.json";

fn tools(skill: Option<&str>) -> String {
    review_agent_tools(Path::new(WORKTREE), Path::new(FINDINGS), skill)
}

fn listed(tools: &str) -> Vec<&str> {
    tools.split(',').collect()
}

fn brief() -> String {
    let subject: ReviewSubject = "https://github.com/acme/shop/pull/7".parse().unwrap();
    let home = tempfile::TempDir::new().unwrap();
    let folders = crate::support::folders_in(home.path());
    let _: &AppFolders = &folders;
    ReviewBrief::of(&folders, &subject).to_string()
}

/// Every command the text names in backticks, such as `guided-review check-findings <pr-url>`.
fn commands_in(text: &str) -> Vec<String> {
    text.split('`')
        .skip(1)
        .step_by(2)
        .filter(|span| {
            ["guided-review ", "gh ", "git "]
                .iter()
                .any(|program| span.starts_with(program))
        })
        .map(str::to_owned)
        .collect()
}

fn allowed_command_prefixes(tools: &str) -> Vec<String> {
    tools
        .split(',')
        .filter_map(|tool| tool.strip_prefix("Bash(")?.strip_suffix(":*)"))
        .map(str::to_owned)
        .collect()
}

#[test]
fn a_review_agent_may_run_every_command_the_brief_asks_for() {
    let commands = commands_in(&brief());
    assert!(commands.len() >= 4, "{commands:?}");
    let allowed = allowed_command_prefixes(&tools(None));

    let denied: Vec<&String> = commands
        .iter()
        .filter(|command| {
            !allowed
                .iter()
                .any(|prefix| command.starts_with(prefix.as_str()))
        })
        .collect();

    assert!(denied.is_empty(), "{denied:?} not in {allowed:?}");
}

#[test]
fn a_review_agent_writes_only_its_findings_file() {
    let tools = tools(None);
    let tools = listed(&tools);

    assert!(
        tools.contains(&"Write(//state/tours/acme/shop/7.findings.json)"),
        "{tools:?}"
    );
    assert!(
        tools.contains(&"Edit(//state/tours/acme/shop/7.findings.json)"),
        "{tools:?}"
    );
    for tool in &tools {
        assert!(
            !["Write", "Edit", "Bash", "NotebookEdit"].contains(tool),
            "bare {tool}"
        );
    }
}

#[test]
fn a_review_agent_reads_its_worktree_and_its_findings_and_may_search_and_start_subagents() {
    let tools = tools(None);
    let tools = listed(&tools);

    for wanted in [
        "Read(//cache/review-worktrees/acme/shop/7/**)",
        "Read(//state/tours/acme/shop/7.findings.json)",
        "Grep",
        "Glob",
        "Task",
        "Bash(git log:*)",
        "Bash(git diff:*)",
    ] {
        assert!(tools.contains(&wanted), "{wanted} is not in {tools:?}");
    }
}

#[test]
fn a_review_agent_may_run_the_tests_but_not_push_or_post() {
    let allowed = tools(None);

    assert!(allowed.contains("Bash(cargo test:*)"), "{allowed}");
    assert!(!allowed.contains("git push"), "{allowed}");
    assert!(!allowed.contains("gh "), "{allowed}");
}

#[test]
fn the_review_skill_is_allowed_when_one_is_set() {
    assert!(listed(&tools(Some("audit"))).contains(&"Skill(audit)"));
    assert!(!tools(None).contains("Skill"));
}

#[test]
fn publishing_is_denied() {
    let denied = review_agent_denied_tools();
    let denied = listed(&denied);

    for rule in [
        "Bash(git push:*)",
        "Bash(gh pr review:*)",
        "Bash(gh pr comment:*)",
        "Bash(gh pr merge:*)",
        "Bash(gh api:*)",
    ] {
        assert!(denied.contains(&rule), "{rule} is not in {denied:?}");
    }
}
