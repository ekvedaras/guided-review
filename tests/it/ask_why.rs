//! Ask why: the reviewer asks the review agent about one finding, on a thread of the finding's own.

use std::path::Path;
use std::sync::Arc;

use crate::support::fake_app::{CliEnded, answering_one_request};
use crate::support::fake_checkouts::FakeCheckouts;
use crate::support::fake_ports::{FakeHost, SentLog};
use crate::support::fake_tour_agent::{FakeTourAgent, TRIAL_BUILD, TourAgentCall};
use crate::support::{FakeGitHub, FixtureRepo, GuidedReview, Run, folders_in};
use guided_review::findings::FindingId;
use guided_review::open_review::open_review;
use guided_review::prompt_safety::OUTSIDE_TEXT_IS_DATA;
use guided_review::review_subject::ReviewSubject;
use guided_review::tour::Tour;
use guided_review::viewer::{
    AgentStatus, Audience, FindingRefusal, FindingRow, PageRow, Review, ReviewAgents, ReviewState,
    ReviewStateFile, SendMode, TerminalId, TourAgentPort, WhyAsked, WhyAuthor, WhyMessage,
    WhyReply, WhyState,
};
use serde_json::{Value, json};

const PR: &str = "https://github.com/acme/shop/pull/7";

fn pr() -> ReviewSubject {
    PR.parse().unwrap()
}

/// `README.md` has three changed lines and is the only step.
struct Shop {
    repo: FixtureRepo,
}

impl Shop {
    fn with_findings(findings: &[Value]) -> Self {
        let repo = FixtureRepo::cloned_from("acme/shop");
        repo.write("README.md", "a\nb\nc\nend\n");
        let base = repo.commit("base");
        repo.write("README.md", "A\nB\nC\nend\n");
        let head = repo.commit("head");
        let tour = json!({
            "pr": PR, "base": base, "head": head, "title": "Readme", "summary": [],
            "steps": [{"id": "readme", "title": "Readme", "why": "w", "pieces": [{"file": "README.md"}]}],
        });
        let path = Tour::path_for(&repo.folders(), &pr());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, tour.to_string()).unwrap();
        std::fs::write(
            Tour::findings_path_for(&repo.folders(), &pr()),
            json!({"pr": PR, "base": base, "head": head, "findings": findings}).to_string(),
        )
        .unwrap();
        Self { repo }
    }

    fn open(&self) -> Review {
        let mut review = open_review(&pr(), self.repo.path(), &self.repo.folders())
            .unwrap()
            .review;
        review.go_forward();
        review
    }

    fn agents(&self, fake: &Arc<FakeTourAgent>) -> ReviewAgents {
        ReviewAgents::new(
            Arc::clone(fake) as Arc<dyn TourAgentPort>,
            Arc::new(FakeCheckouts::default()),
            self.repo.folders(),
        )
    }
}

fn on_line(id: &str, line: u32, body: &str) -> Value {
    json!({"id": id, "severity": "should fix", "body": body, "anchor": {
        "kind": "lines", "file": "README.md", "side": "head", "lines": [line, line],
    }})
}

fn ask_why(review: &mut Review, finding: &str, question: &str) -> WhyAsked {
    review.ask_why(&id(finding), question).unwrap()
}

fn id(text: &str) -> FindingId {
    FindingId::try_from(text.to_owned()).unwrap()
}

/// The review agent left idle after its findings, as the app finds it again.
fn idle_review_agent(review: &mut Review) -> Arc<FakeTourAgent> {
    let fake = Arc::new(FakeTourAgent::new().review_agent("r1", PR, AgentStatus::Idle, 0));
    review.ai_review_agent_resumed(TerminalId::new("r1"), Some(AgentStatus::Idle));
    fake
}

