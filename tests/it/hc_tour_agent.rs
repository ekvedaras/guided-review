use std::ops::ControlFlow;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use guided_review::hc_tour_agent::HcTourAgents;
use guided_review::pull_request::PullRequestUrl;
use guided_review::viewer::{
    AgentStatus, AppAgent, LiveTerminal, SendOutcome, TerminalId, TourAgentPort, TourAgentStart,
};
use tempfile::TempDir;

const PR: &str = "https://github.com/acme/shop/pull/7";

fn pr() -> PullRequestUrl {
    PR.parse().unwrap()
}

/// A stand-in `hc` that logs each call on one line and answers from `<dir>/answer-<subcommand>`,
/// failing when the server is made to be down.
fn fake_hc(dir: &Path) -> PathBuf {
    let script = dir.join("hc");
    std::fs::write(
        &script,
        format!(
            r#"#!/bin/sh
echo "$@" >> "{dir}/calls"
if [ "$1" = "server" ] && [ -f "{dir}/server-down" ]; then
  echo "no server" >&2
  exit 1
fi
if [ "$1" = "send" ] && [ "$4" = "idle" ] && [ -f "{dir}/refuse-idle" ]; then
  cat "{dir}/refuse-idle" >&2
  exit 10
fi
if [ -f "{dir}/answer-$1" ]; then cat "{dir}/answer-$1"; fi
"#,
            dir = dir.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    script
}

/// A stand-in `herdr-app` that logs each call; `<dir>/no-window` makes it find no window, and
/// `<dir>/not-taken-up` makes its next call find no such space, as before the window takes a terminal up.
fn fake_herdr_app(dir: &Path) -> PathBuf {
    let script = dir.join("herdr-app");
    std::fs::write(
        &script,
        format!(
            r#"#!/bin/sh
echo "$@" >> "{dir}/app-calls"
if [ -f "{dir}/no-window" ]; then
  echo "herdr-app: no Herdr window at {dir}/herdr-app.sock" >&2
  exit 2
fi
if [ -f "{dir}/not-taken-up" ]; then
  rm "{dir}/not-taken-up"
  echo '{{"error": "no space \"guided-review\""}}'
  exit 1
fi
"#,
            dir = dir.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    script
}

fn adapter(dir: &Path) -> HcTourAgents {
    HcTourAgents::with_binaries(fake_hc(dir), fake_herdr_app(dir))
}

fn answer(dir: &Path, subcommand: &str, body: &str) {
    std::fs::write(dir.join(format!("answer-{subcommand}")), body).unwrap();
}

fn calls(dir: &Path, file: &str) -> Vec<String> {
    std::fs::read_to_string(dir.join(file))
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect()
}

#[test]
fn the_agent_of_a_pr_is_looked_up_by_its_label_and_the_app_label() {
    let dir = TempDir::new().unwrap();
    answer(
        dir.path(),
        "ls",
        r#"{"hidden_finished":1,"terminals":[{"address":"t7","state":"live","status":"idle"},{"address":"t9","state":"live"}]}"#,
    );

    let terminals = adapter(dir.path()).terminals_of(&pr()).unwrap();

    assert_eq!(terminals, [TerminalId::new("t7"), TerminalId::new("t9")]);
    assert_eq!(
        calls(dir.path(), "calls"),
        [format!(
            "ls --label app=guided-review --label gr.pr={PR} --json"
        )]
    );
}

#[test]
fn a_terminal_is_claimed_with_both_labels_and_released_by_emptying_the_pr_label() {
    let dir = TempDir::new().unwrap();
    let agents = adapter(dir.path());

    agents.claim(&TerminalId::new("t7"), &pr()).unwrap();
    agents.release(&TerminalId::new("t9")).unwrap();

    assert_eq!(
        calls(dir.path(), "calls"),
        [
            format!("label t7 app=guided-review gr.pr={PR}"),
            "label t9 gr.pr=".to_owned()
        ]
    );
}

#[test]
fn the_status_is_read_from_the_terminal_the_way_herdr_reports_it() {
    let dir = TempDir::new().unwrap();
    let agents = adapter(dir.path());
    let t7 = TerminalId::new("t7");
    let on = |state: &str| {
        format!(r#"{{"address":"t7","status":{{"app":"claude-code","state":"{state}"}}}}"#)
    };

    let mut statuses = Vec::new();
    for state in ["idle", "working", "blocked", "done", "error"] {
        answer(dir.path(), "get", &on(state));
        statuses.push(agents.status(&t7).unwrap());
    }
    answer(dir.path(), "get", r#"{"address":"t7","status":null}"#);
    statuses.push(agents.status(&t7).unwrap());

    assert_eq!(
        statuses,
        [
            AgentStatus::Idle,
            AgentStatus::Working,
            AgentStatus::WaitingForYou,
            AgentStatus::Idle,
            AgentStatus::Failed,
            AgentStatus::Unreported
        ]
    );
    assert_eq!(calls(dir.path(), "calls")[0], "get t7 --json");
}

#[test]
fn status_changes_follow_the_events_of_the_terminal_and_skip_repeats() {
    let dir = TempDir::new().unwrap();
    answer(
        dir.path(),
        "events",
        concat!(
            r#"{"seq":1,"terminals":[{"address":"t3","status":{"state":"working"}},{"address":"t7","status":{"state":"idle"}}]}"#,
            "\n",
            "not json\n",
            r#"{"seq":2,"terminals":[{"address":"t7","status":{"state":"idle"}}]}"#,
            "\n",
            r#"{"seq":3,"terminals":[{"address":"t7","status":{"state":"working"}}]}"#,
            "\n",
            r#"{"seq":4,"terminals":[{"address":"t7","status":{"state":"blocked"}}]}"#,
            "\n",
        ),
    );
    let mut seen = Vec::new();

    adapter(dir.path())
        .follow(&TerminalId::new("t7"), &mut |status| {
            seen.push(status);
            ControlFlow::Continue(())
        })
        .unwrap();

    assert_eq!(
        seen,
        [
            AgentStatus::Idle,
            AgentStatus::Working,
            AgentStatus::WaitingForYou
        ]
    );
    assert_eq!(calls(dir.path(), "calls"), ["events --json --terminal t7"]);
}

#[test]
fn status_changes_are_read_from_the_event_lines_herdr_really_writes() {
    let dir = TempDir::new().unwrap();
    answer(
        dir.path(),
        "events",
        include_str!("../fixtures/hc/events-claude-turn.jsonl"),
    );
    let mut seen = Vec::new();

    adapter(dir.path())
        .follow(&TerminalId::new("t2b"), &mut |status| {
            seen.push(status);
            ControlFlow::Continue(())
        })
        .unwrap();

    assert_eq!(
        seen,
        [AgentStatus::Idle, AgentStatus::Working, AgentStatus::Idle]
    );
}

#[test]
fn the_link_is_on_while_the_server_answers_and_off_with_its_reason_when_not() {
    let dir = TempDir::new().unwrap();
    let agents = adapter(dir.path());

    assert_eq!(agents.link(), Ok(()));
    assert_eq!(calls(dir.path(), "calls"), ["server status --json"]);

    std::fs::write(dir.path().join("server-down"), "").unwrap();
    let reason = agents.link().unwrap_err();
    assert!(reason.contains("does not answer"), "{reason}");
    assert!(reason.contains("no server"), "{reason}");
}

#[test]
fn the_link_is_off_when_hc_is_not_installed() {
    let dir = TempDir::new().unwrap();
    let agents = HcTourAgents::with_binaries(dir.path().join("no-hc"), dir.path().join("no-app"));

    let reason = agents.link().unwrap_err();

    assert!(reason.contains("not on PATH"), "{reason}");
}

#[test]
fn showing_an_agent_the_reviewer_started_focuses_its_terminal_and_never_moves_it() {
    let dir = TempDir::new().unwrap();
    answer(
        dir.path(),
        "get",
        &format!(r#"{{"address":"t7","labels":{{"app":"guided-review","gr.pr":"{PR}"}}}}"#),
    );

    adapter(dir.path()).show(&TerminalId::new("t7")).unwrap();

    assert_eq!(calls(dir.path(), "calls"), ["get t7 --json"]);
    assert_eq!(
        calls(dir.path(), "app-calls"),
        ["do agent.focus --terminal t7"]
    );
}

#[test]
fn showing_an_agent_the_app_started_without_a_place_puts_it_in_the_guided_review_space_first() {
    let dir = TempDir::new().unwrap();
    answer(
        dir.path(),
        "get",
        &format!(
            r#"{{"address":"t9","name":"tour-acme-shop-7","labels":{{"app":"guided-review","gr.pr":"{PR}","gr.started-by":"app"}}}}"#
        ),
    );

    adapter(dir.path()).show(&TerminalId::new("t9")).unwrap();

    assert_eq!(
        calls(dir.path(), "calls"),
        [
            "get t9 --json",
            "label t9 herdr.space=guided-review herdr.tab=tour-acme-shop-7"
        ]
    );
    assert_eq!(
        calls(dir.path(), "app-calls"),
        [
            "do space.rename --id guided-review --name guided-review agents",
            "do tab.rename --id tour-acme-shop-7 --name acme/shop#7",
            "do agent.focus --terminal t9"
        ]
    );
}

#[test]
fn showing_an_agent_placed_while_the_herdr_app_was_closed_names_its_space_and_tab() {
    let dir = TempDir::new().unwrap();
    answer(
        dir.path(),
        "get",
        &format!(
            r#"{{"address":"t9","name":"tour-acme-shop-7","labels":{{"gr.pr":"{PR}","gr.started-by":"app","herdr.space":"guided-review","herdr.tab":"tour-acme-shop-7"}}}}"#
        ),
    );

    adapter(dir.path()).show(&TerminalId::new("t9")).unwrap();

    assert_eq!(calls(dir.path(), "calls"), ["get t9 --json"]);
    assert_eq!(
        calls(dir.path(), "app-calls"),
        [
            "do space.rename --id guided-review --name guided-review agents",
            "do tab.rename --id tour-acme-shop-7 --name acme/shop#7",
            "do agent.focus --terminal t9"
        ]
    );
}

fn live(terminal: &str, runs_claude: bool, space: &str) -> LiveTerminal {
    LiveTerminal {
        terminal: TerminalId::new(terminal),
        runs_claude,
        space: space.to_owned(),
    }
}

#[test]
fn live_terminals_say_whether_they_run_claude_and_in_which_space_as_herdr_lists_them() {
    let dir = TempDir::new().unwrap();
    answer(dir.path(), "ls", include_str!("../fixtures/herdr/ls.json"));

    let terminals = adapter(dir.path()).live_terminals().unwrap();

    assert_eq!(
        terminals,
        [
            live("t1", true, "dotfiles"),
            live("t3", false, "shop"),
            live("t4", false, "shop"),
            live("t1b", true, "guided-review"),
            live("t2h", false, "guided-review"),
            live("t2j", true, "guided-review"),
        ]
    );
    assert_eq!(calls(dir.path(), "calls"), ["ls --json"]);
}

#[test]
fn text_is_sent_only_if_the_agent_is_idle() {
    let dir = TempDir::new().unwrap();
    answer(
        dir.path(),
        "send",
        include_str!("../fixtures/hc/send-started.json"),
    );

    let sent = adapter(dir.path())
        .send_if_idle(&TerminalId::new("t7"), "a question")
        .unwrap();

    assert_eq!(sent, SendOutcome::Delivered);
    assert_eq!(calls(dir.path(), "calls"), ["send t7 --if idle a question"]);
}

#[test]
fn a_working_agent_refuses_the_text_with_herdrs_reason() {
    let dir = TempDir::new().unwrap();
    std::fs::write(
        dir.path().join("refuse-idle"),
        include_str!("../fixtures/hc/send-refused-working.json"),
    )
    .unwrap();

    let sent = adapter(dir.path())
        .send_if_idle(&TerminalId::new("t2b"), "a question")
        .unwrap();

    assert_eq!(
        sent,
        SendOutcome::Refused("t2b is working, not idle".to_owned())
    );
}

#[test]
fn an_agent_whose_screen_has_not_settled_since_it_started_is_still_starting_and_takes_nothing() {
    let dir = TempDir::new().unwrap();
    std::fs::write(
        dir.path().join("refuse-idle"),
        include_str!("../fixtures/hc/send-refused-starting.json"),
    )
    .unwrap();

    let sent = adapter(dir.path())
        .send_if_idle(&TerminalId::new("t2b"), "a question")
        .unwrap();

    assert_eq!(sent, SendOutcome::Starting);
    assert_eq!(
        calls(dir.path(), "calls"),
        ["send t2b --if idle a question"]
    );
}

#[test]
fn a_send_herdr_refuses_for_another_reason_fails_with_herdrs_words_not_its_json() {
    let dir = TempDir::new().unwrap();
    std::fs::write(
        dir.path().join("refuse-idle"),
        r#"{"error":{"hint":"see hc ls","kind":"not_found","message":"no terminal t2b"}}"#,
    )
    .unwrap();

    let error = adapter(dir.path())
        .send_if_idle(&TerminalId::new("t2b"), "a question")
        .unwrap_err();

    assert_eq!(
        format!("{error:#}"),
        "herdr did not type it into t2b: no terminal t2b"
    );
}

#[test]
fn an_agent_that_finished_its_turn_unseen_takes_the_text_though_herdr_calls_it_done_not_idle() {
    let dir = TempDir::new().unwrap();
    std::fs::write(
        dir.path().join("refuse-idle"),
        include_str!("../fixtures/hc/send-refused-done.json"),
    )
    .unwrap();

    let sent = adapter(dir.path())
        .send_if_idle(&TerminalId::new("t2b"), "a question")
        .unwrap();

    assert_eq!(sent, SendOutcome::Delivered);
    assert_eq!(
        calls(dir.path(), "calls"),
        ["send t2b --if idle a question", "send t2b a question"]
    );
}

#[test]
fn a_tour_agent_is_started_in_the_background_outliving_the_app_with_its_labels_tools_and_the_apps_path()
 {
    let dir = TempDir::new().unwrap();
    answer(
        dir.path(),
        "run",
        r#"{"address":"t9","name":"tour-acme-shop-7"}"#,
    );

    let started = adapter(dir.path())
        .giving_agents_path("/opt/gr/bin:/usr/bin")
        .start(&TourAgentStart {
            pr: pr(),
            name: "tour-acme-shop-7".to_owned(),
            folder: PathBuf::from("/clones/shop"),
            first_task: "You are the tour agent".to_owned(),
            summary_skill: None,
        })
        .unwrap();

    assert_eq!(started, TerminalId::new("t9"));
    let calls = calls(dir.path(), "calls");
    let [call] = calls.as_slice() else {
        panic!("{calls:?}");
    };
    let (before_tools, tools) = call.split_once(" --allowedTools ").unwrap();
    assert_eq!(
        before_tools,
        format!(
            "run --bg --json --outlive-parent --no-parent --name tour-acme-shop-7 --cwd /clones/shop \
             --label app=guided-review --label gr.pr={PR} --label gr.started-by=app \
             --label herdr.space=guided-review --label herdr.tab=tour-acme-shop-7 \
             -- env PATH=/opt/gr/bin:/usr/bin claude You are the tour agent --permission-mode auto"
        )
    );
    let tools: Vec<&str> = tools.split(',').collect();
    for needed in [
        "Bash(guided-review open:*)",
        "Bash(guided-review agent-reply:*)",
        "Bash(guided-review check:*)",
        "Skill(tour)",
    ] {
        assert!(tools.contains(&needed), "{needed} missing from {tools:?}");
    }
}

fn start_shop_agent(agents: &HcTourAgents) -> anyhow::Result<TerminalId> {
    agents.start(&TourAgentStart {
        pr: pr(),
        name: "tour-acme-shop-7".to_owned(),
        folder: PathBuf::from("/clones/shop"),
        first_task: "You are the tour agent".to_owned(),
        summary_skill: None,
    })
}

fn tools_of_started_agent(summary_skill: Option<&str>) -> Vec<String> {
    let dir = TempDir::new().unwrap();
    answer(dir.path(), "run", r#"{"address":"t9"}"#);
    adapter(dir.path())
        .start(&TourAgentStart {
            pr: pr(),
            name: "tour-acme-shop-7".to_owned(),
            folder: PathBuf::from("/clones/shop"),
            first_task: "You are the tour agent".to_owned(),
            summary_skill: summary_skill.map(str::to_owned),
        })
        .unwrap();
    let calls = calls(dir.path(), "calls");
    let [call] = calls.as_slice() else {
        panic!("{calls:?}");
    };
    let (_, tools) = call.split_once(" --allowedTools ").unwrap();
    tools.split(',').map(str::to_owned).collect()
}

#[test]
fn a_tour_agent_may_run_the_summary_skill_the_reviewer_named_and_only_then() {
    let with = tools_of_started_agent(Some("recap"));
    let without = tools_of_started_agent(None);

    assert!(with.contains(&"Skill(recap)".to_owned()), "{with:?}");
    assert!(
        !without.iter().any(|tool| tool == "Skill(recap)"),
        "{without:?}"
    );
    assert_eq!(with.len(), without.len() + 1);
}

#[test]
fn a_started_tour_agent_gets_the_guided_review_space_and_a_tab_named_after_its_pr_in_the_herdr_app()
{
    let dir = TempDir::new().unwrap();
    answer(dir.path(), "run", r#"{"address":"t9"}"#);

    start_shop_agent(&adapter(dir.path())).unwrap();

    assert_eq!(
        calls(dir.path(), "app-calls"),
        [
            "do space.rename --id guided-review --name guided-review agents",
            "do tab.rename --id tour-acme-shop-7 --name acme/shop#7"
        ]
    );
}

#[test]
fn a_tour_agent_starts_placed_with_the_herdr_app_closed_and_naming_is_not_tried_again() {
    let dir = TempDir::new().unwrap();
    answer(dir.path(), "run", r#"{"address":"t9"}"#);
    std::fs::write(dir.path().join("no-window"), "").unwrap();

    let started = start_shop_agent(&adapter(dir.path())).unwrap();

    assert_eq!(started, TerminalId::new("t9"));
    assert_eq!(
        calls(dir.path(), "app-calls"),
        ["do space.rename --id guided-review --name guided-review agents"]
    );
}

#[test]
fn naming_waits_for_the_herdr_app_to_take_the_new_terminal_up() {
    let dir = TempDir::new().unwrap();
    answer(dir.path(), "run", r#"{"address":"t9"}"#);
    std::fs::write(dir.path().join("not-taken-up"), "").unwrap();

    start_shop_agent(&adapter(dir.path())).unwrap();

    assert_eq!(
        calls(dir.path(), "app-calls"),
        [
            "do space.rename --id guided-review --name guided-review agents",
            "do space.rename --id guided-review --name guided-review agents",
            "do tab.rename --id tour-acme-shop-7 --name acme/shop#7"
        ]
    );
}

#[test]
fn agents_get_a_path_that_finds_the_running_guided_review_before_the_rest_of_the_apps_path() {
    assert_eq!(
        HcTourAgents::path_for_agents(
            Path::new("/Users/me/.cargo/bin/guided-review"),
            Some("/opt/homebrew/bin:/usr/bin".as_ref())
        ),
        "/Users/me/.cargo/bin:/opt/homebrew/bin:/usr/bin"
    );
    assert_eq!(
        HcTourAgents::path_for_agents(
            Path::new("/Users/me/.cargo/bin/guided-review"),
            Some("/usr/bin:/Users/me/.cargo/bin".as_ref())
        ),
        "/usr/bin:/Users/me/.cargo/bin",
        "already on it"
    );
    assert_eq!(
        HcTourAgents::path_for_agents(Path::new("/Users/me/.cargo/bin/guided-review"), None),
        "/Users/me/.cargo/bin"
    );
}

#[test]
fn the_last_screen_lines_are_read_with_their_history_and_blank_lines_dropped() {
    let dir = TempDir::new().unwrap();
    let recorded = include_str!("../fixtures/hc/read-lines.json");
    answer(
        dir.path(),
        "read",
        &recorded.replace(r#"╰─""#, r#"╰─\n\n\n""#),
    );

    let lines = adapter(dir.path())
        .screen_lines(&TerminalId::new("t9"), 3)
        .unwrap();

    assert_eq!(lines.len(), 3);
    assert_eq!(lines[0], "hello-probe");
    assert!(lines[1].starts_with("╭─"), "{lines:?}");
    assert_eq!(lines[2], "╰─");
    assert_eq!(calls(dir.path(), "calls"), ["read t9 --json --lines 3"]);
}

#[test]
fn the_last_lines_of_a_claude_terminal_are_its_work_above_the_prompt_box_without_blank_lines() {
    let dir = TempDir::new().unwrap();
    answer(
        dir.path(),
        "read",
        include_str!("../fixtures/hc/read-claude-after-turn.json"),
    );

    let lines = adapter(dir.path())
        .screen_lines(&TerminalId::new("t2r"), 3)
        .unwrap();

    assert_eq!(
        lines,
        [
            "  1. Find out where guided-review is installed. Then add that folder to the PATH that this session gets. (PATH is the list of folders the shell looks in for commands.)",
            "  2. Start the tour again from the app.",
            "✻ Cooked for 9s · done 11:19 AM",
        ]
    );
}

#[test]
fn a_tour_agent_is_ended_with_hc_kill() {
    let dir = TempDir::new().unwrap();

    adapter(dir.path()).kill(&TerminalId::new("t9")).unwrap();

    assert_eq!(calls(dir.path(), "calls"), ["kill t9"]);
}

#[test]
fn the_app_started_agents_are_listed_with_their_pr_status_and_when_it_changed() {
    let dir = TempDir::new().unwrap();
    answer(
        dir.path(),
        "ls",
        r#"{"terminals":[{"address":"t9","state":"live"}]}"#,
    );
    answer(
        dir.path(),
        "get",
        &format!(
            r#"{{"address":"t9","labels":{{"gr.pr":"{PR}","gr.started-by":"app"}},"status":{{"app":"claude-code","state":"idle"}},"status_ms":1793954000972}}"#
        ),
    );

    let agents = adapter(dir.path()).app_started().unwrap();

    assert_eq!(
        agents,
        [AppAgent {
            terminal: TerminalId::new("t9"),
            pr: Some(pr()),
            status: AgentStatus::Idle,
            status_since_ms: 1_793_954_000_972,
        }]
    );
    assert_eq!(
        calls(dir.path(), "calls"),
        [
            "ls --label app=guided-review --label gr.started-by=app --json",
            "get t9 --json"
        ]
    );
}
