//! A local review has no PR to open on GitHub or to add notes to: it asks its agent to open one.

use std::sync::Arc;
use std::time::Instant;

use crate::support::fake_ports::{FakeHost, SentLog};
use crate::support::fake_tour_agent::{FakeTourAgent, TourAgentCall};
use crate::support::work::drive_without_agents;
use crate::support::{FakeGitHub, FixtureRepo, GuidedReview};
use guided_review::build::Build;
use guided_review::local_work_tree::LocalWorkTree;
use guided_review::open_review::{OpenedReview, open_review};
use guided_review::opened_subject::OpenedChangeSet;
use guided_review::review_subject::ReviewSubject;
use guided_review::tour::{StepId, Tour};
use guided_review::viewer::{
    AgentStatus, BranchOffer, Command, Effect, Intent, NoteSubject, OwnAgentHandOff, PageRow,
    PaletteAction, Review, ReviewStateFile, TerminalId, ToastTone, TourAgentPort, TourAgents,
    WindowMoment,
};
use serde_json::json;

const SHOP_7: &str = "https://github.com/acme/shop/pull/7";

/// `feature` one commit past `main` that adds the cart, with the README changed and not committed.
struct Feature {
    repo: FixtureRepo,
    subject: ReviewSubject,
    sent: SentLog,
}

impl Feature {
    fn with_work_in_progress() -> Self {
        let repo = FixtureRepo::cloned_from("acme/shop");
        repo.write("README.md", "hello\n");
        repo.commit("Start the shop");
        repo.git(&["checkout", "-q", "-b", "feature"]);
        repo.write("src/cart.rs", "pub fn total() {}\n");
        repo.commit("Add the cart");
        repo.write("README.md", "hello\nworld\n");
        let work_tree = LocalWorkTree::containing(repo.path()).unwrap();
        let subject = ReviewSubject::from(work_tree.change_set().unwrap());
        Self {
            repo,
            subject,
            sent: SentLog::default(),
        }
    }

    fn open(&self) -> OpenedReview {
        open_review(&self.subject, self.repo.path(), &self.repo.folders()).unwrap()
    }

    /// A guided tour whose step has a diagram and whose reviewer summary has a picture, checked, and
    /// with no basic tour beside it, so it is the one that opens.
    fn with_guided_tour(self) -> Self {
        OpenedChangeSet::new(self.subject.local().unwrap(), &self.repo.folders())
            .make_basic_tour_if_no_tour()
            .unwrap();
        let path = Tour::path_for(&self.repo.folders(), &self.subject);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path.parent().unwrap().join("before.png"), b"png").unwrap();
        let tour = json!({
            "title": "Cart",
            "summary": ["A cart"],
            "reviewer_summary": "Looks fine\n\n![Before](before.png)",
            "steps": [
                {"id": "readme", "title": "Readme", "why": "w", "diagram": "flowchart LR\n  a --> b",
                 "pieces": [{"file": "README.md"}]},
                {"id": "cart", "title": "Cart", "why": "w", "pieces": [{"rest": ""}]},
            ],
        });
        std::fs::write(&path, tour.to_string()).unwrap();
        let unreachable = FakeGitHub {
            url: "http://127.0.0.1:1".to_owned(),
        };
        let run =
            GuidedReview::new(&self.repo, &unreachable).run(&["check", &self.subject.to_string()]);
        assert!(run.success, "{}{}", run.stdout, run.stderr);
        std::fs::remove_file(Tour::basic_path_for(&self.repo.folders(), &self.subject)).unwrap();
        self
    }

    /// The window comes to the front, and the review looks for the PR opened from its branch.
    fn focus(&self, opened: &mut OpenedReview, host: &FakeHost) {
        opened.review.pull_from_host();
        drive_without_agents(
            &mut opened.review,
            host,
            opened.source.as_ref(),
            Instant::now(),
        );
    }
}

fn pr_review(repo: &FixtureRepo) -> Review {
    repo.write("README.md", "hello\n");
    let base = repo.commit("base");
    repo.write("README.md", "hello\nworld\n");
    let head = repo.commit("head");
    let tour = serde_json::from_value(json!({
        "pr": SHOP_7, "base": base, "head": head, "title": "Hello world",
        "steps": [{"id": "one", "title": "One"}],
    }))
    .unwrap();
    let source =
        guided_review::local_git::LocalGit::find(&crate::support::shop_on_github(), repo.path())
            .unwrap()
            .unwrap();
    let state_file = ReviewStateFile::for_subject(&repo.folders(), &SHOP_7.parse().unwrap());
    Review::open(
        tour,
        &source,
        state_file,
        crate::support::fake_ports::every_capability(),
    )
    .unwrap()
}

fn offered(review: &Review, action: PaletteAction) -> bool {
    review.palette_actions(Build::Local).contains(&action)
}

