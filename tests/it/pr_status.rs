use std::time::{Duration, Instant};

use crate::support::FixtureRepo;
use crate::support::fake_ports::{FakeGitHub, GitHubCall, GitHubFault, ReadFailure, SentLog};
use crate::support::work::drive_without_agents;
use guided_review::local_git::LocalGit;
use guided_review::pull_request::RepoName;
use guided_review::tour::GuidedTour;
use guided_review::update::Build;
use guided_review::viewer::{
    Checks, Command, Effect, Intent, PaletteAction, PrStatus, Problem, Read, Readiness,
    ReadinessStep, Review, ReviewStateFile, WindowMoment,
};
use serde_json::json;

const PR: &str = "https://github.com/acme/shop/pull/7";

struct Pull {
    repo: FixtureRepo,
    base: String,
    head: String,
    sent: SentLog,
}

impl Pull {
    fn new() -> Self {
        let repo = FixtureRepo::cloned_from("acme/shop");
        repo.write("a.txt", "one\n");
        let base = repo.commit("base");
        repo.write("a.txt", "two\n");
        let head = repo.commit("head");
        Self {
            repo,
            base,
            head,
            sent: SentLog::default(),
        }
    }

    fn github(&self) -> FakeGitHub {
        FakeGitHub::with_pull("main-tip", &self.head, &self.sent)
    }

    fn source(&self) -> LocalGit {
        LocalGit::find(&RepoName::new("acme", "shop"), self.repo.path())
            .unwrap()
            .unwrap()
    }

    fn open(&self) -> Review {
        let tour: GuidedTour = serde_json::from_value(json!({
            "pr": PR,
            "base": self.base,
            "head": self.head,
            "title": "Letters",
            "summary": [],
            "steps": [
                {"id": "a", "title": "A", "why": "because", "pieces": [{"file": "a.txt"}], "children": []},
            ],
        }))
        .unwrap();
        let state_file = ReviewStateFile::for_subject(&self.repo.folders(), &PR.parse().unwrap());
        Review::open(tour, &self.source(), state_file).unwrap()
    }

    fn read(&self, review: &mut Review, github: &FakeGitHub, now: Instant) {
        review.pull_from_github();
        drive_without_agents(review, github, &self.source(), now);
    }
}

fn status(readiness: Readiness, checks: Checks) -> PrStatus {
    PrStatus {
        readiness,
        checks,
        viewer_may_change_readiness: true,
    }
}

#[test]
fn the_status_is_unknown_until_github_is_read_and_then_what_github_says() {
    let pull = Pull::new();
    let mut review = pull.open();
    let github = pull
        .github()
        .with_status(status(Readiness::Draft, Checks::Failing));
    assert_eq!(review.pr_status(), None);

    pull.read(&mut review, &github, Instant::now());

    assert_eq!(
        review.pr_status(),
        Some(&status(Readiness::Draft, Checks::Failing))
    );
}

#[test]
fn a_rollup_state_is_one_of_four_words() {
    assert_eq!(Checks::from_github_state(None), Checks::NoneRun);
    assert_eq!(Checks::from_github_state(Some("SUCCESS")), Checks::Passing);
    assert_eq!(Checks::from_github_state(Some("FAILURE")), Checks::Failing);
    assert_eq!(Checks::from_github_state(Some("ERROR")), Checks::Failing);
    assert_eq!(Checks::from_github_state(Some("PENDING")), Checks::Running);
    assert_eq!(Checks::from_github_state(Some("EXPECTED")), Checks::Running);
}

fn failing(pull: &Pull, failure: ReadFailure) -> FakeGitHub {
    let github = pull.github();
    github.inject(GitHubFault::ReadsFailWith(failure));
    github
}

#[test]
fn a_status_that_cannot_be_read_while_github_does_not_answer_is_one_more_thing_that_may_be_old() {
    let pull = Pull::new();
    let mut review = pull.open();
    let github = failing(&pull, ReadFailure::Status(503));

    pull.read(&mut review, &github, Instant::now());

    let problems = review.problems();
    assert_eq!(problems.len(), 1, "{problems:?}");
    let detail = problems[0].detail().unwrap();
    assert!(
        detail.contains("whether the PR is a draft and its checks"),
        "{detail}"
    );
}

#[test]
fn the_status_last_read_stays_while_github_does_not_answer_and_is_read_again_when_it_does() {
    let pull = Pull::new();
    let mut review = pull.open();
    let github = pull
        .github()
        .with_status(status(Readiness::Draft, Checks::Passing));
    let now = Instant::now();
    pull.read(&mut review, &github, now);
    github.inject(GitHubFault::ReadsFailWith(ReadFailure::Status(503)));

    pull.read(&mut review, &github, now + Duration::from_secs(1));
    assert_eq!(
        review.pr_status(),
        Some(&status(Readiness::Draft, Checks::Passing))
    );
    assert_eq!(review.problems().len(), 1);

    github.heal(&GitHubFault::ReadsFailWith(ReadFailure::Status(503)));
    github.set_status(status(Readiness::Ready, Checks::Passing));
    drive_without_agents(
        &mut review,
        &github,
        &pull.source(),
        now + Duration::from_secs(40),
    );

    assert_eq!(
        review.pr_status(),
        Some(&status(Readiness::Ready, Checks::Passing))
    );
    assert_eq!(review.problems(), []);
}

