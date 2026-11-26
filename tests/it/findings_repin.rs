//! Findings after new commits: they re-pin like local threads, and one on lines that changed is outdated.

use std::sync::Arc;
use std::time::Instant;

use crate::support::FixtureRepo;
use crate::support::fake_checkouts::FakeCheckouts;
use crate::support::fake_ports::{FakeAskProcesses, FakeHost, SentLog};
use crate::support::fake_tour_agent::{FakeTourAgent, TourAgentCall};
use crate::support::work::drive_until_idle;
use guided_review::config::AskSettings;
use guided_review::file_path::FilePath;
use guided_review::findings::FindingId;
use guided_review::findings::{Anchor, AnchorSide, Finding, FindingsFile};
use guided_review::line_number::LineNumber;
use guided_review::line_range::LineRange;
use guided_review::local_git::LocalGit;
use guided_review::open_review::open_review;
use guided_review::pull_request::Sha;
use guided_review::review_brief::ReviewBrief;
use guided_review::review_subject::ReviewSubject;
use guided_review::tour::Tour;
use guided_review::viewer::{
    AgentStatus, PageRow, Review, ReviewAgents, ReviewStateFile, TerminalId, TourAgentPort,
    TourAgents, WorkPorts, catch_up_findings, repin_tour,
};
use serde_json::{Value, json};

const PR: &str = "https://github.com/acme/shop/pull/7";

fn pr() -> ReviewSubject {
    PR.parse().unwrap()
}

/// `app.txt` changes on lines 2 and 5 at the first head. The second head adds a line on top, so every
/// line moves down by one, and changes line 5 again.
struct Shop {
    repo: FixtureRepo,
    base: String,
    first_head: String,
}

impl Shop {
    fn with_findings(findings: &[Value]) -> Self {
        let repo = FixtureRepo::cloned_from("acme/shop");
        repo.write("app.txt", "one\ntwo\nthree\nfour\nfive\nsix\n");
        repo.write("notes.txt", "a\n");
        let base = repo.commit("base");
        repo.write("app.txt", "one\nTWO\nthree\nfour\nFIVE\nsix\n");
        repo.write("notes.txt", "b\n");
        let first_head = repo.commit("first head");
        let tour = json!({
            "pr": PR, "base": base, "head": first_head, "title": "App", "summary": [],
            "steps": [{"id": "app", "title": "App", "why": "w", "pieces": [{"file": "app.txt"}, {"file": "notes.txt"}]}],
        });
        let path = Tour::path_for(&repo.folders(), &pr());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, tour.to_string()).unwrap();
        std::fs::write(
            Tour::findings_path_for(&repo.folders(), &pr()),
            json!({"pr": PR, "base": base, "head": first_head, "findings": findings}).to_string(),
        )
        .unwrap();
        Self {
            repo,
            base,
            first_head,
        }
    }

    /// What a review agent that started before a re-pin leaves when it finishes after it.
    fn write_findings_read_at_first_head(&self, findings: &[Value]) {
        std::fs::write(
            Tour::findings_path_for(&self.repo.folders(), &pr()),
            json!({"pr": PR, "base": self.base, "head": self.first_head, "findings": findings})
                .to_string(),
        )
        .unwrap();
    }

    fn commit_second_head(&self) -> String {
        self.repo
            .write("app.txt", "zero\none\nTWO\nthree\nfour\nFIVE!\nsix\n");
        self.repo.commit("second head")
    }

    fn repin_onto(&self, head: &str) {
        repin_tour(
            &self.repo.folders(),
            &pr(),
            &Tour::path_for(&self.repo.folders(), &pr()),
            &ReviewStateFile::for_subject(&self.repo.folders(), &pr()),
            &self.source(),
            &Sha::new(&self.base),
            &Sha::new(head),
        )
        .unwrap();
    }

    fn source(&self) -> LocalGit {
        LocalGit::find(&crate::support::shop_on_github(), self.repo.path())
            .unwrap()
            .unwrap()
    }

    fn open(&self) -> Review {
        let mut review = open_review(&pr(), self.repo.path(), &self.repo.folders())
            .unwrap()
            .review;
        review.go_forward();
        review
    }

    fn deliver_fixes(&self, review: &mut Review, head: &str, agent: &Arc<FakeTourAgent>) {
        let host = FakeHost::with_pull("main-tip", head, &SentLog::default());
        let agents = TourAgents::new(Arc::clone(agent) as Arc<dyn TourAgentPort>);
        review.tour_agent_is(&agents.lookup(&pr()));
        let ports = WorkPorts {
            host: &host,
            source: &self.source(),
            agents: &agents,
        };
        drive_until_idle(review, &ports, Instant::now());
    }

    fn review_agents(&self, fake: &Arc<FakeTourAgent>) -> ReviewAgents {
        ReviewAgents::new(
            Arc::clone(fake) as Arc<dyn TourAgentPort>,
            Arc::new(FakeCheckouts::default()),
            self.repo.folders(),
        )
    }

    fn findings(&self) -> FindingsFile {
        FindingsFile::read_of(&self.repo.folders(), &pr())
            .unwrap()
            .unwrap()
    }

    fn finding(&self, id: &str) -> Finding {
        self.findings()
            .findings
            .into_iter()
            .find(|finding| finding.id.to_string() == id)
            .unwrap()
    }
}