#[test]
fn a_question_goes_to_the_idle_review_agent_with_the_finding_and_how_to_answer() {
    let shop = Shop::with_findings(&[on_line("f1", 2, "B is shouted.")]);
    let mut review = shop.open();
    let fake = idle_review_agent(&mut review);

    let asked = ask_why(&mut review, "f1", "Why is that wrong?");
    let delivery = review.why_delivery().unwrap();
    review.why_delivered(delivery.run(&shop.agents(&fake)));

    assert_eq!(asked, WhyAsked::ToReviewAgent);
    let calls = fake.calls();
    let [TourAgentCall::Send { terminal, text }] = calls.as_slice() else {
        panic!("{calls:?}");
    };
    assert_eq!(terminal, &TerminalId::new("r1"));
    for part in [
        "B is shouted.",
        "Why is that wrong?",
        "README.md",
        "guided-review agent-reply https://github.com/acme/shop/pull/7 f1 --body -",
    ] {
        assert!(text.contains(part), "{part} missing from {text}");
    }
    assert_eq!(review.why_state_of(&id("f1")), Some(&WhyState::Delivered));
}

#[test]
fn a_trial_build_names_itself_in_the_command_that_answers_a_question_on_a_finding() {
    let shop = Shop::with_findings(&[on_line("f1", 2, "B is shouted.")]);
    let mut review = shop.open();
    let fake = Arc::new(
        FakeTourAgent::new()
            .review_agent("r1", PR, AgentStatus::Idle, 0)
            .of_a_trial_build(),
    );
    review.ai_review_agent_resumed(TerminalId::new("r1"), Some(AgentStatus::Idle));

    ask_why(&mut review, "f1", "Why is that wrong?");
    let delivery = review.why_delivery().unwrap();
    review.why_delivered(delivery.run(&shop.agents(&fake)));

    let calls = fake.calls();
    let [TourAgentCall::Send { text, .. }] = calls.as_slice() else {
        panic!("{calls:?}");
    };
    assert!(
        text.contains(&format!("`{TRIAL_BUILD} agent-reply {PR} f1 --body -`")),
        "{text}"
    );
    assert!(!text.contains("`guided-review "), "{text}");
}

#[test]
fn a_delivered_question_is_worked_on_then_ends_without_an_answer_when_the_agent_stops_silent() {
    let shop = Shop::with_findings(&[on_line("f1", 2, "B is shouted.")]);
    let mut review = shop.open();
    let fake = idle_review_agent(&mut review);
    ask_why(&mut review, "f1", "");
    let delivery = review.why_delivery().unwrap();
    review.why_delivered(delivery.run(&shop.agents(&fake)));

    review.ai_review_agent_status(AgentStatus::Working);
    assert_eq!(review.why_state_of(&id("f1")), Some(&WhyState::Working));

    review.ai_review_agent_status(AgentStatus::Idle);
    assert_eq!(
        review.why_state_of(&id("f1")),
        Some(&WhyState::EndedWithoutAnswer)
    );
}

#[test]
fn a_second_question_waits_for_the_answer_to_the_first() {
    let shop = Shop::with_findings(&[on_line("f1", 2, "B is shouted.")]);
    let mut review = shop.open();
    idle_review_agent(&mut review);
    ask_why(&mut review, "f1", "Why?");

    let again = review.ask_why(&id("f1"), "And?");

    assert_eq!(again, Err(FindingRefusal::BeingAsked));
}

#[test]
fn the_sidebar_offers_show_agent_while_the_idle_review_agent_lives_after_its_findings() {
    let shop = Shop::with_findings(&[on_line("f1", 2, "B is shouted.")]);
    let mut review = shop.open();
    assert!(!review.ai_review_stage().can_show_agent);

    idle_review_agent(&mut review);
    assert!(review.ai_review_stage().can_show_agent);

    review.ai_review_agent_gone();
    assert!(!review.ai_review_stage().can_show_agent);
}

fn finding_row(review: &Review, finding: &str) -> Arc<FindingRow> {
    review
        .page()
        .rows()
        .iter()
        .find_map(|row| match row {
            PageRow::Finding(row) if row.id == id(finding) => Some(Arc::clone(row)),
            _ => None,
        })
        .unwrap()
}

