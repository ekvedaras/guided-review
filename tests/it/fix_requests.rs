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
    AgentStatus, FindingRefusal, FixNote, FixReply, FixState, FixTiming, FixesOffer, PageRow,
    Review, TerminalId, TourAgentPort, TourAgents, WhyAuthor, WorkPorts,
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
fn the_tour_agents_answers_move_each_finding_and_a_fixed_one_stays_open() {
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
                summary: None,
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
            commit: "abc1234".to_owned(),
            summary: None,
        })
    );
    assert_eq!(
        review.fix_state_of(&id("b")),
        Some(&FixState::NotFixed {
            reason: "It is on purpose.".to_owned()
        })
    );
    assert_eq!(review.ai_review().counts().total(), 3);
    assert_eq!(shop.open().ai_review().counts().total(), 3);
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

fn said_on_thread(review: &Review, finding: &str) -> Vec<(WhyAuthor, String)> {
    review
        .finding_thread(&id(finding))
        .map_or_default(|thread| {
            thread
                .messages()
                .iter()
                .map(|message| (message.author, message.body.clone()))
                .collect()
        })
}

#[test]
fn the_fix_request_and_the_tour_agents_answer_are_said_on_the_findings_thread() {
    let shop = Shop::with_findings(&[on_line("a", 1, "First."), on_line("b", 2, "Second.")]);
    let mut review = shop.open();
    let asked_why = |review: &Review| {
        review.page().rows().iter().find_map(|row| match row {
            PageRow::Finding(finding) if finding.id == id("a") => finding.asked.clone(),
            _ => None,
        })
    };

    review.fix_findings(&[id("a"), id("b")]);

    assert_eq!(
        said_on_thread(&review, "a"),
        [(WhyAuthor::Reviewer, "Fix requested".to_owned())]
    );
    assert_eq!(asked_why(&review), None);
    review
        .fix_replied(
            &id("a"),
            &FixReply::NotFixed {
                reason: "It is on purpose.".to_owned(),
            },
        )
        .unwrap();
    review
        .fix_replied(
            &id("b"),
            &FixReply::Fixed {
                commit: "abc1234".to_owned(),
                summary: None,
            },
        )
        .unwrap();

    assert_eq!(
        said_on_thread(&review, "a"),
        [
            (WhyAuthor::Reviewer, "Fix requested".to_owned()),
            (
                WhyAuthor::TourAgent,
                "Not fixed: It is on purpose.".to_owned()
            ),
        ]
    );
    assert_eq!(
        said_on_thread(&review, "b"),
        [
            (WhyAuthor::Reviewer, "Fix requested".to_owned()),
            (WhyAuthor::TourAgent, "Fixed in abc1234".to_owned()),
        ]
    );
    assert_eq!(
        said_on_thread(&shop.open(), "a").len(),
        2,
        "the thread is kept with the review"
    );
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
fn the_finding_on_the_page_says_where_its_fix_is_and_a_fixed_one_stays() {
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
                summary: None,
            },
        )
        .unwrap();
    assert_eq!(
        fix_lines(&review),
        [
            ("a".to_owned(), Some("Not fixed: On purpose.".to_owned())),
            ("b".to_owned(), Some("Fixed in abc1234".to_owned())),
        ]
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
                summary: None,
            },
        )
        .unwrap();

    assert!(
        review.ai_review_stage().line.contains("1 fix: abc1234"),
        "{}",
        review.ai_review_stage().line
    );
}

/// A fix request that the tour agent has started on and then asked the reviewer about.
fn asked_by_the_tour_agent(
    shop: &Shop,
    review: &mut Review,
    fake: &Arc<FakeTourAgent>,
    text: &str,
) {
    let agents = agents_of(fake);
    review.fix_findings(&[id("a"), id("b")]);
    shop.deliver(review, &agents);
    fake.set_status("t1", AgentStatus::Working);
    review.tour_agent_is(&agents.lookup(&pr()));
    review
        .fix_replied(
            &id("a"),
            &FixReply::Question {
                text: text.to_owned(),
            },
        )
        .unwrap();
}

