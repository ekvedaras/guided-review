use std::time::Instant;

use crate::support::FixtureRepo;
use crate::support::fake_ports::{FakeGitHub, SentLog};
use crate::support::work::drive_without_agents;
use guided_review::local_git::LocalGit;
use guided_review::pull_request::RepoName;
use guided_review::tour::GuidedTour;
use guided_review::viewer::{Checks, PrStatus, Readiness, Review, ReviewStateFile};
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
