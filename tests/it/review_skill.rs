use std::path::Path;

use guided_review::app_folders::AppFolders;
use guided_review::claude_cli::{review_agent_denied_tools, review_agent_tools};
use guided_review::review_brief::ReviewBrief;
use guided_review::review_subject::ReviewSubject;
use guided_review::viewer::ReviewPlace;

const AGENT_FOLDER: &str = "/work/shop";
const FINDINGS: &str = "/state/tours/acme/shop/7.findings.json";

fn tools(skill: Option<&str>) -> String {
    review_agent_tools(&own_worktree(), Path::new(FINDINGS), skill)
}

fn own_worktree() -> ReviewPlace {
    ReviewPlace::OwnWorktree {
        clone: "/clones/shop".into(),
    }
}

fn tour_agent_folder() -> ReviewPlace {
    ReviewPlace::TourAgentFolder(AGENT_FOLDER.into())
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
fn a_review_agent_reads_its_folder_and_its_findings_and_may_search_and_start_subagents() {
    let tools = review_agent_tools(&tour_agent_folder(), Path::new(FINDINGS), None);
    let tools = listed(&tools);

    for wanted in [
        "Read(//work/shop/**)",
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

#[test]
fn a_review_agent_that_makes_its_own_worktree_may_enter_and_leave_it_and_check_out_the_head() {
    let tools = tools(None);
    let tools = listed(&tools);

    for wanted in [
        "EnterWorktree",
        "ExitWorktree",
        "Bash(git checkout --detach:*)",
        "Read",
    ] {
        assert!(tools.contains(&wanted), "{wanted} is not in {tools:?}");
    }
}

#[test]
fn nothing_the_review_agent_needs_for_its_own_worktree_is_denied() {
    let denied = review_agent_denied_tools();
    let allowed = tools(None);

    for rule in listed(&denied) {
        let command = rule
            .strip_prefix("Bash(")
            .unwrap()
            .strip_suffix(":*)")
            .unwrap();
        for needed in ["git checkout --detach", "EnterWorktree", "ExitWorktree"] {
            assert!(!needed.starts_with(command), "{rule} denies {needed}");
        }
    }
    assert!(!allowed.contains("git worktree"), "{allowed}");
}

#[test]
fn an_agent_in_the_tour_agents_folder_makes_no_worktree() {
    let tools = review_agent_tools(&tour_agent_folder(), Path::new(FINDINGS), None);

    assert!(!tools.contains("Worktree"), "{tools}");
    assert!(!tools.contains("checkout"), "{tools}");
}