#[test]
fn the_finding_shows_its_thread_and_where_the_question_is() {
    let shop = Shop::with_findings(&[on_line("f1", 2, "B is shouted.")]);
    let mut review = shop.open();
    let fake = idle_review_agent(&mut review);
    assert!(finding_row(&review, "f1").can_ask_why);

    ask_why(&mut review, "f1", "Why?");
    let delivery = review.why_delivery().unwrap();
    review.why_delivered(delivery.run(&shop.agents(&fake)));
    let asked = finding_row(&review, "f1");
    assert!(!asked.can_ask_why);
    assert_eq!(
        asked.asked.as_deref(),
        Some("Asked: sent to the review agent")
    );

    review
        .why_replied(
            &id("f1"),
            &WhyReply::Answer("Because it shouts.".to_owned()),
        )
        .unwrap();

    let answered = finding_row(&review, "f1");
    assert!(answered.can_ask_why);
    assert_eq!(answered.asked.as_deref(), Some("The review agent answered"));
    let said: Vec<(WhyAuthor, &str)> = answered
        .thread
        .iter()
        .map(|message| (message.author, message.body.as_str()))
        .collect();
    assert_eq!(
        said,
        [
            (WhyAuthor::Reviewer, "Why?"),
            (WhyAuthor::ReviewAgent, "Because it shouts.")
        ]
    );
}

#[test]
fn a_follow_up_after_the_answer_goes_to_the_review_agent_with_the_talk_so_far() {
    let shop = Shop::with_findings(&[on_line("f1", 2, "B is shouted.")]);
    let mut review = shop.open();
    let fake = idle_review_agent(&mut review);
    ask_why(&mut review, "f1", "Why?");
    let delivery = review.why_delivery().unwrap();
    review.why_delivered(delivery.run(&shop.agents(&fake)));
    review
        .why_replied(
            &id("f1"),
            &WhyReply::Answer("Because it shouts.".to_owned()),
        )
        .unwrap();

    let idle_again = Arc::new(FakeTourAgent::new().review_agent("r1", PR, AgentStatus::Idle, 0));

    ask_why(&mut review, "f1", "Is it on purpose?");
    let delivery = review.why_delivery().unwrap();
    review.why_delivered(delivery.run(&shop.agents(&idle_again)));

    let calls = idle_again.calls();
    let Some(TourAgentCall::Send { text, .. }) = calls.last() else {
        panic!("{calls:?}");
    };
    for part in [
        "\nReviewer said before: Why?",
        "\nYou said before:\n> Because it shouts.",
        "\nQuestion: Is it on purpose?",
        "\nFinding:\n> B is shouted.",
    ] {
        assert!(text.contains(part), "{part} missing from {text}");
    }
    assert_eq!(text.matches(OUTSIDE_TEXT_IS_DATA).count(), 1, "{text}");
}

const BUILD: &str = "test-build";

fn state_file(home: &Path) -> ReviewStateFile {
    ReviewStateFile::for_subject(&folders_in(home), &pr())
}

/// A question on finding f1 the review agent has.
fn with_a_question(home: &Path) {
    let state: ReviewState = serde_json::from_value(json!({
        "finding_threads": [{
            "finding": {"id": "f1", "severity": "nit", "body": "Rename.", "anchor": {"kind": "change"}},
            "messages": [{"author": "reviewer", "body": "Why?"}],
            "state": "Delivered",
        }]
    }))
    .unwrap();
    state_file(home).save(&state).unwrap();
}

fn agent_reply(repo: &FixtureRepo, args: &[&str]) -> Run {
    GuidedReview::new(repo, &FakeGitHub { url: String::new() })
        .with_env("GUIDED_REVIEW_BUILD", Path::new(BUILD))
        .run(&[&["agent-reply", PR, "f1"], args].concat())
}

