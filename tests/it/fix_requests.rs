//! Fix requests: findings the reviewer sends to the tour agent to fix, alone or in a batch.

use std::sync::Arc;
use std::time::Instant;

use crate::support::FixtureRepo;
use crate::support::fake_ports::{FakeGitHub, SentLog};
use crate::support::fake_tour_agent::{FakeTourAgent, TourAgentCall};
use crate::support::work::drive_until_idle;
use guided_review::findings::FindingId;
use guided_review::local_git::LocalGit;
use guided_review::open_review::open_review;
use guided_review::pull_request::RepoName;
use guided_review::review_subject::ReviewSubject;
use guided_review::tour::Tour;
use guided_review::viewer::{
    AgentStatus, FindingRefusal, FixReply, FixState, PageRow, Review, TerminalId, TourAgentPort,
    TourAgents, WorkPorts,
};
use serde_json::{Value, json};

const PR: &str = "https://github.com/acme/shop/pull/7";

fn pr() -> ReviewSubject {
    PR.parse().unwrap()
}

/// `README.md` has three changed lines and is the only step.
struct Shop {
    repo: FixtureRepo,
    head: String,
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
        Self { repo, head }
    }

    fn open(&self) -> Review {
        let mut review = open_review(&pr(), self.repo.path(), &self.repo.folders())
            .unwrap()
            .review;
        review.go_forward();
        review
    }

    fn deliver(&self, review: &mut Review, agents: &TourAgents) {
        let source = LocalGit::find(&RepoName::new("acme", "shop"), self.repo.path())
            .unwrap()
            .unwrap();
        let github = FakeGitHub::with_pull("main-tip", &self.head, &SentLog::default());
        let ports = WorkPorts {
            github: &github,
            source: &source,
            agents,
        };
        drive_until_idle(review, &ports, Instant::now());
    }
}

fn on_line(id: &str, line: u32, body: &str) -> Value {
    json!({"id": id, "severity": "should fix", "body": body, "anchor": {
        "kind": "lines", "file": "README.md", "side": "head", "lines": [line, line],
    }})
}

fn id(text: &str) -> FindingId {
    FindingId::try_from(text.to_owned()).unwrap()
}

fn idle_agent() -> Arc<FakeTourAgent> {
    Arc::new(FakeTourAgent::new().with_terminal("t1", Some(PR), AgentStatus::Idle))
}

fn agents_of(fake: &Arc<FakeTourAgent>) -> TourAgents {
    TourAgents::new(Arc::clone(fake) as Arc<dyn TourAgentPort>)
}

#[test]
fn a_batch_of_fix_requests_goes_to_the_tour_agent_as_one_message_in_reading_order() {
    let shop = Shop::with_findings(&[on_line("late", 3, "Third."), on_line("early", 1, "First.")]);
    let mut review = shop.open();
    let fake = idle_agent();

    let batch = review.fix_findings(&[id("late"), id("early")]);
    shop.deliver(&mut review, &agents_of(&fake));

    assert_eq!(batch.refused.len(), 0);
    let calls = fake.calls();
    let [TourAgentCall::Send { terminal, text }] = calls.as_slice() else {
        panic!("{calls:?}");
    };
    assert_eq!(terminal, &TerminalId::new("t1"));
    let first = text.find("First.").unwrap();
    let third = text.find("Third.").unwrap();
    assert!(first < third, "{text}");
    for part in [
        "one subagent",
        "one commit per finding",
        "guided-review fix-reply",
        "README.md",
    ] {
        assert!(text.contains(part), "{part} missing from {text}");
    }
    assert_eq!(
        review.fix_state_of(&id("early")),
        Some(&FixState::Delivered)
    );
}