#[test]
fn a_question_from_the_tour_agent_waits_for_the_reviewer_even_when_the_agent_goes_idle() {
    let shop = Shop::with_findings(&[on_line("a", 1, "First."), on_line("b", 2, "Second.")]);
    let mut review = shop.open();
    let fake = idle_agent();
    let agents = agents_of(&fake);
    asked_by_the_tour_agent(&shop, &mut review, &fake, "Rename it to what?");

    fake.set_status("t1", AgentStatus::Idle);
    review.tour_agent_is(&agents.lookup(&pr()));

    assert_eq!(review.fix_state_of(&id("a")), Some(&FixState::Asked));
    assert_eq!(
        said_on_thread(&review, "a").last(),
        Some(&(WhyAuthor::TourAgent, "Rename it to what?".to_owned()))
    );
    let row = fix_rows(&review).remove(0);
    assert_eq!(
        row.fix.as_deref(),
        Some("The tour agent asks you something: answer it below")
    );
    assert!(row.can_answer_fix);
    assert!(!row.can_fix);
    assert!(
        !fix_rows(&review)[1].can_answer_fix,
        "only a finding with a question can be answered"
    );
}

fn fix_rows(review: &Review) -> Vec<Arc<guided_review::viewer::FindingRow>> {
    review
        .page()
        .rows()
        .iter()
        .filter_map(|row| match row {
            PageRow::Finding(finding) => Some(Arc::clone(finding)),
            _ => None,
        })
        .collect()
}

#[test]
fn the_reviewers_answer_is_typed_into_the_tour_agent_and_said_on_the_thread() {
    let shop = Shop::with_findings(&[on_line("a", 1, "First."), on_line("b", 2, "Second.")]);
    let mut review = shop.open();
    let fake = idle_agent();
    let agents = agents_of(&fake);
    asked_by_the_tour_agent(&shop, &mut review, &fake, "Rename it to what?");
    fake.set_status("t1", AgentStatus::Idle);
    review.tour_agent_is(&agents.lookup(&pr()));
    let sent_before = fake.calls().len();

    review
        .answer_fix_question(&id("a"), "Call it total.")
        .unwrap();
    assert_eq!(
        review.fix_state_of(&id("a")),
        Some(&FixState::AnswerQueued {
            answer: "Call it total.".to_owned()
        })
    );
    shop.deliver(&mut review, &agents);

    let calls = fake.calls();
    let Some(TourAgentCall::Send { text, .. }) = calls.get(sent_before) else {
        panic!("{calls:?}");
    };
    for part in [
        "Call it total.",
        "guided-review fix-reply",
        "--not-fixed",
        "a",
    ] {
        assert!(text.contains(part), "{part} missing from {text}");
    }
    assert_eq!(review.fix_state_of(&id("a")), Some(&FixState::Delivered));
    assert_eq!(
        said_on_thread(&review, "a").last(),
        Some(&(WhyAuthor::Reviewer, "Call it total.".to_owned()))
    );
}

#[test]
fn an_answer_waits_for_a_busy_tour_agent() {
    let shop = Shop::with_findings(&[on_line("a", 1, "First."), on_line("b", 2, "Second.")]);
    let mut review = shop.open();
    let fake = idle_agent();
    let agents = agents_of(&fake);
    asked_by_the_tour_agent(&shop, &mut review, &fake, "Rename it to what?");
    review
        .answer_fix_question(&id("a"), "Call it total.")
        .unwrap();

    shop.deliver(&mut review, &agents);
    assert!(matches!(
        review.fix_state_of(&id("a")),
        Some(FixState::AnswerQueued { .. })
    ));

    fake.set_status("t1", AgentStatus::Idle);
    review.tour_agent_is(&agents.lookup(&pr()));
    shop.deliver(&mut review, &agents);

    assert_eq!(review.fix_state_of(&id("a")), Some(&FixState::Delivered));
}

#[test]
fn only_a_question_the_tour_agent_asked_can_be_answered() {
    let shop = Shop::with_findings(&[on_line("a", 1, "First.")]);
    let mut review = shop.open();
    assert_eq!(
        review.answer_fix_question(&id("a"), "Yes."),
        Err(FindingRefusal::NoFixQuestion)
    );

    review.fix_findings(&[id("a")]);
    assert_eq!(
        review.answer_fix_question(&id("a"), "Yes."),
        Err(FindingRefusal::NoFixQuestion)
    );
}

#[test]
fn the_fix_request_tells_the_tour_agent_to_ask_through_the_app_not_in_its_terminal() {
    let shop = Shop::with_findings(&[on_line("a", 1, "First.")]);
    let mut review = shop.open();
    let fake = idle_agent();

    review.fix_findings(&[id("a")]);
    shop.deliver(&mut review, &agents_of(&fake));

    let calls = fake.calls();
    let [TourAgentCall::Send { text, .. }] = calls.as_slice() else {
        panic!("{calls:?}");
    };
    assert!(text.contains("--question --body -"), "{text}");
    assert!(text.contains("not in your terminal"), "{text}");
}