#[test]
fn with_no_app_running_the_review_agents_answer_goes_on_the_findings_thread_in_the_state_file() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    with_a_question(repo.home());

    let working = agent_reply(&repo, &["--working"]);
    assert!(working.success, "{}", working.stderr);
    let state = state_file(repo.home()).load().unwrap();
    assert_eq!(
        state
            .finding_threads()
            .thread_of(&id("f1"))
            .unwrap()
            .state(),
        &WhyState::Working
    );

    let answered = agent_reply(&repo, &["--body", "Because it shouts."]);
    assert!(answered.success, "{}", answered.stderr);
    let state = state_file(repo.home()).load().unwrap();
    let thread = state.finding_threads().thread_of(&id("f1")).unwrap();
    assert_eq!(thread.state(), &WhyState::Answered);
    assert_eq!(
        thread.messages().last(),
        Some(&WhyMessage {
            author: WhyAuthor::ReviewAgent,
            body: "Because it shouts.".to_owned(),
        })
    );
}

#[test]
fn a_running_app_that_shows_the_review_takes_the_answer_to_a_question_on_a_finding() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    with_a_question(repo.home());
    let cli_ended = CliEnded::default();
    let (app, seen) = answering_one_request(
        repo.home(),
        &cli_ended,
        BUILD,
        &format!(r#"{{"why_answered":{{"pr":"{PR}"}}}}"#),
    );

    let run = agent_reply(&repo, &["--body", "Because it shouts."]);

    cli_ended.mark();
    app.join().unwrap();
    assert!(run.success, "{}", run.stderr);
    let request: Value = serde_json::from_str(&seen.lock().unwrap()[0]).unwrap();
    assert_eq!(request["request"]["why_replied"]["finding"], "f1");
    assert_eq!(
        request["request"]["why_replied"]["reply"],
        json!({"Answer": "Because it shouts."})
    );
    let state = state_file(repo.home()).load().unwrap();
    assert_eq!(
        state
            .finding_threads()
            .thread_of(&id("f1"))
            .unwrap()
            .state(),
        &WhyState::Delivered,
        "the app keeps the answer"
    );
}

#[test]
fn with_the_review_agent_gone_the_question_goes_privately_to_the_tour_agent_on_the_findings_lines()
{
    let shop = Shop::with_findings(&[on_line("f1", 2, "B is shouted.")]);
    let mut review = shop.open();

    let asked = review.ask_why(&id("f1"), "Why is that wrong?").unwrap();

    let WhyAsked::ToTourAgent(place) = asked else {
        panic!("{asked:?}");
    };
    let request = review
        .start_sending(&place, SendMode::SendNow, Audience::Agent)
        .unwrap();
    let host = FakeHost::with_pull("main-tip", "head", &SentLog::default());
    review.apply_comment(request.run(&host)).unwrap();
    let [hand_off] = review.hand_offs().all() else {
        panic!("one hand-off expected");
    };
    assert_eq!(hand_off.mode(), SendMode::SendNow);
    for part in ["Why is that wrong?", "> B is shouted.", "should fix"] {
        assert!(
            hand_off.comment().contains(part),
            "{part} missing from {}",
            hand_off.comment()
        );
    }
    assert_eq!(
        review.why_state_of(&id("f1")),
        None,
        "no thread of the review agent's"
    );
    assert!(
        finding_row_is_open(&review, "f1"),
        "asking about a finding does not decide it"
    );
}

#[test]
fn with_the_review_agent_gone_a_question_on_a_finding_with_no_lines_is_refused() {
    let shop = Shop::with_findings(&[json!({
        "id": "w1", "severity": "nit", "body": "Overall.", "anchor": {"kind": "change"},
    })]);
    let mut review = shop.open();

    assert_eq!(
        review.ask_why(&id("w1"), "Why?"),
        Err(FindingRefusal::NoReviewAgent)
    );
}

fn finding_row_is_open(review: &Review, finding: &str) -> bool {
    review
        .page()
        .rows()
        .iter()
        .any(|row| matches!(row, PageRow::Finding(row) if row.id == id(finding)))
}
