#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "a panic in a test helper is the test failing"
)]

#[allow(dead_code)]
mod support;

use std::path::PathBuf;
use std::sync::Arc;

use guided_review::config::AskSettings;
use guided_review::local_git::LocalGit;
use guided_review::pull_request::{RepoName, Sha};
use guided_review::tour::Tour;
use guided_review::viewer::{
    AgentLauncher, AgentPort, Asks, Author, LineLocation, Place, Review, ReviewStateFile, Side,
    StepId,
};
use serde_json::json;
use support::FixtureRepo;
use support::fake_ports::{FakeAgentLauncher, FakeAskStore, SentLog};

const PONG: &str = include_str!("fixtures/ask/pong.jsonl");
const READS_A_FILE: &str = include_str!("fixtures/ask/reads_a_file.jsonl");
const FAILED: &str = include_str!("fixtures/ask/failed.jsonl");
const SESSION: &str = "7f2cd173-4eda-4770-aeb2-c82977b1b673";

struct Shop {
    review: Review,
    head: Sha,
}

fn shop() -> Shop {
    let repo = FixtureRepo::cloned_from("acme/shop");
    repo.write(
        "app/Order.php",
        "<?php\n\nclass Order\n{\n    public function total(): int\n    {\n        return 1;\n    }\n}\n",
    );
    let base = repo.commit("base");
    repo.write(
        "app/Order.php",
        "<?php\n\nclass Order\n{\n    public function total(): int\n    {\n        return 2;\n    }\n}\n",
    );
    let head = repo.commit("head");
    let tour: Tour = serde_json::from_value(json!({
        "pr": "https://github.com/acme/shop/pull/7",
        "base": base,
        "head": head,
        "title": "Orders cost two",
        "summary": ["Order total goes up"],
        "steps": [{
            "id": "order", "title": "Total changes", "why": "pricing moved to two",
            "pieces": [{"file": "app/Order.php", "symbol": "Order::total"}]
        }],
    }))
    .unwrap();
    let source = LocalGit::find(&RepoName::new("acme", "shop"), repo.path())
        .unwrap()
        .unwrap();
    let state_file = ReviewStateFile::for_pr(
        repo.home(),
        &"https://github.com/acme/shop/pull/7".parse().unwrap(),
    );
    Shop {
        review: Review::open(tour, &source, state_file).unwrap(),
        head: Sha::new(&head),
    }
}

fn order_step() -> Place {
    Place::Step {
        step: StepId::from("order"),
    }
}

fn start(launcher: &FakeAgentLauncher, resume: Option<&str>) -> Box<dyn AgentPort> {
    let launch = shop().review.agent_launch(
        PathBuf::from("/clones/shop"),
        &AskSettings::default(),
        resume.map(str::to_string),
    );
    launcher.start(&launch).unwrap()
}

#[test]
fn asking_about_a_step_sends_pointers_step_text_and_diff_lines_and_streams_the_answer() {
    let Shop { review, head } = shop();
    let sent = SentLog::default();
    let store = FakeAskStore::default();
    let launcher = FakeAgentLauncher {
        sent: sent.clone(),
        turns: vec![PONG.to_string()],
    };
    let mut agent = start(&launcher, None);
    let mut asks = Asks::load(Arc::new(store.clone())).unwrap();

    let id = asks.start(order_step(), &head);
    let context = review.ask_context(&order_step()).unwrap();
    asks.ask(id, "Why two?", &context, agent.as_mut()).unwrap();

    let message = sent.agent_lines().pop().unwrap();
    for expected in [
        "https://github.com/acme/shop/pull/7",
        head.as_str(),
        "Total changes",
        "pricing moved to two",
        "app/Order.php",
        "- ",
        "return 1;",
        "+ ",
        "return 2;",
        "Why two?",
    ] {
        assert!(
            message.contains(expected),
            "{expected:?} missing in: {message}"
        );
    }
    assert!(
        !message.contains("class Order"),
        "whole files must not be pasted"
    );

    let ask = asks.get(id).unwrap();
    assert_eq!(ask.messages.len(), 2);
    assert_eq!(ask.messages[0].author, Author::Reviewer);
    assert_eq!(ask.messages[0].text, "Why two?");
    assert_eq!(ask.messages[1].author, Author::Ai);
    assert_eq!(ask.messages[1].text, "pong");
    assert_eq!(ask.session_id.as_deref(), Some(SESSION));
    assert_eq!(ask.head, head);
}

#[test]
fn a_follow_up_sends_only_the_question() {
    let Shop { review, head } = shop();
    let sent = SentLog::default();
    let launcher = FakeAgentLauncher {
        sent: sent.clone(),
        turns: vec![PONG.to_string(), PONG.to_string()],
    };
    let mut agent = start(&launcher, None);
    let mut asks = Asks::load(Arc::new(FakeAskStore::default())).unwrap();
    let id = asks.start(order_step(), &head);
    let context = review.ask_context(&order_step()).unwrap();

    asks.ask(id, "Why two?", &context, agent.as_mut()).unwrap();
    asks.ask(id, "And in tests?", &context, agent.as_mut())
        .unwrap();

    assert_eq!(sent.agent_lines().pop().unwrap(), "agent: And in tests?");
    assert_eq!(asks.get(id).unwrap().messages.len(), 4);
}