fn finding(id: &str, anchor: Value) -> Value {
    json!({"id": id, "severity": "nit", "body": format!("About {id}."), "anchor": anchor})
}

fn on_lines(side: &str, from: u32, to: u32) -> Value {
    json!({"kind": "lines", "file": "app.txt", "side": side, "lines": [from, to]})
}

fn lines(side: AnchorSide, from: u32, to: u32) -> Anchor {
    Anchor::Lines {
        file: FilePath::new("app.txt").unwrap(),
        side,
        lines: LineRange::new(
            LineNumber::try_from(from).unwrap(),
            LineNumber::try_from(to).unwrap(),
        ),
    }
}

#[test]
fn a_finding_on_lines_no_commit_changed_moves_with_them_and_stays_current() {
    let shop = Shop::with_findings(&[finding("two", on_lines("head", 2, 2))]);
    let second = shop.commit_second_head();

    shop.repin_onto(&second);

    let moved = shop.finding("two");
    assert_eq!(moved.anchor, lines(AnchorSide::Head, 3, 3));
    assert_eq!(moved.outdated, None);
}

#[test]
fn a_findings_file_written_after_a_re_pin_at_the_old_head_is_followed_when_the_review_opens() {
    let shop = Shop::with_findings(&[]);
    let second = shop.commit_second_head();
    shop.repin_onto(&second);
    shop.write_findings_read_at_first_head(&[finding("two", on_lines("head", 2, 2))]);

    let review = shop.open();

    assert_eq!(review.ai_review().counts().total(), 1);
    let file = shop.findings();
    assert_eq!(file.pin.head, Sha::new(&second));
    assert_eq!(shop.finding("two").anchor, lines(AnchorSide::Head, 3, 3));
}

#[test]
fn findings_done_after_a_re_pin_follows_a_file_read_at_the_old_head_to_the_current_head() {
    let shop = Shop::with_findings(&[]);
    let second = shop.commit_second_head();
    shop.repin_onto(&second);
    shop.write_findings_read_at_first_head(&[finding("two", on_lines("head", 2, 2))]);

    catch_up_findings(
        &shop.repo.folders(),
        &pr(),
        &ReviewStateFile::for_subject(&shop.repo.folders(), &pr()),
        &shop.source(),
        &Sha::new(&shop.base),
        &Sha::new(&second),
    )
    .unwrap();

    assert_eq!(shop.findings().pin.head, Sha::new(&second));
    assert_eq!(shop.finding("two").anchor, lines(AnchorSide::Head, 3, 3));
}

#[test]
fn a_finding_on_lines_a_commit_changed_is_outdated_with_what_they_said() {
    let shop = Shop::with_findings(&[finding("five", on_lines("head", 4, 5))]);
    let second = shop.commit_second_head();

    shop.repin_onto(&second);

    let moved = shop.finding("five");
    assert_eq!(moved.anchor, lines(AnchorSide::Head, 5, 6));
    assert_eq!(
        moved.outdated.map(|outdated| outdated.before),
        Some(vec!["four".to_owned(), "FIVE".to_owned()])
    );
}

#[test]
fn the_findings_file_is_pinned_to_the_new_head_and_keeps_its_other_findings() {
    let shop = Shop::with_findings(&[
        finding("removed", on_lines("base", 2, 2)),
        finding("file", json!({"kind": "file", "file": "notes.txt"})),
        finding("change", json!({"kind": "change"})),
    ]);
    let second = shop.commit_second_head();

    shop.repin_onto(&second);

    let file = shop.findings();
    assert_eq!(file.pin.head, Sha::new(&second));
    assert_ne!(file.pin.head, Sha::new(&shop.first_head));
    let anchors: Vec<(String, Anchor, bool)> = file
        .findings
        .into_iter()
        .map(|f| (f.id.to_string(), f.anchor, f.outdated.is_some()))
        .collect();
    assert_eq!(
        anchors,
        vec![
            ("removed".to_owned(), lines(AnchorSide::Base, 2, 2), false),
            (
                "file".to_owned(),
                Anchor::File {
                    file: FilePath::new("notes.txt").unwrap()
                },
                false
            ),
            ("change".to_owned(), Anchor::Change, false),
        ]
    );
}

