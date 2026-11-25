use std::sync::Arc;

use crate::support::FixtureRepo;
use guided_review::local_git::LocalGit;
use guided_review::pull_request::{PullRequestUrl, RepoName};
use guided_review::review_subject::ReviewSubject;
use guided_review::tour::GuidedTour;
use guided_review::viewer::{
    AfterJob, JobOutcome, JobProgress, Problem, RepinnedTour, ReplacingJob, Review,
    ReviewStateFile, Screen, StartedJob, StepId,
};
use serde_json::json;

const PR: &str = "https://github.com/acme/shop/pull/7";

struct Pull {
    repo: FixtureRepo,
    base: String,
    head: String,
}

impl Pull {
    fn new() -> Self {
        let repo = FixtureRepo::cloned_from("acme/shop");
        repo.write("notes.txt", "line 1\nline 2\n");
        let base = repo.commit("base");
        repo.write("notes.txt", "line 1\nline two\n");
        let head = repo.commit("head");
        Self { repo, base, head }
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
            "title": "Notes",
            "summary": [],
            "steps": [{"id": "notes", "title": "Notes", "why": "because", "pieces": [{"file": "notes.txt"}], "children": []}],
        }))
        .unwrap();
        let pr: PullRequestUrl = PR.parse().unwrap();
        let state_file =
            ReviewStateFile::for_subject(&self.repo.folders(), &ReviewSubject::from(pr));
        Review::open(tour, &self.source(), state_file).unwrap()
    }

    fn built_again(&self) -> RepinnedTour {
        RepinnedTour {
            review: self.open(),
            source: Arc::new(self.source()),
        }
    }
}

fn notes() -> StepId {
    "notes".into()
}

fn failed(reason: &str) -> anyhow::Error {
    anyhow::anyhow!(reason.to_owned())
}

#[test]
fn a_second_job_never_starts_while_one_runs() {
    let pull = Pull::new();
    let mut review = pull.open();

    assert_eq!(
        review.start_job(ReplacingJob::Refresh),
        Some(StartedJob::Refresh)
    );

    assert_eq!(review.start_job(ReplacingJob::Repin), None);
    assert_eq!(review.start_job(ReplacingJob::Refresh), None);
    assert_eq!(review.start_job(ReplacingJob::SwitchToGuided), None);
    assert_eq!(review.job_progress(ReplacingJob::Repin), JobProgress::Idle);
}

#[test]
fn refresh_while_an_update_runs_does_nothing() {
    let pull = Pull::new();
    let mut review = pull.open();
    review.start_job(ReplacingJob::Repin).unwrap();

    assert_eq!(review.start_job(ReplacingJob::Refresh), None);

    assert_eq!(
        review.job_progress(ReplacingJob::Refresh),
        JobProgress::Idle
    );
    assert!(review.job_is_running(ReplacingJob::Repin));
}

#[test]
fn a_failed_update_says_why_offers_try_again_and_the_review_takes_changes_again() {
    let pull = Pull::new();
    let mut review = pull.open();
    review.start_job(ReplacingJob::Repin).unwrap();
    assert_eq!(
        review.job_progress(ReplacingJob::Repin),
        JobProgress::Running { activity: None }
    );
    assert!(review.tick(&notes()).is_err());

    let after = review.land_job(JobOutcome::Repin(Err(failed("GitHub did not answer"))));

    assert!(matches!(after, AfterJob::Stays { toast: None }));
    assert_eq!(
        review.job_progress(ReplacingJob::Repin),
        JobProgress::Failed("GitHub did not answer".into())
    );
    review.tick(&notes()).unwrap();
    assert!(pull.open().is_ticked(&notes()));

    review.start_job(ReplacingJob::Repin).unwrap();
    assert_eq!(
        review.job_progress(ReplacingJob::Repin),
        JobProgress::Running { activity: None }
    );
}

#[test]
fn after_an_update_the_built_review_takes_the_place_of_this_one_on_the_screen_shown() {
    let pull = Pull::new();
    let mut review = pull.open();
    review.go_forward();
    review.start_job(ReplacingJob::Repin).unwrap();

    let after = review.land_job(JobOutcome::Repin(Ok(pull.built_again())));

    let AfterJob::Reopen { screen, .. } = after else {
        panic!("the review is not replaced");
    };
    assert_eq!(screen, Screen::Step(notes()));
    assert_eq!(review.job_progress(ReplacingJob::Repin), JobProgress::Idle);
}

#[test]
fn after_the_switch_the_guided_review_opens_on_its_summary() {
    let pull = Pull::new();
    let mut review = pull.open();
    review.go_forward();
    review.start_job(ReplacingJob::SwitchToGuided).unwrap();

    let after = review.land_job(JobOutcome::SwitchToGuided(Ok(pull.built_again())));

    let AfterJob::Reopen { screen, .. } = after else {
        panic!("the review is not replaced");
    };
    assert_eq!(screen, Screen::Summary);
}