/// Two changed files, `a.txt` and `b.txt`, the tour reads `b.txt` first.
fn shop_reading_b_before_a(findings: &[Value]) -> Shop {
    let repo = FixtureRepo::cloned_from("acme/shop");
    repo.write("a.txt", "a\n");
    repo.write("b.txt", "b\n");
    let base = repo.commit("base");
    repo.write("a.txt", "A\n");
    repo.write("b.txt", "B\n");
    let head = repo.commit("head");
    let tour = json!({
        "pr": PR, "base": base, "head": head, "title": "Two", "summary": [],
        "steps": [
            {"id": "b", "title": "B", "why": "w", "pieces": [{"file": "b.txt"}]},
            {"id": "a", "title": "A", "why": "w", "pieces": [{"file": "a.txt"}]},
        ],
    });
    let path = Tour::path_for(&repo.folders(), &pr());
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, tour.to_string()).unwrap();
    std::fs::write(
        Tour::findings_path_for(&repo.folders(), &pr()),
        json!({"pr": PR, "base": base, "head": head, "findings": findings}).to_string(),
    )
    .unwrap();
    Shop { repo, head }
}

fn on_file_line(id: &str, file: &str, body: &str) -> Value {
    json!({"id": id, "severity": "should fix", "body": body, "anchor": {
        "kind": "lines", "file": file, "side": "head", "lines": [1, 1],
    }})
}

#[test]
fn a_batch_reads_in_the_order_of_the_tours_steps_not_the_order_of_the_changed_files() {
    let shop = shop_reading_b_before_a(&[
        on_file_line("in-a", "a.txt", "About a."),
        on_file_line("in-b", "b.txt", "About b."),
    ]);
    let mut review = shop.open();
    let fake = idle_agent();

    review.fix_findings(&[id("in-a"), id("in-b")]);
    shop.deliver(&mut review, &agents_of(&fake));

    let calls = fake.calls();
    let [TourAgentCall::Send { text, .. }] = calls.as_slice() else {
        panic!("{calls:?}");
    };
    let (about_a, about_b) = (
        text.find("About a.").unwrap(),
        text.find("About b.").unwrap(),
    );
    assert!(about_b < about_a, "{text}");
}

#[test]
fn a_busy_tour_agent_gets_the_batch_when_it_is_idle() {
    let shop = Shop::with_findings(&[on_line("a", 1, "First.")]);
    let mut review = shop.open();
    let fake = Arc::new(FakeTourAgent::new().with_terminal("t1", Some(PR), AgentStatus::Working));
    let agents = agents_of(&fake);

    review.fix_findings(&[id("a")]);
    shop.deliver(&mut review, &agents);
    assert_eq!(review.fix_state_of(&id("a")), Some(&FixState::Queued));

    fake.set_status("t1", AgentStatus::Idle);
    review.tour_agent_is(&agents.lookup(&pr()));
    shop.deliver(&mut review, &agents);

    assert_eq!(review.fix_state_of(&id("a")), Some(&FixState::Delivered));
}

#[test]
fn a_finding_being_fixed_is_not_sent_again() {
    let shop = Shop::with_findings(&[on_line("a", 1, "First.")]);
    let mut review = shop.open();

    review.fix_findings(&[id("a")]);
    let again = review.fix_findings(&[id("a")]);

    assert_eq!(again.refused, [(id("a"), FindingRefusal::BeingFixed)]);
}

#[test]
fn the_tour_agents_answers_move_each_finding_and_a_fixed_one_closes() {
    let shop = Shop::with_findings(&[
        on_line("a", 1, "First."),
        on_line("b", 2, "Second."),
        on_line("c", 3, "Third."),
    ]);
    let mut review = shop.open();
    let fake = idle_agent();
    review.fix_findings(&[id("a"), id("b"), id("c")]);
    shop.deliver(&mut review, &agents_of(&fake));

    review.fix_replied(&id("a"), &FixReply::Working).unwrap();
    assert_eq!(review.fix_state_of(&id("a")), Some(&FixState::Fixing));
    review
        .fix_replied(
            &id("a"),
            &FixReply::Fixed {
                commit: "abc1234".to_owned(),
            },
        )
        .unwrap();
    review
        .fix_replied(
            &id("b"),
            &FixReply::NotFixed {
                reason: "It is on purpose.".to_owned(),
            },
        )
        .unwrap();

    assert_eq!(
        review.fix_state_of(&id("a")),
        Some(&FixState::Fixed {
            commit: "abc1234".to_owned()
        })
    );
    assert_eq!(
        review.fix_state_of(&id("b")),
        Some(&FixState::NotFixed {
            reason: "It is on purpose.".to_owned()
        })
    );
    assert_eq!(review.ai_review().counts().total(), 2);
    assert_eq!(shop.open().ai_review().counts().total(), 2);
}