#[test]
fn a_second_re_pin_keeps_what_the_lines_said_when_the_finding_was_written() {
    let shop = Shop::with_findings(&[finding("five", on_lines("head", 5, 5))]);
    let second = shop.commit_second_head();
    shop.repin_onto(&second);
    shop.repo
        .write("app.txt", "zero\none\nTWO\nthree\nfour\nFIVE?\nsix\n");
    let third = shop.repo.commit("third head");

    shop.repin_onto(&third);

    assert_eq!(
        shop.finding("five")
            .outdated
            .map(|outdated| outdated.before),
        Some(vec!["FIVE".to_owned()])
    );
}

#[test]
fn a_re_pin_with_no_findings_file_writes_none() {
    let shop = Shop::with_findings(&[]);
    std::fs::remove_file(Tour::findings_path_for(&shop.repo.folders(), &pr())).unwrap();
    let second = shop.commit_second_head();

    shop.repin_onto(&second);

    assert!(
        FindingsFile::read_of(&shop.repo.folders(), &pr())
            .unwrap()
            .is_none()
    );
}

fn id(text: &str) -> FindingId {
    FindingId::try_from(text.to_owned()).unwrap()
}

fn sent_text(fake: &FakeTourAgent) -> String {
    fake.calls()
        .into_iter()
        .find_map(|call| match call {
            TourAgentCall::Send { text, .. } => Some(text),
            _ => None,
        })
        .unwrap()
}

fn finding_row(review: &Review, finding: &str) -> Option<Arc<guided_review::viewer::FindingRow>> {
    review.page().rows().iter().find_map(|row| match row {
        PageRow::Finding(row) if row.id == id(finding) => Some(Arc::clone(row)),
        _ => None,
    })
}

#[test]
fn a_dismissed_finding_stays_dismissed_and_the_next_run_is_told_its_new_lines() {
    let shop = Shop::with_findings(&[finding("two", on_lines("head", 2, 2))]);
    shop.open().dismiss_finding(&id("two")).unwrap();
    let second = shop.commit_second_head();

    shop.repin_onto(&second);

    assert!(finding_row(&shop.open(), "two").is_none());
    let brief = ReviewBrief::of(&shop.repo.folders(), &pr()).to_string();
    assert!(brief.contains("app.txt line 3 of the head"), "{brief}");
}

#[test]
fn a_fix_request_waiting_for_the_tour_agent_follows_its_finding_to_the_new_lines() {
    let shop = Shop::with_findings(&[finding("two", on_lines("head", 2, 2))]);
    let batch = shop.open().fix_findings(&[id("two")]);
    assert_eq!(batch.refused.len(), 0);
    let second = shop.commit_second_head();

    shop.repin_onto(&second);
    let mut review = shop.open();
    let agent = Arc::new(FakeTourAgent::new().with_terminal("t1", Some(PR), AgentStatus::Idle));
    shop.deliver_fixes(&mut review, &second, &agent);

    let text = sent_text(&agent);
    assert!(
        text.contains("Place:\n> app.txt line 3 of the head"),
        "{text}"
    );
    assert!(finding_row(&review, "two").unwrap().fix.is_some());
}

#[test]
fn a_question_waiting_for_the_review_agent_follows_its_finding_to_the_new_lines() {
    let shop = Shop::with_findings(&[finding("two", on_lines("head", 2, 2))]);
    let mut review = shop.open();
    review.ai_review_agent_resumed(TerminalId::new("r1"), Some(AgentStatus::Working));
    review
        .ask_why(
            &id("two"),
            "Why?",
            &AskSettings::default(),
            Instant::now(),
            &mut FakeAskProcesses::default(),
        )
        .unwrap();
    let second = shop.commit_second_head();

    shop.repin_onto(&second);
    let mut review = shop.open();
    let fake = Arc::new(FakeTourAgent::new().review_agent("r1", PR, AgentStatus::Idle, 0));
    review.ai_review_agent_resumed(TerminalId::new("r1"), Some(AgentStatus::Idle));
    let delivery = review.why_delivery().unwrap();
    review.why_delivered(delivery.run(&shop.review_agents(&fake)));

    let text = sent_text(&fake);
    assert!(text.contains("app.txt line 3 of the head"), "{text}");
    assert_eq!(finding_row(&review, "two").unwrap().thread.len(), 1);
}

#[test]
fn an_outdated_finding_shows_on_its_lines_with_what_they_said_and_is_marked_in_the_findings_list() {
    let shop = Shop::with_findings(&[
        finding("two", on_lines("head", 2, 2)),
        finding("five", on_lines("head", 5, 5)),
    ]);
    let second = shop.commit_second_head();

    shop.repin_onto(&second);
    let review = shop.open();

    let five = finding_row(&review, "five").unwrap();
    assert_eq!(five.outdated.as_deref(), Some(&["FIVE".to_owned()][..]));
    assert_eq!(finding_row(&review, "two").unwrap().outdated, None);
    let marks: Vec<(String, bool)> = review
        .findings_list()
        .rows()
        .into_iter()
        .map(|row| (row.id.to_string(), row.is_outdated))
        .collect();
    assert_eq!(
        marks,
        vec![("two".to_owned(), false), ("five".to_owned(), true)]
    );
}