#[test]
fn a_local_review_offers_open_pr_and_not_open_on_github() {
    let feature = Feature::with_work_in_progress();
    let opened = feature.open();

    assert_eq!(opened.review.branch_offer(), Some(BranchOffer::OpenPr));
    assert!(opened.review.can_open_pr());
    assert!(offered(&opened.review, PaletteAction::OpenPr));
    assert!(!offered(&opened.review, PaletteAction::OpenOnGitHub));
    assert_eq!(PaletteAction::OpenPr.label(), "Open PR");
    assert_eq!(PaletteAction::OpenPr.command(), Some(Command::OpenPr));
    assert_eq!(Command::OpenPr.default_chords()[0].keystroke(), "p");
    assert_eq!(Command::OpenPr.config_name(), "open_pr");
    assert!(Command::ALL.contains(&Command::OpenPr));
    assert_eq!(
        opened
            .review
            .decide(Intent::OpenPr, WindowMoment::default()),
        Effect::OpenPr
    );
}

#[test]
fn a_pr_review_offers_open_on_github_and_not_open_pr() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    let review = pr_review(&repo);

    assert_eq!(review.branch_offer(), None);
    assert!(!review.can_open_pr());
    assert!(offered(&review, PaletteAction::OpenOnGitHub));
    assert!(!offered(&review, PaletteAction::OpenPr));
    assert_eq!(
        review.decide(Intent::OpenPr, WindowMoment::default()),
        Effect::Nothing
    );
}

#[test]
fn once_the_pr_of_the_branch_is_open_the_local_review_offers_the_move_instead_of_open_pr() {
    let feature = Feature::with_work_in_progress();
    let host = FakeHost::with_pull("main-tip", "snapshot", &feature.sent).with_open_pull_from(
        "feature",
        SHOP_7,
        "Add the cart",
    );
    let mut opened = feature.open();

    feature.focus(&mut opened, &host);

    assert!(matches!(
        opened.review.branch_offer(),
        Some(BranchOffer::MoveToPr(_))
    ));
    assert!(!opened.review.can_open_pr());
    assert!(!offered(&opened.review, PaletteAction::OpenPr));
    assert_eq!(
        opened
            .review
            .decide(Intent::OpenPr, WindowMoment::default()),
        Effect::Nothing
    );
}

#[test]
fn a_local_review_that_kept_away_from_its_pr_offers_neither_the_move_nor_open_pr() {
    let feature = Feature::with_work_in_progress();
    let host = FakeHost::with_pull("main-tip", "snapshot", &feature.sent).with_open_pull_from(
        "feature",
        SHOP_7,
        "Add the cart",
    );
    let mut opened = feature.open();
    feature.focus(&mut opened, &host);

    opened.review.decline_the_move();

    assert_eq!(opened.review.branch_offer(), None);
    assert!(!opened.review.can_open_pr());
}

#[test]
fn open_pr_types_pr_it_into_the_change_sets_agent() {
    let feature = Feature::with_work_in_progress();
    let key = feature.subject.to_string();
    let fake = Arc::new(FakeTourAgent::new().with_terminal("t1", Some(&key), AgentStatus::Idle));
    let agents = TourAgents::new(Arc::clone(&fake) as Arc<dyn TourAgentPort>);

    let outcome = agents.ask_to_open_pr(&feature.subject);

    assert_eq!(outcome, OwnAgentHandOff::Delivered(TerminalId::new("t1")));
    assert!(fake.calls().contains(&TourAgentCall::Send {
        terminal: TerminalId::new("t1"),
        text: "pr it".to_owned(),
    }));
    assert_eq!(
        outcome.open_pr_toast(),
        (
            ToastTone::Success,
            "Asked your agent t1 to open the PR".to_owned()
        )
    );
}

#[test]
fn open_pr_with_no_agent_in_the_folder_starts_one_that_opens_the_pr() {
    let feature = Feature::with_work_in_progress();
    let fake = Arc::new(FakeTourAgent::new());
    let agents = TourAgents::new(Arc::clone(&fake) as Arc<dyn TourAgentPort>);

    let outcome = agents.ask_to_open_pr(&feature.subject);

    let OwnAgentHandOff::Started(terminal) = &outcome else {
        panic!("{outcome:?}");
    };
    assert_eq!(
        outcome.open_pr_toast(),
        (
            ToastTone::Success,
            format!("Started an agent {terminal} to open the PR")
        )
    );
}

#[test]
fn open_pr_that_reaches_no_agent_says_why_and_stays_until_dismissed() {
    let (tone, line) = OwnAgentHandOff::PickAnAgent.open_pr_toast();

    assert_eq!(tone, ToastTone::Danger);
    assert!(line.starts_with("Could not ask to open the PR: "), "{line}");
}

#[test]
fn a_local_review_shows_its_diagram_and_summary_with_no_buttons_that_add_to_pr_notes() {
    let feature = Feature::with_work_in_progress().with_guided_tour();
    let mut opened = feature.open();
    let diagram = NoteSubject::StepDiagram(StepId::from("readme"));

    assert_eq!(opened.review.notes_button(&diagram), None);
    assert_eq!(
        opened.review.notes_button(&NoteSubject::ReviewerSummary),
        None
    );
    assert_eq!(
        opened
            .review
            .notes_button(&NoteSubject::SummaryPicture("before.png".into())),
        None
    );

    opened.review.go_to(&StepId::from("readme"));
    let diagram_row = opened
        .review
        .page()
        .rows()
        .iter()
        .find_map(|row| match row {
            PageRow::StepIntro(intro) => intro
                .diagram
                .as_ref()
                .map(|diagram| diagram.notes_button.clone()),
            _ => None,
        });
    assert_eq!(diagram_row, Some(None));
}
