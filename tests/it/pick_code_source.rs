use std::path::Path;
use std::sync::Arc;

use crate::support::FixtureRepo;
use guided_review::code_source::{BlobInfo, CodeSource, FileDiff};
use guided_review::file_path::FilePath;
use guided_review::github::GitHub;
use guided_review::local_git::LocalGit;
use guided_review::pick_code_source::pick_code_source;
use guided_review::pull_request::{PullRequestTips, PullRequestUrl, RepoName, Sha};
use guided_review::tour::Tour;
use guided_review::viewer::{Problem, Review, ReviewStateFile};
use serde_json::json;

const PR: &str = "https://github.com/acme/shop/pull/7";

struct Shop {
    repo: FixtureRepo,
    base: String,
    head: String,
}

impl Shop {
    fn with_pull() -> Self {
        let repo = FixtureRepo::cloned_from("acme/shop");
        repo.write("README.md", "hello\n");
        let base = repo.commit("base");
        repo.write("README.md", "hello\nworld\n");
        let head = repo.commit("head");
        Self { repo, base, head }
    }

    fn tips(&self) -> PullRequestTips {
        PullRequestTips {
            base_branch_tip: Sha::new(&self.base),
            head: Sha::new(&self.head),
        }
    }

    fn pick(&self, tips: &PullRequestTips) -> Box<dyn CodeSource> {
        let github = GitHub::at("http://127.0.0.1:1");
        pick_code_source(&github, &pr(), tips, self.repo.path()).unwrap()
    }

    fn open(&self, source: &dyn CodeSource) -> Review {
        let tour: Tour = serde_json::from_value(json!({
            "pr": PR,
            "base": self.base,
            "head": self.head,
            "title": "Hello world",
            "summary": [],
            "steps": [{"id": "readme", "title": "Readme", "why": "w", "pieces": [{"file": "README.md"}]}],
        }))
        .unwrap();
        let state_file = ReviewStateFile::for_pr(self.repo.home(), &pr());
        Review::open(tour, source, state_file).unwrap()
    }
}

fn pr() -> PullRequestUrl {
    PR.parse().unwrap()
}

/// A commit the clone lacks, which its only remote cannot give.
fn tips_the_clone_cannot_fetch(shop: &Shop) -> PullRequestTips {
    shop.repo.git(&[
        "config",
        "url./nowhere/shop.insteadOf",
        "git@github.com:acme/shop.git",
    ]);
    PullRequestTips {
        base_branch_tip: Sha::new(&shop.base),
        head: Sha::new("0123456789012345678901234567890123456789"),
    }
}

/// A local clone that stands in for the GitHub source a failed fetch falls back to.
struct FellBack(LocalGit);

impl CodeSource for FellBack {
    fn file_at(&self, sha: &Sha, path: &FilePath) -> anyhow::Result<String> {
        self.0.file_at(sha, path)
    }

    fn blob_at(&self, sha: &Sha, path: &FilePath) -> anyhow::Result<Arc<[u8]>> {
        self.0.blob_at(sha, path)
    }

    fn blob_info_at(&self, sha: &Sha, path: &FilePath) -> anyhow::Result<BlobInfo> {
        self.0.blob_info_at(sha, path)
    }

    fn diff(&self, base: &Sha, head: &Sha) -> anyhow::Result<Vec<FileDiff>> {
        self.0.diff(base, head)
    }

    fn merge_base(&self, a: &Sha, b: &Sha) -> anyhow::Result<Sha> {
        self.0.merge_base(a, b)
    }

    fn working_tree_is_at(&self, head: &Sha) -> bool {
        self.0.working_tree_is_at(head)
    }

    fn local_clone(&self) -> Option<&Path> {
        None
    }

    fn fallback_reason(&self) -> Option<&str> {
        Some("could not fetch")
    }
}

#[test]
fn a_clone_that_cannot_fetch_leaves_the_code_on_github_and_says_why() {
    let shop = Shop::with_pull();
    let tips = tips_the_clone_cannot_fetch(&shop);

    let source = shop.pick(&tips);

    assert!(source.local_clone().is_none());
    assert!(source.fallback_reason().is_some());
}

#[test]
fn a_clone_that_has_the_commits_is_read_without_a_reason_to_fall_back() {
    let shop = Shop::with_pull();

    let source = shop.pick(&shop.tips());

    assert!(source.local_clone().is_some());
    assert_eq!(source.fallback_reason(), None);
}

#[test]
fn a_review_opened_on_a_fallen_back_source_names_why_until_one_opens_on_the_clone() {
    let shop = Shop::with_pull();
    let clone = LocalGit::find(&RepoName::new("acme", "shop"), shop.repo.path())
        .unwrap()
        .unwrap();

    let fallen_back = shop.open(&FellBack(clone));
    assert_eq!(
        fallen_back.problems(),
        [Problem::CodeOnGitHub("could not fetch".into())]
    );

    let on_the_clone = shop.open(shop.pick(&shop.tips()).as_ref());
    assert_eq!(on_the_clone.problems(), []);
}