#[test]
fn a_finding_added_to_fixes_waits_for_send_fixes() {
    let shop = Shop::with_findings(&[on_line("a", 1, "First."), on_line("b", 2, "Second.")]);
    let mut review = shop.open();
    let fake = idle_agent();

    let added = review.add_to_fixes(&[id("a"), id("b")]);
    shop.deliver(&mut review, &agents_of(&fake));

    assert_eq!(added.refused.len(), 0);
    assert_eq!(fake.calls().len(), 0, "{:?}", fake.calls());
    assert_eq!(review.fix_state_of(&id("a")), Some(&FixState::ToSend));
    assert_eq!(review.fixes_to_send(), 2);
}

#[test]
fn send_fixes_sends_every_finding_added_to_fixes_as_one_message_in_reading_order() {
    let shop = Shop::with_findings(&[on_line("late", 3, "Third."), on_line("early", 1, "First.")]);
    let mut review = shop.open();
    let fake = idle_agent();
    review.add_to_fixes(&[id("late")]);
    review.add_to_fixes(&[id("early")]);

    let sent = review.send_fixes();
    shop.deliver(&mut review, &agents_of(&fake));

    assert_eq!(sent.refused.len(), 0);
    let calls = fake.calls();
    let [TourAgentCall::Send { text, .. }] = calls.as_slice() else {
        panic!("{calls:?}");
    };
    assert!(
        text.find("First.").unwrap() < text.find("Third.").unwrap(),
        "{text}"
    );
    assert_eq!(review.fix_state_of(&id("late")), Some(&FixState::Delivered));
    assert_eq!(review.fixes_to_send(), 0);
}

#[test]
fn a_finding_taken_out_of_fixes_is_not_sent() {
    let shop = Shop::with_findings(&[on_line("a", 1, "First."), on_line("b", 2, "Second.")]);
    let mut review = shop.open();
    let fake = idle_agent();
    review.add_to_fixes(&[id("a"), id("b")]);

    review.remove_from_fixes(&id("a")).unwrap();
    review.send_fixes();
    shop.deliver(&mut review, &agents_of(&fake));

    assert_eq!(review.fix_state_of(&id("a")), None);
    let calls = fake.calls();
    let [TourAgentCall::Send { text, .. }] = calls.as_slice() else {
        panic!("{calls:?}");
    };
    assert!(!text.contains("First."), "{text}");
    assert_eq!(
        review.remove_from_fixes(&id("a")),
        Err(FindingRefusal::NotInFixes)
    );
}

#[test]
fn a_dismissed_finding_leaves_the_fixes_to_send() {
    let shop = Shop::with_findings(&[on_line("a", 1, "First.")]);
    let mut review = shop.open();
    review.add_to_fixes(&[id("a")]);

    review.dismiss_finding(&id("a")).unwrap();

    assert_eq!(review.fixes_to_send(), 0);
    assert_eq!(review.fix_state_of(&id("a")), None);
}

#[test]
fn the_fixes_to_send_are_kept_when_the_review_is_opened_again() {
    let shop = Shop::with_findings(&[on_line("a", 1, "First.")]);
    shop.open().add_to_fixes(&[id("a")]);

    let review = shop.open();

    assert_eq!(review.fixes_to_send(), 1);
}

#[test]
fn fix_now_on_a_finding_in_fixes_sends_only_it() {
    let shop = Shop::with_findings(&[on_line("a", 1, "First."), on_line("b", 2, "Second.")]);
    let mut review = shop.open();
    let fake = idle_agent();
    review.add_to_fixes(&[id("a"), id("b")]);

    review.fix_findings(&[id("a")]);
    shop.deliver(&mut review, &agents_of(&fake));

    assert_eq!(review.fix_state_of(&id("a")), Some(&FixState::Delivered));
    assert_eq!(review.fix_state_of(&id("b")), Some(&FixState::ToSend));
    assert_eq!(review.fixes_to_send(), 1);
}