#[test]
fn asks_are_kept_in_the_store_newest_first_per_place_and_come_back_on_load() {
    let Shop { review, head } = shop();
    let sent = SentLog::default();
    let store = FakeAskStore::default();
    let launcher = FakeAgentLauncher {
        sent: sent.clone(),
        turns: vec![PONG.to_string(), PONG.to_string()],
    };
    let mut agent = start(&launcher, None);
    let mut asks = Asks::load(Arc::new(store.clone())).unwrap();
    let context = review.ask_context(&order_step()).unwrap();
    let first = asks.start(order_step(), &head);
    asks.ask(first, "first?", &context, agent.as_mut()).unwrap();
    let second = asks.start(order_step(), &head);
    asks.ask(second, "second?", &context, agent.as_mut())
        .unwrap();
    let unanswered_draft = asks.start(Place::Tour, &head);

    let reloaded = Asks::load(Arc::new(store.clone())).unwrap();

    let at_step: Vec<&str> = reloaded
        .at(&order_step())
        .iter()
        .map(|ask| ask.messages[0].text.as_str())
        .collect();
    assert_eq!(at_step, ["second?", "first?"]);
    assert_eq!(reloaded.count_at(&order_step()), 2);
    assert_eq!(
        reloaded.count_at(&Place::Tour),
        0,
        "a draft with no question is not kept"
    );
    assert!(reloaded.get(unanswered_draft).is_none());
    assert_eq!(
        reloaded.get(first).unwrap().session_id.as_deref(),
        Some(SESSION)
    );
}

#[test]
fn a_tool_use_shows_as_activity_and_the_answer_continues_in_a_new_paragraph() {
    let Shop { review, head } = shop();
    let sent = SentLog::default();
    let launcher = FakeAgentLauncher {
        sent: sent.clone(),
        turns: vec![READS_A_FILE.to_string()],
    };
    let mut agent = start(&launcher, None);
    let mut asks = Asks::load(Arc::new(FakeAskStore::default())).unwrap();
    let id = asks.start(order_step(), &head);
    let context = review.ask_context(&order_step()).unwrap();
    let mut activity_seen = Vec::new();

    let message = asks.begin_turn(id, "Why?", &context);
    agent
        .ask(&message, &mut |line| {
            asks.apply_line(id, line);
            activity_seen.extend(asks.get(id).unwrap().activity.clone());
        })
        .unwrap();

    assert_eq!(activity_seen, ["Read app/Order.php"]);
    let ask = asks.get(id).unwrap();
    assert_eq!(ask.messages[1].text, "Let me look.\n\nIt returns two.");
    assert_eq!(ask.activity, None);
}

#[test]
fn an_error_result_is_kept_as_the_ask_failure() {
    let Shop { review, head } = shop();
    let launcher = FakeAgentLauncher {
        sent: SentLog::default(),
        turns: vec![FAILED.to_string()],
    };
    let mut agent = start(&launcher, None);
    let mut asks = Asks::load(Arc::new(FakeAskStore::default())).unwrap();
    let id = asks.start(order_step(), &head);
    let context = review.ask_context(&order_step()).unwrap();

    asks.ask(id, "Why?", &context, agent.as_mut()).unwrap();

    assert_eq!(
        asks.get(id).unwrap().failure.as_deref(),
        Some("Credit balance is too low")
    );
}

#[test]
fn a_piece_and_the_whole_tour_have_their_own_context() {
    let Shop { review, .. } = shop();

    let piece = review
        .ask_context(&Place::Piece {
            step: StepId::from("order"),
            piece: "app/Order.php#Order::total".to_string(),
        })
        .unwrap();
    let tour = review.ask_context(&Place::Tour).unwrap();

    assert!(piece.contains("return 2;") && piece.contains("pricing moved to two"));
    assert!(tour.contains("Orders cost two") && tour.contains("Order total goes up"));
    assert!(tour.contains("Total changes") && !tour.contains("return 2;"));
}

#[test]
fn the_agent_is_started_in_the_clone_with_default_model_and_effort_and_can_resume() {
    let sent = SentLog::default();
    let launcher = FakeAgentLauncher {
        sent: sent.clone(),
        turns: vec![],
    };

    start(&launcher, None);
    start(&launcher, Some(SESSION));

    assert_eq!(
        sent.agent_lines(),
        [
            "agent started in /clones/shop with opus / medium".to_string(),
            format!("agent started in /clones/shop with opus / medium, resuming {SESSION}"),
        ]
    );
}

#[test]
fn the_launch_tells_the_agent_it_is_read_only_and_how_to_read_more_code() {
    let launch =
        shop()
            .review
            .agent_launch(PathBuf::from("/clones/shop"), &AskSettings::default(), None);

    assert!(
        launch
            .system_prompt
            .contains("guided-review show https://github.com/acme/shop/pull/7")
    );
    assert!(launch.system_prompt.contains("--around"));
}

#[test]
fn config_overrides_the_ask_model_and_effort() {
    let settings =
        AskSettings::from_config_text("ask_model = \"sonnet\"\nask_effort = \"high\"\n").unwrap();

    assert_eq!(
        (settings.model.as_str(), settings.effort.as_str()),
        ("sonnet", "high")
    );
}

fn lines_place(start: usize, end: usize) -> Place {
    let at = |line| LineLocation {
        path: "src/a.rs".into(),
        side: Side::Head,
        line,
    };
    Place::Lines {
        step: None,
        start: at(start),
        end: at(end),
        pieces: vec![],
    }
}

#[test]
fn a_lines_place_is_labelled_with_its_file_and_lines() {
    assert_eq!(
        lines_place(4, 4).lines_label().as_deref(),
        Some("src/a.rs line 4")
    );
    assert_eq!(
        lines_place(4, 9).lines_label().as_deref(),
        Some("src/a.rs lines 4 to 9")
    );
    assert_eq!(Place::Tour.lines_label(), None);
}
