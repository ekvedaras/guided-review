//! A local review finds the PR opened from its branch and offers to move to it.

use std::time::{Duration, Instant};

use crate::support::fake_ports::{FakeGitHub, GitHubFault, SentLog};
use crate::support::work::drive_without_agents;
use crate::support::{FixtureRepo, serve_files};
use guided_review::github::GitHub;
use guided_review::local_work_tree::LocalWorkTree;
use guided_review::open_review::{OpenedReview, open_review};
use guided_review::pull_request::{PullRequestUrl, RepoName};
use guided_review::review_subject::{BranchName, ReviewSubject};
use guided_review::viewer::{
    MoveOffer, PULL_OF_BRANCH_RETRY, PullOfBranch, PullsOfBranchPort,
};

const SHOP_7: &str = "https://github.com/acme/shop/pull/7";

/// `feature` one commit past `main` that adds the cart, with the README's second line written and not
/// committed.
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

    fn github(&self) -> FakeGitHub {
        FakeGitHub::with_pull("main-tip", "snapshot", &self.sent)
    }

    fn open(&self) -> OpenedReview {
        open_review(&self.subject, self.repo.path(), &self.repo.folders()).unwrap()
    }

    /// The window comes to the front, and what the review wants of GitHub is done.
    fn focus(&self, opened: &mut OpenedReview, github: &FakeGitHub, now: Instant) {
        opened.review.pull_from_github();
        drive_without_agents(&mut opened.review, github, opened.source.as_ref(), now);
    }
}

fn offer_of(pr: &str, title: &str) -> MoveOffer {
    MoveOffer {
        pr: pr.parse().unwrap(),
        title: title.to_owned(),
    }
}

#[test]
fn a_local_review_offers_to_move_to_the_open_pr_whose_head_is_its_branch() {
    let feature = Feature::with_work_in_progress();
    let github = feature
        .github()
        .with_open_pull_from("feature", SHOP_7, "Add the cart");
    let mut opened = feature.open();
    assert_eq!(opened.review.move_offer(), None);

    feature.focus(&mut opened, &github, Instant::now());

    let offer = opened.review.move_offer().unwrap();
    assert_eq!(offer, offer_of(SHOP_7, "Add the cart"));
    assert_eq!(offer.label(), "Move to acme/shop#7");
    assert_eq!(feature.sent.branch_lookups(), ["acme/shop:feature"]);
}

#[test]
fn there_is_no_offer_without_a_pr_from_the_branch_or_with_one_from_another_branch() {
    let feature = Feature::with_work_in_progress();
    let github = feature
        .github()
        .with_open_pull_from("another", SHOP_7, "Another change");
    let mut opened = feature.open();

    feature.focus(&mut opened, &github, Instant::now());

    assert_eq!(opened.review.move_offer(), None);
    assert_eq!(feature.sent.branch_lookups(), ["acme/shop:feature"]);
    assert_eq!(feature.sent.github_calls(), []);
}

#[test]
fn a_look_that_fails_is_silent_and_tried_again_later() {
    let feature = Feature::with_work_in_progress();
    let github = feature
        .github()
        .with_open_pull_from("feature", SHOP_7, "Add the cart");
    github.inject(GitHubFault::ReadsFail);
    let mut opened = feature.open();
    let now = Instant::now();

    feature.focus(&mut opened, &github, now);

    assert_eq!(opened.review.move_offer(), None);
    assert_eq!(opened.review.problems(), []);
    github.heal(&GitHubFault::ReadsFail);

    drive_without_agents(
        &mut opened.review,
        &github,
        opened.source.as_ref(),
        now + PULL_OF_BRANCH_RETRY + Duration::from_secs(1),
    );

    assert_eq!(
        opened.review.move_offer(),
        Some(offer_of(SHOP_7, "Add the cart"))
    );
}

#[test]
fn a_pr_review_looks_for_no_branch() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    let sent = SentLog::default();
    let github = FakeGitHub::with_pull("main-tip", "head", &sent).with_open_pull_from(
        "feature",
        SHOP_7,
        "Add the cart",
    );
    repo.write("a.txt", "one\n");
    let base = repo.commit("base");
    repo.write("a.txt", "two\n");
    let head = repo.commit("head");
    let pr: PullRequestUrl = SHOP_7.parse().unwrap();
    let tour = guided_review::tour::BasicTour::new(
        serde_json::from_value(serde_json::json!({
            "pr": SHOP_7, "base": base, "head": head,
        }))
        .unwrap(),
        "Orders cost two".to_owned(),
    );
    let source = guided_review::local_git::LocalGit::find(&pr.repo, repo.path())
        .unwrap()
        .unwrap();
    let state_file = guided_review::viewer::ReviewStateFile::for_subject(
        &repo.folders(),
        &ReviewSubject::from(pr),
    );
    let mut review = guided_review::viewer::Review::open_basic(tour, &source, state_file).unwrap();

    review.pull_from_github();
    drive_without_agents(&mut review, &github, &source, Instant::now());

    assert_eq!(review.move_offer(), None);
    assert_eq!(sent.branch_lookups(), Vec::<String>::new());
}

#[test]
fn the_github_adapter_lists_the_open_pr_of_a_branch_of_the_repo_itself() {
    let listed = r#"[{"number":7,"title":"Add the cart","html_url":"https://github.com/acme/shop/pull/7"}]"#;
    let url = serve_files(vec![
        (
            "/repos/acme/shop/pulls?head=acme%3Afeature/cart&state=open",
            listed.as_bytes().to_vec(),
        ),
        (
            "/repos/acme/shop/pulls?head=acme%3Aquiet&state=open",
            b"[]".to_vec(),
        ),
    ]);
    let github = GitHub::without_token_at(&url);
    let repo = RepoName::new("acme", "shop");

    let found = github
        .open_pull_of_branch(&repo, &BranchName::new("feature/cart").unwrap())
        .unwrap();
    let none = github
        .open_pull_of_branch(&repo, &BranchName::new("quiet").unwrap())
        .unwrap();

    assert_eq!(
        found,
        Some(PullOfBranch {
            pr: SHOP_7.parse().unwrap(),
            title: "Add the cart".to_owned(),
        })
    );
    assert_eq!(none, None);
}
