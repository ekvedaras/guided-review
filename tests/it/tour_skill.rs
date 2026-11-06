use guided_review::claude_cli::{refresh_writer_tools, tour_agent_tools};

const SKILL: &str = include_str!("../../skills/tour/SKILL.md");

/// Every command the text names in backticks, such as `guided-review check <pr-url>`.
fn commands_in(text: &'static str) -> Vec<&'static str> {
    text.split('`')
        .skip(1)
        .step_by(2)
        .filter(|span| {
            ["guided-review ", "gh ", "mkdir ", "git ", "cat ", "ls "]
                .iter()
                .any(|program| span.starts_with(program))
        })
        .collect()
}

fn refresh_section() -> &'static str {
    SKILL
        .split("\n## ")
        .find(|section| section.starts_with("Refresh\n"))
        .expect("a Refresh section")
}

fn allowed_command_prefixes(tools: &str) -> Vec<String> {
    tools
        .split(',')
        .filter_map(|tool| tool.strip_prefix("Bash(")?.strip_suffix(":*)"))
        .map(str::to_owned)
        .collect()
}

fn denied_by(tools: &str, commands: Vec<&'static str>) -> Vec<&'static str> {
    let allowed = allowed_command_prefixes(tools);
    commands
        .into_iter()
        .filter(|command| {
            !allowed
                .iter()
                .any(|prefix| command.starts_with(prefix.as_str()))
        })
        .collect()
}

#[test]
fn a_tour_agent_may_run_every_command_the_tour_skill_asks_for() {
    let commands = commands_in(SKILL);
    assert!(commands.len() > 10, "{commands:?}");

    let tools = tour_agent_tools();
    assert_eq!(denied_by(&tools, commands), Vec::<&str>::new(), "{tools}");
}

#[test]
fn the_refresh_writer_may_run_every_command_the_refresh_section_asks_for() {
    let commands = commands_in(refresh_section());
    assert!(commands.len() >= 6, "{commands:?}");

    let tools = refresh_writer_tools();
    assert_eq!(denied_by(tools, commands), Vec::<&str>::new(), "{tools}");
}
