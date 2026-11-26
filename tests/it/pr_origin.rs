//! The title bar names who opened a PR and the branch it merges into; a local review names its own branch
//! and the repo's default one.

use std::time::Instant;

use crate::support::fake_ports::{FakeHost, HostFault, SentLog};
use crate::support::work::drive_without_agents;
use crate::support::{FixtureRepo, FixtureRepo as Repo};
use guided_review::local_git::LocalGit;
use guided_review::local_work_tree::LocalWorkTree;
use guided_review::open_review::open_review;
use guided_review::review_subject::{BranchName, ReviewSubject};
use guided_review::tour::GuidedTour;
use guided_review::viewer::{
    HeadBranch, MergeRoute, PrAuthor, PullRequestOrigin, Review, ReviewStateFile,
};
use serde_json::json;

const PR: &str = "https://github.com/acme/shop/pull/7";

fn branch(name: &str) -> BranchName {
    BranchName::new(name).unwrap()
}

fn ada() -> PrAuthor {
    PrAuthor {
        login: "ada".into(),
        avatar_url: Some("https://avatars.example/ada.png".into()),
    }
}

fn origin(base: &str, head: HeadBranch) -> PullRequestOrigin {
    PullRequestOrigin {
        author: Some(ada()),
        route: MergeRoute {
            base: branch(base),
            head,
        },
    }
}

struct Shop {
    repo: Repo,
    head: String,
    sent: SentLog,
}

impl Shop {
    fn new() -> Self {
        let repo = FixtureRepo::cloned_from("acme/shop");
        repo.write("a.txt", "one\n");
        repo.commit("base");
        repo.write("a.txt", "two\n");
        let head = repo.commit("head");
        Self {
            repo,
            head,
            sent: SentLog::default(),
        }
    }

    fn source(&self) -> LocalGit {
        LocalGit::find(&crate::support::shop_on_github(), self.repo.path())
            .unwrap()
            .unwrap()
    }

    fn open(&self) -> Review {
        let tour: GuidedTour = serde_json::from_value(json!({
            "pr": PR,
            "base": self.head,
            "head": self.head,
            "title": "Letters",
            "summary": [],
            "steps": [{"id": "a", "title": "A", "why": "w", "pieces": [{"file": "a.txt"}]}],
        }))
        .unwrap();
        let state_file = ReviewStateFile::for_subject(&self.repo.folders(), &PR.parse().unwrap());
        Review::open(
            tour,
            &self.source(),
            state_file,
            crate::support::fake_ports::every_capability(),
        )
        .unwrap()
    }

    fn host(&self, origin: PullRequestOrigin) -> FakeHost {
        FakeHost::with_pull("main-tip", &self.head, &self.sent).with_origin(origin)
    }

    fn read(&self, review: &mut Review, host: &FakeHost) {
        review.pull_from_host();
        drive_without_agents(review, host, &self.source(), Instant::now());
    }
}

#[test]
fn a_pr_names_its_author_and_branches_once_github_answers() {
    let shop = Shop::new();
    let mut review = shop.open();
    let host = shop.host(origin("main", HeadBranch::of_repo(branch("feat/cart"))));

    assert_eq!(review.merge_route(), None);
    assert_eq!(review.pr_author(), None);

    shop.read(&mut review, &host);

    let route = review.merge_route().unwrap();
    assert_eq!(route.base, branch("main"));
    assert_eq!(route.head.to_string(), "feat/cart");
    assert_eq!(review.pr_author(), Some(&ada()));
    assert_eq!(
        review.merge_route_sentence().as_deref(),
        Some("ada wants to merge feat/cart into main")
    );
}

#[test]
fn a_pr_from_a_fork_writes_its_head_branch_as_owner_and_branch() {
    let shop = Shop::new();
    let mut review = shop.open();
    let host = shop.host(origin("main", HeadBranch::of_fork("grace", branch("fix"))));

    shop.read(&mut review, &host);

    assert_eq!(review.merge_route().unwrap().head.to_string(), "grace:fix");
    assert_eq!(
        review.merge_route_sentence().as_deref(),
        Some("ada wants to merge grace:fix into main")
    );
}