#[test]
fn a_refusal_to_read_the_status_is_a_problem_of_its_own_that_is_not_tried_again() {
    let pull = Pull::new();
    let mut review = pull.open();
    let github = failing(&pull, ReadFailure::Status(404));

    pull.read(&mut review, &github, Instant::now());

    assert!(
        review.problems().contains(&Problem::NotRead(
            Read::PrStatus,
            "http status: 404".to_owned()
        )),
        "{:?}",
        review.problems()
    );
    assert_eq!(review.next_due(), None);
}

#[test]
fn checks_that_are_running_are_read_again_every_30_seconds_until_they_are_done() {
    let pull = Pull::new();
    let mut review = pull.open();
    let github = pull
        .github()
        .with_status(status(Readiness::Ready, Checks::Running));
    let mut now = Instant::now();
    pull.read(&mut review, &github, now);

    let mut waits = Vec::new();
    for _ in 0..2 {
        let due = review.next_due().unwrap();
        waits.push(due - now);
        now = due;
        drive_without_agents(&mut review, &github, &pull.source(), now);
    }
    github.set_status(status(Readiness::Ready, Checks::Failing));
    let due = review.next_due().unwrap();
    drive_without_agents(&mut review, &github, &pull.source(), due);

    assert_eq!(
        waits.iter().map(Duration::as_secs).collect::<Vec<_>>(),
        [30, 30]
    );
    assert_eq!(
        review.pr_status().map(|status| status.checks),
        Some(Checks::Failing)
    );
    assert_eq!(review.next_due(), None);
}

#[test]
fn checks_that_are_not_running_are_not_read_again_by_themselves() {
    for checks in [Checks::NoneRun, Checks::Passing, Checks::Failing] {
        let pull = Pull::new();
        let mut review = pull.open();
        let github = pull.github().with_status(status(Readiness::Ready, checks));

        pull.read(&mut review, &github, Instant::now());

        assert_eq!(review.next_due(), None, "{checks:?}");
    }
}

fn read_status(
    pull: &Pull,
    readiness: Readiness,
    may_change: bool,
) -> (Review, FakeGitHub, Instant) {
    let mut review = pull.open();
    let github = pull.github().with_status(PrStatus {
        readiness,
        checks: Checks::Passing,
        viewer_may_change_readiness: may_change,
    });
    let now = Instant::now();
    pull.read(&mut review, &github, now);
    (review, github, now)
}

fn readiness_calls(pull: &Pull) -> Vec<GitHubCall> {
    pull.sent
        .github_calls()
        .into_iter()
        .filter(|call| matches!(call, GitHubCall::SetReadiness { .. }))
        .collect()
}

#[test]
fn the_first_press_says_what_pressing_again_does_and_changes_nothing() {
    let pull = Pull::new();
    let (mut review, github, now) = read_status(&pull, Readiness::Ready, true);

    let step = review.ask_to_change_readiness(now);

    let ReadinessStep::Ask(note) = step else {
        panic!("{step:?}");
    };
    assert!(note.contains("convert the PR to a draft"), "{note}");
    assert!(note.contains("again within 10 s"), "{note}");
    assert!(note.contains(" D "), "{note}");
    drive_without_agents(&mut review, &github, &pull.source(), now);
    assert_eq!(readiness_calls(&pull), []);
}

#[test]
fn the_same_press_again_within_ten_seconds_converts_a_ready_pr_to_a_draft() {
    let pull = Pull::new();
    let (mut review, github, now) = read_status(&pull, Readiness::Ready, true);
    review.ask_to_change_readiness(now);

    let later = now + Duration::from_secs(10);
    assert_eq!(review.ask_to_change_readiness(later), ReadinessStep::Sent);
    drive_without_agents(&mut review, &github, &pull.source(), later);

    assert_eq!(
        readiness_calls(&pull),
        [GitHubCall::SetReadiness {
            pr: PR.parse().unwrap(),
            readiness: Readiness::Draft
        }]
    );
    assert_eq!(
        review.pr_status().map(|status| status.readiness),
        Some(Readiness::Draft)
    );
    assert_eq!(review.problems(), []);
}