#[test]
fn a_finding_being_fixed_cannot_be_added_to_fixes() {
    let shop = Shop::with_findings(&[on_line("a", 1, "First.")]);
    let mut review = shop.open();
    review.fix_findings(&[id("a")]);

    let added = review.add_to_fixes(&[id("a")]);

    assert_eq!(added.refused, [(id("a"), FindingRefusal::BeingFixed)]);
}

fn fixes_offer_of(review: &Review, finding: &str) -> FixesOffer {
    review
        .page()
        .rows()
        .iter()
        .find_map(|row| match row {
            PageRow::Finding(row) if row.id.to_string() == finding => Some(row.fixes_offer),
            _ => None,
        })
        .unwrap()
}

#[test]
fn a_finding_offers_add_to_fixes_then_remove_and_neither_while_it_is_being_fixed() {
    let shop = Shop::with_findings(&[on_line("a", 1, "First.")]);
    let mut review = shop.open();
    assert_eq!(fixes_offer_of(&review, "a"), FixesOffer::Add);

    review.add_to_fixes(&[id("a")]);
    assert_eq!(fixes_offer_of(&review, "a"), FixesOffer::Remove);
    assert_eq!(
        fix_lines(&review)[0].1.as_deref(),
        Some("To fix: goes with Send fixes")
    );

    review.send_fixes();
    assert_eq!(fixes_offer_of(&review, "a"), FixesOffer::None);
}

#[test]
fn a_finding_being_fixed_cannot_be_dismissed_or_used() {
    let shop = Shop::with_findings(&[on_line("a", 1, "First.")]);
    let mut review = shop.open();
    review.fix_findings(&[id("a")]);

    assert_eq!(
        review.dismiss_finding(&id("a")),
        Err(FindingRefusal::BeingFixed)
    );
    assert_eq!(
        review.use_finding(&id("a")).err(),
        Some(FindingRefusal::BeingFixed)
    );
    let mode = review.add_mode();
    assert_eq!(
        review.use_finding_as_is(&id("a"), mode).err(),
        Some(FindingRefusal::BeingFixed)
    );
    let commented = review.comment_findings(&[id("a")], mode);
    assert_eq!(commented.refused, [(id("a"), FindingRefusal::BeingFixed)]);
    assert!(!can_decide(&review, "a"));
}

#[test]
fn a_finding_added_to_fixes_can_still_be_decided() {
    let shop = Shop::with_findings(&[on_line("a", 1, "First.")]);
    let mut review = shop.open();
    review.add_to_fixes(&[id("a")]);

    assert!(can_decide(&review, "a"));
}

fn can_decide(review: &Review, finding: &str) -> bool {
    review
        .page()
        .rows()
        .iter()
        .find_map(|row| match row {
            PageRow::Finding(row) if row.id.to_string() == finding => Some(row.can_decide),
            _ => None,
        })
        .unwrap()
}

#[test]
fn a_finding_the_tour_agent_asked_about_can_still_be_dismissed() {
    let shop = Shop::with_findings(&[on_line("a", 1, "First."), on_line("b", 2, "Second.")]);
    let mut review = shop.open();
    asked_by_the_tour_agent(&shop, &mut review, &idle_agent(), "Which date?");
    assert!(can_decide(&review, "a"));

    review.dismiss_finding(&id("a")).unwrap();

    assert_eq!(review.fix_state_of(&id("a")), None);
}

#[test]
fn a_finding_the_tour_agent_asked_about_can_still_be_used() {
    let shop = Shop::with_findings(&[on_line("a", 1, "First."), on_line("b", 2, "Second.")]);
    let mut review = shop.open();
    asked_by_the_tour_agent(&shop, &mut review, &idle_agent(), "Which date?");

    assert!(review.use_finding(&id("a")).is_ok());
}

fn note(text: &str) -> FixNote {
    FixNote::new(text).unwrap()
}

#[test]
fn fix_now_with_a_note_tells_the_tour_agent_the_note_and_says_it_on_the_thread() {
    let shop = Shop::with_findings(&[on_line("a", 1, "First.")]);
    let mut review = shop.open();
    let fake = idle_agent();

    review
        .fix_with_note(&id("a"), note("Use the loading date."), FixTiming::Now)
        .unwrap();
    shop.deliver(&mut review, &agents_of(&fake));

    let calls = fake.calls();
    let [TourAgentCall::Send { text, .. }] = calls.as_slice() else {
        panic!("{calls:?}");
    };
    assert!(text.contains("Use the loading date."), "{text}");
    let thread = review.finding_thread(&id("a")).unwrap();
    assert_eq!(
        thread.messages().last().unwrap().body,
        "Fix requested: Use the loading date."
    );
}