#[test]
fn a_finding_the_tour_agent_never_answered_ended_without_an_answer() {
    let shop = Shop::with_findings(&[on_line("a", 1, "First."), on_line("b", 2, "Second.")]);
    let mut review = shop.open();
    let fake = idle_agent();
    let agents = agents_of(&fake);
    review.fix_findings(&[id("a"), id("b")]);
    shop.deliver(&mut review, &agents);

    fake.set_status("t1", AgentStatus::Working);
    review.tour_agent_is(&agents.lookup(&pr()));
    review.fix_replied(&id("a"), &FixReply::Working).unwrap();
    fake.set_status("t1", AgentStatus::Idle);
    review.tour_agent_is(&agents.lookup(&pr()));

    assert_eq!(
        review.fix_state_of(&id("a")),
        Some(&FixState::EndedWithoutAnswer)
    );
    let again = review.fix_findings(&[id("a")]);
    assert_eq!(again.done, [id("a")]);
}

#[test]
fn an_answer_for_a_finding_nobody_asked_to_fix_is_refused() {
    let shop = Shop::with_findings(&[on_line("a", 1, "First.")]);
    let mut review = shop.open();

    let refused = review.fix_replied(&id("a"), &FixReply::Working);

    assert!(refused.is_err());
}

fn fix_lines(review: &Review) -> Vec<(String, Option<String>)> {
    review
        .page()
        .rows()
        .iter()
        .filter_map(|row| match row {
            PageRow::Finding(finding) => Some((
                finding.id.to_string(),
                finding.fix.as_deref().map(str::to_owned),
            )),
            _ => None,
        })
        .collect()
}

#[test]
fn the_finding_on_the_page_says_where_its_fix_is_and_a_fixed_one_leaves() {
    let shop = Shop::with_findings(&[on_line("a", 1, "First."), on_line("b", 2, "Second.")]);
    let mut review = shop.open();
    assert_eq!(
        fix_lines(&review),
        [("a".to_owned(), None), ("b".to_owned(), None)]
    );

    review.fix_findings(&[id("a"), id("b")]);
    assert_eq!(
        fix_lines(&review)[0].1.as_deref(),
        Some("Fix requested: waits for the tour agent")
    );

    shop.deliver(&mut review, &agents_of(&idle_agent()));
    assert_eq!(
        fix_lines(&review)[0].1.as_deref(),
        Some("Fix requested: sent to the tour agent")
    );

    review
        .fix_replied(
            &id("a"),
            &FixReply::NotFixed {
                reason: "On purpose.".to_owned(),
            },
        )
        .unwrap();
    review
        .fix_replied(
            &id("b"),
            &FixReply::Fixed {
                commit: "abc1234".to_owned(),
            },
        )
        .unwrap();
    assert_eq!(
        fix_lines(&review),
        [("a".to_owned(), Some("Not fixed: On purpose.".to_owned()))]
    );
}

#[test]
fn the_findings_list_says_where_each_fix_is() {
    let shop = Shop::with_findings(&[on_line("a", 1, "First."), on_line("b", 2, "Second.")]);
    let mut review = shop.open();

    review.fix_findings(&[id("b")]);
    let list = review.findings_list();

    let fixes: Vec<Option<String>> = list.rows().into_iter().map(|row| row.fix).collect();
    assert_eq!(
        fixes,
        [
            None,
            Some("Fix requested: waits for the tour agent".to_owned())
        ]
    );
    let refreshed = list.refreshed_to(&review.findings_list());
    assert_eq!(
        refreshed.rows()[1].fix.as_deref(),
        Some("Fix requested: waits for the tour agent")
    );
}

#[test]
fn the_sidebar_names_the_commits_of_the_fixes_made() {
    let shop = Shop::with_findings(&[on_line("a", 1, "First."), on_line("b", 2, "Second.")]);
    let mut review = shop.open();
    review.fix_findings(&[id("a")]);

    review
        .fix_replied(
            &id("a"),
            &FixReply::Fixed {
                commit: "abc1234".to_owned(),
            },
        )
        .unwrap();

    assert!(
        review.ai_review_stage().line.contains("1 fix: abc1234"),
        "{}",
        review.ai_review_stage().line
    );
}