#[test]
fn a_pr_of_a_deleted_user_has_a_route_and_no_author() {
    let shop = Shop::new();
    let mut review = shop.open();
    let mut ghost = origin("main", HeadBranch::of_repo(branch("feat/cart")));
    ghost.author = None;
    let host = shop.host(ghost);

    shop.read(&mut review, &host);

    assert_eq!(review.pr_author(), None);
    assert_eq!(
        review.merge_route_sentence().as_deref(),
        Some("Merges feat/cart into main")
    );
}

#[test]
fn a_pr_retargeted_on_github_shows_its_new_base_when_the_window_comes_to_the_front() {
    let shop = Shop::new();
    let mut review = shop.open();
    let host = shop.host(origin("main", HeadBranch::of_repo(branch("feat/cart"))));
    shop.read(&mut review, &host);

    host.set_origin(origin(
        "release/2",
        HeadBranch::of_repo(branch("feat/cart")),
    ));
    shop.read(&mut review, &host);

    assert_eq!(review.merge_route().unwrap().base, branch("release/2"));
}

#[test]
fn a_failed_read_keeps_the_last_answer() {
    let shop = Shop::new();
    let mut review = shop.open();
    let host = shop.host(origin("main", HeadBranch::of_repo(branch("feat/cart"))));
    shop.read(&mut review, &host);

    host.inject(HostFault::ReadsAnswerAnError);
    shop.read(&mut review, &host);

    assert_eq!(review.merge_route().unwrap().base, branch("main"));
    assert_eq!(review.pr_author(), Some(&ada()));
}

#[test]
fn a_review_that_replaces_another_goes_on_with_what_it_knew() {
    let shop = Shop::new();
    let mut replaced = shop.open();
    let host = shop.host(origin("main", HeadBranch::of_repo(branch("feat/cart"))));
    shop.read(&mut replaced, &host);

    let mut review = shop.open();
    review.carry_on_from(replaced);

    assert_eq!(review.merge_route().unwrap().base, branch("main"));
    assert_eq!(review.pr_author(), Some(&ada()));
}

#[test]
fn a_local_review_goes_from_its_branch_into_the_default_branch_with_no_author() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    repo.write("README.md", "hello\n");
    repo.commit("Start the shop");
    repo.git(&["checkout", "-q", "-b", "feature"]);
    repo.write("src/cart.rs", "pub fn total() {}\n");
    repo.commit("Add the cart");
    let work_tree = LocalWorkTree::containing(repo.path()).unwrap();
    let subject = ReviewSubject::from(work_tree.change_set().unwrap());

    let opened = open_review(&subject, repo.path(), &repo.folders()).unwrap();

    let route = opened.review.merge_route().unwrap();
    assert_eq!(route.base, branch("main"));
    assert_eq!(route.head, HeadBranch::of_repo(branch("feature")));
    assert_eq!(opened.review.pr_author(), None);
    assert_eq!(
        opened.review.merge_route_sentence().as_deref(),
        Some("Reviewing feature against main")
    );
}

#[test]
fn a_local_review_compared_with_origin_still_merges_into_the_branch_by_its_own_name() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    repo.write("README.md", "hello\n");
    repo.commit("Start the shop");
    repo.git(&["update-ref", "refs/remotes/origin/main", "HEAD"]);
    repo.git(&[
        "symbolic-ref",
        "refs/remotes/origin/HEAD",
        "refs/remotes/origin/main",
    ]);
    repo.git(&["checkout", "-q", "-b", "feature"]);
    repo.write("src/cart.rs", "pub fn total() {}\n");
    repo.commit("Add the cart");
    let work_tree = LocalWorkTree::containing(repo.path()).unwrap();
    let subject = ReviewSubject::from(work_tree.change_set().unwrap());

    let opened = open_review(&subject, repo.path(), &repo.folders()).unwrap();

    assert_eq!(opened.review.merge_route().unwrap().base, branch("main"));
    assert_eq!(
        opened.review.merge_route_sentence().as_deref(),
        Some("Reviewing feature against main")
    );
}
