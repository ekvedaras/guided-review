use guided_review::claude_cli::tour_agent_tools;

const SKILL: &str = include_str!("../../skills/tour/SKILL.md");

/// Every command the skill names in backticks, such as `guided-review check <pr-url>`.
fn commands_in_skill() -> Vec<&'static str> {
    SKILL
        .split('`')
        .skip(1)
        .step_by(2)
        .filter(|span| {
            ["guided-review ", "gh ", "mkdir ", "git ", "cat ", "ls "]
                .iter()
                .any(|program| span.starts_with(program))
        })
        .collect()
}

fn allowed_command_prefixes() -> Vec<String> {
    tour_agent_tools()
        .split(',')
        .filter_map(|tool| tool.strip_prefix("Bash(")?.strip_suffix(":*)"))
        .map(str::to_owned)
        .collect()
}

#[test]
fn a_tour_agent_may_run_every_command_the_tour_skill_asks_for() {
    let allowed = allowed_command_prefixes();
    let commands = commands_in_skill();
    assert!(commands.len() > 10, "{commands:?}");

    let denied: Vec<&str> = commands
        .into_iter()
        .filter(|command| {
            !allowed
                .iter()
                .any(|prefix| command.starts_with(prefix.as_str()))
        })
        .collect();

    assert_eq!(denied, Vec::<&str>::new(), "allowed: {allowed:?}");
}