#[test]
fn a_note_given_with_add_to_fixes_shows_on_the_finding_and_goes_with_send_fixes() {
    let shop = Shop::with_findings(&[on_line("a", 1, "First.")]);
    let mut review = shop.open();
    let fake = idle_agent();

    review
        .fix_with_note(
            &id("a"),
            note("Use the loading date."),
            FixTiming::WithSendFixes,
        )
        .unwrap();
    assert_eq!(
        fix_lines(&review)[0].1.as_deref(),
        Some("To fix: goes with Send fixes. Your note: Use the loading date.")
    );
    review.send_fixes();
    shop.deliver(&mut review, &agents_of(&fake));

    let calls = fake.calls();
    let [TourAgentCall::Send { text, .. }] = calls.as_slice() else {
        panic!("{calls:?}");
    };
    assert!(text.contains("Use the loading date."), "{text}");
}

#[test]
fn a_blank_note_is_no_note() {
    assert_eq!(FixNote::new("  \n "), None);
}

fn fixed_by_the_tour_agent(shop: &Shop, review: &mut Review, summary: Option<&str>) {
    review.fix_findings(&[id("a")]);
    shop.deliver(review, &agents_of(&idle_agent()));
    review
        .fix_replied(
            &id("a"),
            &FixReply::Fixed {
                commit: "abc1234".to_owned(),
                summary: summary.map(str::to_owned),
            },
        )
        .unwrap();
}

#[test]
fn a_fixed_finding_stays_open_with_what_the_tour_agent_did_until_it_is_marked_fixed() {
    let shop = Shop::with_findings(&[on_line("a", 1, "First.")]);
    let mut review = shop.open();
    fixed_by_the_tour_agent(&shop, &mut review, Some("Priced on the loading date."));

    assert_eq!(
        said_on_thread(&review, "a").last().unwrap(),
        &(
            WhyAuthor::TourAgent,
            "Fixed in abc1234: Priced on the loading date.".to_owned()
        )
    );
    assert!(can_mark_fixed(&review, "a"));

    review.mark_fixed(&id("a")).unwrap();

    assert_eq!(review.ai_review().counts().total(), 0);
    assert_eq!(shop.open().ai_review().counts().total(), 0);
}

#[test]
fn only_a_finding_the_tour_agent_fixed_can_be_marked_fixed() {
    let shop = Shop::with_findings(&[on_line("a", 1, "First.")]);
    let mut review = shop.open();
    assert!(!can_mark_fixed(&review, "a"));

    assert_eq!(
        review.mark_fixed(&id("a")),
        Err(FindingRefusal::NotFixedYet)
    );
}

#[test]
fn a_fixed_finding_can_be_sent_back_with_a_note_for_more() {
    let shop = Shop::with_findings(&[on_line("a", 1, "First.")]);
    let mut review = shop.open();
    let fake = idle_agent();
    fixed_by_the_tour_agent(&shop, &mut review, Some("Priced on the loading date."));

    review
        .fix_with_note(
            &id("a"),
            note("Also cover the sorting date."),
            FixTiming::Now,
        )
        .unwrap();
    shop.deliver(&mut review, &agents_of(&fake));

    assert!(!can_mark_fixed(&review, "a"));
    let calls = fake.calls();
    let [TourAgentCall::Send { text, .. }] = calls.as_slice() else {
        panic!("{calls:?}");
    };
    assert!(text.contains("Also cover the sorting date."), "{text}");
}

#[test]
fn the_fix_request_asks_the_tour_agent_to_say_what_it_did() {
    let shop = Shop::with_findings(&[on_line("a", 1, "First.")]);
    let mut review = shop.open();
    let fake = idle_agent();

    review.fix_findings(&[id("a")]);
    shop.deliver(&mut review, &agents_of(&fake));

    let calls = fake.calls();
    let [TourAgentCall::Send { text, .. }] = calls.as_slice() else {
        panic!("{calls:?}");
    };
    assert!(text.contains("--fixed <commit sha> --body -"), "{text}");
}

fn can_mark_fixed(review: &Review, finding: &str) -> bool {
    review
        .page()
        .rows()
        .iter()
        .find_map(|row| match row {
            PageRow::Finding(row) if row.id.to_string() == finding => Some(row.can_mark_fixed),
            _ => None,
        })
        .unwrap()
}