#[test]
fn a_draft_is_marked_ready_by_the_same_key() {
    let pull = Pull::new();
    let (mut review, github, now) = read_status(&pull, Readiness::Draft, true);

    let ReadinessStep::Ask(note) = review.ask_to_change_readiness(now) else {
        panic!("not asked");
    };
    assert!(note.contains("mark the PR ready for review"), "{note}");
    review.ask_to_change_readiness(now);
    drive_without_agents(&mut review, &github, &pull.source(), now);

    assert_eq!(github.status_now().readiness, Readiness::Ready);
}

#[test]
fn a_press_after_the_ten_seconds_only_asks_again() {
    let pull = Pull::new();
    let (mut review, github, now) = read_status(&pull, Readiness::Ready, true);
    review.ask_to_change_readiness(now);

    let step = review.ask_to_change_readiness(now + Duration::from_secs(11));

    assert!(matches!(step, ReadinessStep::Ask(_)), "{step:?}");
    drive_without_agents(&mut review, &github, &pull.source(), now);
    assert_eq!(readiness_calls(&pull), []);
}

#[test]
fn nothing_is_asked_where_github_would_refuse_or_the_status_is_not_known() {
    let pull = Pull::new();
    let (mut read_only, _github, now) = read_status(&pull, Readiness::Ready, false);
    assert_eq!(
        read_only.ask_to_change_readiness(now),
        ReadinessStep::NotAllowed
    );

    let mut unread = pull.open();
    assert_eq!(unread.ask_to_change_readiness(now), ReadinessStep::Nothing);
}

#[test]
fn a_change_that_github_does_not_take_is_a_problem_and_leaves_the_status_as_it_was() {
    let pull = Pull::new();
    let (mut review, github, now) = read_status(&pull, Readiness::Ready, true);
    github.inject(GitHubFault::ConnectionLost);
    review.ask_to_change_readiness(now);
    review.ask_to_change_readiness(now);

    drive_without_agents(&mut review, &github, &pull.source(), now);

    assert_eq!(
        review.pr_status().map(|status| status.readiness),
        Some(Readiness::Ready)
    );
    assert_eq!(
        review.problems(),
        [Problem::ReadinessNotChanged(
            Readiness::Draft,
            "network is down".to_owned()
        )]
    );
}

#[test]
fn the_palette_offers_the_change_that_applies_and_only_where_the_viewer_may_make_it() {
    let offered = |review: &Review| -> Vec<&'static str> {
        review
            .palette_actions(Build::Local)
            .into_iter()
            .map(PaletteAction::label)
            .filter(|label| ["Convert to draft", "Mark as ready for review"].contains(label))
            .collect()
    };
    let pull = Pull::new();

    let (ready, ..) = read_status(&pull, Readiness::Ready, true);
    let (draft, ..) = read_status(&pull, Readiness::Draft, true);
    let (read_only, ..) = read_status(&pull, Readiness::Ready, false);

    assert_eq!(offered(&ready), ["Convert to draft"]);
    assert_eq!(offered(&draft), ["Mark as ready for review"]);
    assert_eq!(offered(&read_only), [] as [&str; 0]);
    assert_eq!(offered(&pull.open()), [] as [&str; 0]);
}

#[test]
fn the_palette_always_offers_the_checks_page_of_a_pr() {
    let pull = Pull::new();
    let review = pull.open();

    assert!(
        review
            .palette_actions(Build::Local)
            .contains(&PaletteAction::OpenChecksOnGitHub)
    );
    assert_eq!(
        review.decide(Intent::OpenChecksOnGitHub, WindowMoment::default()),
        Effect::OpenChecksOnGitHub(PR.parse().unwrap())
    );
}

#[test]
fn the_key_and_the_palette_ask_for_the_same_thing() {
    let pull = Pull::new();
    let review = pull.open();

    assert_eq!(
        review.decide(Intent::ToggleDraft, WindowMoment::default()),
        Effect::ChangeReadiness
    );
    for action in [
        PaletteAction::ConvertToDraft,
        PaletteAction::MarkReadyForReview,
    ] {
        assert_eq!(Intent::from(action), Intent::ToggleDraft);
        assert_eq!(action.command(), Some(Command::ToggleDraft));
    }
    assert_eq!(Command::ToggleDraft.default_chords()[0].keystroke(), "d");
}

#[test]
fn the_label_says_draft_or_ready_and_what_the_checks_roll_up_to() {
    let label = |readiness, checks| status(readiness, checks).label();

    assert_eq!(label(Readiness::Draft, Checks::NoneRun), "Draft");
    assert_eq!(label(Readiness::Ready, Checks::NoneRun), "Ready");
    assert_eq!(
        label(Readiness::Ready, Checks::Passing),
        "Ready · checks passing"
    );
    assert_eq!(
        label(Readiness::Draft, Checks::Running),
        "Draft · checks running"
    );
    assert_eq!(
        label(Readiness::Ready, Checks::Failing),
        "Ready · checks failing"
    );
}
