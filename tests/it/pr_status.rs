use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::support::FixtureRepo;
use crate::support::fake_ports::{
    FakeGitHub, GitHubCall, GitHubFault, ReadFailure, SentLog, blocked_merging,
};
use crate::support::work::drive_without_agents;
use guided_review::build::Build;
use guided_review::local_git::LocalGit;
use guided_review::pull_request::RepoName;
use guided_review::tour::GuidedTour;
use guided_review::viewer::{
    CHECKS_SHOWN, CheckRun, CheckState, Checks, Command, Effect, Intent, OpenPrStatus,
    PaletteAction, PrStatus, PressStep, Problem, Read, Readiness, Review, ReviewStateFile,
    WindowMoment,
};
use serde_json::json;

pub(crate) const PR: &str = "https://github.com/acme/shop/pull/7";

/// A one-step PR review whose GitHub is a fake, for the PR status and the keys that change it.
pub(crate) struct Pull {
    repo: FixtureRepo,
    base: String,
    head: String,
    pub(crate) sent: SentLog,
}

impl Pull {
    pub(crate) fn new() -> Self {
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

    pub(crate) fn github(&self) -> FakeGitHub {
        FakeGitHub::with_pull("main-tip", &self.head, &self.sent)
    }

    pub(crate) fn source(&self) -> LocalGit {
        LocalGit::find(&RepoName::new("acme", "shop"), self.repo.path())
            .unwrap()
            .unwrap()
    }

    pub(crate) fn open(&self) -> Review {
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

    pub(crate) fn read(&self, review: &mut Review, github: &FakeGitHub, now: Instant) {
        review.pull_from_github();
        drive_without_agents(review, github, &self.source(), now);
    }
}

fn open_status(readiness: Readiness, checks: Checks) -> OpenPrStatus {
    OpenPrStatus {
        readiness,
        checks,
        check_runs: Arc::default(),
        viewer_may_change_readiness: true,
        merging: blocked_merging(),
    }
}

fn status(readiness: Readiness, checks: Checks) -> PrStatus {
    PrStatus::Open(open_status(readiness, checks))
}

fn open_part(review: &Review) -> Option<&OpenPrStatus> {
    review.pr_status().and_then(PrStatus::open)
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
        open_part(&review).map(|status| status.checks),
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
    let github = pull.github().with_status(PrStatus::Open(OpenPrStatus {
        viewer_may_change_readiness: may_change,
        ..open_status(readiness, Checks::Passing)
    }));
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

    let PressStep::Ask(note) = step else {
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
    assert_eq!(review.ask_to_change_readiness(later), PressStep::Sent);
    drive_without_agents(&mut review, &github, &pull.source(), later);

    assert_eq!(
        readiness_calls(&pull),
        [GitHubCall::SetReadiness {
            pr: PR.parse().unwrap(),
            readiness: Readiness::Draft
        }]
    );
    assert_eq!(
        open_part(&review).map(|status| status.readiness),
        Some(Readiness::Draft)
    );
    assert_eq!(review.problems(), []);
}

#[test]
fn a_draft_is_marked_ready_by_the_same_key() {
    let pull = Pull::new();
    let (mut review, github, now) = read_status(&pull, Readiness::Draft, true);

    let PressStep::Ask(note) = review.ask_to_change_readiness(now) else {
        panic!("not asked");
    };
    assert!(note.contains("mark the PR ready for review"), "{note}");
    review.ask_to_change_readiness(now);
    drive_without_agents(&mut review, &github, &pull.source(), now);

    assert_eq!(
        github.status_now().open().map(|status| status.readiness),
        Some(Readiness::Ready)
    );
}

#[test]
fn nothing_is_asked_where_github_would_refuse_or_the_status_is_not_known() {
    let pull = Pull::new();
    let (mut read_only, _github, now) = read_status(&pull, Readiness::Ready, false);
    let refused = read_only.ask_to_change_readiness(now);
    assert!(
        matches!(&refused, PressStep::Refused(why) if why.contains("write access")),
        "{refused:?}"
    );

    let mut unread = pull.open();
    assert_eq!(unread.ask_to_change_readiness(now), PressStep::Nothing);
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
        open_part(&review).map(|status| status.readiness),
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
fn the_chip_says_draft_or_ready_and_the_description_adds_what_the_checks_roll_up_to() {
    let chip = |readiness, checks| status(readiness, checks);

    assert_eq!(chip(Readiness::Draft, Checks::Failing).label(), "Draft");
    assert_eq!(chip(Readiness::Ready, Checks::Passing).label(), "Ready");
    let description = |readiness, checks| chip(readiness, checks).description();
    assert_eq!(description(Readiness::Draft, Checks::NoneRun), "Draft");
    assert_eq!(description(Readiness::Ready, Checks::NoneRun), "Ready");
    assert_eq!(
        description(Readiness::Ready, Checks::Passing),
        "Ready · checks passing"
    );
    assert_eq!(
        description(Readiness::Draft, Checks::Running),
        "Draft · checks running"
    );
    assert_eq!(
        description(Readiness::Ready, Checks::Failing),
        "Ready · checks failing"
    );
}

fn run(name: &str, state: CheckState, page: Option<&str>) -> CheckRun {
    CheckRun {
        name: name.into(),
        state,
        page: page.map(Into::into),
    }
}

fn status_with_runs(runs: Vec<CheckRun>) -> PrStatus {
    PrStatus::Open(open_with_runs(runs))
}

fn open_with_runs(runs: Vec<CheckRun>) -> OpenPrStatus {
    OpenPrStatus {
        check_runs: runs.into(),
        ..open_status(Readiness::Ready, Checks::Failing)
    }
}

#[test]
fn the_check_list_puts_failing_first_then_running_then_passing_in_the_order_github_gave() {
    let list = open_with_runs(vec![
        run("lint", CheckState::Passing, None),
        run("build", CheckState::Running, None),
        run("test", CheckState::Failing, None),
        run("deploy", CheckState::Passing, None),
        run("e2e", CheckState::Failing, None),
    ])
    .check_list();

    let names: Vec<&str> = list.shown.iter().map(|check| &*check.name).collect();
    assert_eq!(names, ["test", "e2e", "build", "lint", "deploy"]);
    assert_eq!(list.hidden, 0);
}

#[test]
fn the_check_list_puts_neutral_and_then_skipped_checks_after_the_passing_ones() {
    let list = open_with_runs(vec![
        run("docs", CheckState::Skipped, None),
        run("lint", CheckState::Passing, None),
        run("advice", CheckState::Neutral, None),
        run("test", CheckState::Failing, None),
    ])
    .check_list();

    let names: Vec<&str> = list.shown.iter().map(|check| &*check.name).collect();
    assert_eq!(names, ["test", "lint", "advice", "docs"]);
}

#[test]
fn the_check_list_is_capped_and_counts_what_it_left_out_after_the_worst_were_kept() {
    let mut runs: Vec<CheckRun> = (0..CHECKS_SHOWN + 5)
        .map(|index| run(&format!("ok-{index}"), CheckState::Passing, None))
        .collect();
    runs.push(run("broken", CheckState::Failing, None));

    let list = open_with_runs(runs).check_list();

    assert_eq!(list.shown.len(), CHECKS_SHOWN);
    assert_eq!(&*list.shown[0].name, "broken");
    assert_eq!(list.hidden, 6);
}

#[test]
fn a_check_run_is_running_until_it_completed_and_then_passes_or_fails_by_its_conclusion() {
    let of = CheckState::of_check_run;

    assert_eq!(of("IN_PROGRESS", None), CheckState::Running);
    assert_eq!(of("QUEUED", None), CheckState::Running);
    assert_eq!(of("COMPLETED", Some("SUCCESS")), CheckState::Passing);
    assert_eq!(of("COMPLETED", Some("SKIPPED")), CheckState::Skipped);
    assert_eq!(of("COMPLETED", Some("NEUTRAL")), CheckState::Neutral);
    assert_eq!(of("COMPLETED", Some("FAILURE")), CheckState::Failing);
    assert_eq!(of("COMPLETED", Some("TIMED_OUT")), CheckState::Failing);
    assert_eq!(of("COMPLETED", Some("CANCELLED")), CheckState::Failing);
    assert_eq!(
        CheckState::of_status_context("SUCCESS"),
        CheckState::Passing
    );
    assert_eq!(CheckState::of_status_context("ERROR"), CheckState::Failing);
    assert_eq!(
        CheckState::of_status_context("PENDING"),
        CheckState::Running
    );
}

#[test]
fn clicking_a_check_opens_its_run_and_one_without_a_page_or_the_more_row_opens_the_checks_page() {
    let pull = Pull::new();
    let github = pull.github().with_status(status_with_runs(vec![
        run(
            "lint",
            CheckState::Passing,
            Some("https://ci.example/lint/1"),
        ),
        run(
            "test",
            CheckState::Failing,
            Some("https://ci.example/test/2"),
        ),
        run("legacy", CheckState::Passing, None),
    ]));
    let mut review = pull.open();
    pull.read(&mut review, &github, Instant::now());
    let click = |index| review.decide(Intent::OpenCheck(index), WindowMoment::default());
    let checks_page = Effect::OpenChecksOnGitHub(PR.parse().unwrap());

    assert_eq!(
        click(0),
        Effect::OpenCheck("https://ci.example/test/2".to_owned())
    );
    assert_eq!(
        click(1),
        Effect::OpenCheck("https://ci.example/lint/1".to_owned())
    );
    assert_eq!(click(2), checks_page);
    assert_eq!(click(3), checks_page);
}