#[test]
fn a_failed_switch_is_told_in_a_toast_and_lets_another_job_start() {
    let pull = Pull::new();
    let mut review = pull.open();
    review.start_job(ReplacingJob::SwitchToGuided).unwrap();

    let after = review.land_job(JobOutcome::SwitchToGuided(Err(failed("no tour"))));

    let AfterJob::Stays { toast: Some(toast) } = after else {
        panic!("nothing is told");
    };
    assert_eq!(toast, "Could not switch to the guided tour: no tour");
    assert!(review.start_job(ReplacingJob::Repin).is_some());
}

#[test]
fn a_failed_refresh_is_a_problem_and_its_banner_offers_try_again() {
    let pull = Pull::new();
    let mut review = pull.open();
    review.start_job(ReplacingJob::Refresh).unwrap();
    review.job_did(ReplacingJob::Refresh, "Read app/Pricing.php".to_owned());
    assert_eq!(
        review.job_progress(ReplacingJob::Refresh),
        JobProgress::Running {
            activity: Some("Read app/Pricing.php".into())
        }
    );

    let after = review.land_job(JobOutcome::Refresh(Err(failed("claude stopped"))));

    assert!(matches!(after, AfterJob::Stays { toast: None }));
    assert_eq!(
        review.problems(),
        [Problem::TourNotRefreshed("claude stopped".into())]
    );
    assert_eq!(
        review.job_progress(ReplacingJob::Refresh),
        JobProgress::Failed("claude stopped".into())
    );
}

#[test]
fn what_a_job_did_is_kept_only_while_it_runs() {
    let pull = Pull::new();
    let mut review = pull.open();
    review.job_did(ReplacingJob::Refresh, "Read app/Pricing.php".to_owned());
    review.start_job(ReplacingJob::Refresh).unwrap();
    review.job_did(ReplacingJob::Repin, "Read app/Cart.php".to_owned());

    assert_eq!(
        review.job_progress(ReplacingJob::Refresh),
        JobProgress::Running { activity: None }
    );
}

#[test]
fn a_tour_that_could_not_be_read_again_is_a_problem() {
    let pull = Pull::new();
    let mut review = pull.open();
    review.start_job(ReplacingJob::Reload).unwrap();

    let after = review.land_job(JobOutcome::Reload(Err(failed("no tour file"))));

    assert!(matches!(after, AfterJob::Stays { toast: None }));
    assert_eq!(
        review.problems(),
        [Problem::TourNotReloaded("no tour file".into())]
    );
}

#[test]
fn a_reload_asked_for_while_an_update_runs_waits_and_runs_once_the_update_failed() {
    let pull = Pull::new();
    let mut review = pull.open();
    review.start_job(ReplacingJob::Repin).unwrap();

    assert_eq!(review.start_job(ReplacingJob::Reload), None);
    assert_eq!(review.due_job(), None, "the update still runs");

    review.land_job(JobOutcome::Repin(Err(failed("offline"))));
    assert_eq!(review.due_job(), Some(StartedJob::Reload));
    assert_eq!(review.due_job(), None, "it runs once");
}

#[test]
fn a_reload_asked_for_while_an_update_runs_is_carried_into_the_updated_review() {
    let pull = Pull::new();
    let mut review = pull.open();
    review.start_job(ReplacingJob::Repin).unwrap();
    review.start_job(ReplacingJob::Reload);

    let AfterJob::Reopen { replacement, .. } =
        review.land_job(JobOutcome::Repin(Ok(pull.built_again())))
    else {
        panic!("the review is not replaced");
    };
    let mut updated = replacement.review;
    updated.carry_on_from(review);

    assert_eq!(updated.due_job(), Some(StartedJob::Reload));
}

#[test]
fn a_failure_carried_into_the_rebuilt_review_still_shows() {
    let pull = Pull::new();
    let mut review = pull.open();
    review.start_job(ReplacingJob::Repin).unwrap();
    review.land_job(JobOutcome::Repin(Err(failed("offline"))));
    review.start_job(ReplacingJob::Reload).unwrap();

    let AfterJob::Reopen { replacement, .. } =
        review.land_job(JobOutcome::Reload(Ok(pull.built_again())))
    else {
        panic!("the review is not replaced");
    };
    let mut reloaded = replacement.review;
    reloaded.carry_on_from(review);

    assert_eq!(
        reloaded.job_progress(ReplacingJob::Repin),
        JobProgress::Failed("offline".into())
    );
    assert!(reloaded.start_job(ReplacingJob::Repin).is_some());
}

#[test]
fn an_outcome_of_a_job_that_is_not_running_changes_nothing() {
    let pull = Pull::new();
    let mut review = pull.open();
    review.start_job(ReplacingJob::Refresh).unwrap();

    let after = review.land_job(JobOutcome::Repin(Err(failed("offline"))));

    assert!(matches!(after, AfterJob::Nothing));
    assert!(review.job_is_running(ReplacingJob::Refresh));
    assert_eq!(review.job_progress(ReplacingJob::Repin), JobProgress::Idle);
}

#[test]
fn a_pr_review_cannot_be_dropped() {
    let pull = Pull::new();
    let mut review = pull.open();

    assert_eq!(review.start_job(ReplacingJob::DropReview), None);
    assert_eq!(review.start_job(ReplacingJob::MoveToPr), None, "no offer");
    review.tick(&notes()).unwrap();
}
