use crate::support::FixtureRepo;
use guided_review::github::GitHub;
use guided_review::local_git::LocalGit;
use guided_review::pull_request::{PullRequestUrl, RepoName};
use guided_review::tour::Tour;
use guided_review::viewer::{Block, PullRequestDescriptionPort, Review, ReviewStateFile};
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

    fn open(&self) -> Review {
        let tour: Tour = serde_json::from_value(json!({
            "pr": PR,
            "base": self.base,
            "head": self.head,
            "title": "Hello world",
            "summary": ["Greets the world"],
            "steps": [{"id": "readme", "title": "Readme", "why": "w", "pieces": [{"file": "README.md"}]}],
        }))
        .unwrap();
        let source = LocalGit::find(&RepoName::new("acme", "shop"), self.repo.path())
            .unwrap()
            .unwrap();
        let state_file =
            ReviewStateFile::for_pr(self.repo.home(), &PR.parse::<PullRequestUrl>().unwrap());
        Review::open(tour, &source, state_file).unwrap()
    }
}

fn paragraph_texts(blocks: &[Block]) -> Vec<String> {
    blocks
        .iter()
        .map(|block| match block {
            Block::Paragraph(runs) => runs.iter().map(|run| run.text.as_str()).collect(),
            other => format!("{other:?}"),
        })
        .collect()
}

#[test]
fn the_pr_description_shows_under_the_summary_as_markdown() {
    let shop = Shop::with_pull();
    let mut review = shop.open();
    assert!(review.pr_description().is_none());

    review
        .set_pr_description("Fixes **totals**.\n\nSee the ticket.")
        .unwrap();

    let description = review.pr_description().unwrap();
    assert_eq!(
        paragraph_texts(&description.blocks),
        ["Fixes totals.", "See the ticket."]
    );
}

#[test]
fn headings_of_the_pr_description_rank_under_the_tours_own() {
    let shop = Shop::with_pull();
    let mut review = shop.open();

    review
        .set_pr_description("## What\n\nFixes totals.")
        .unwrap();

    assert!(matches!(
        review.pr_description().unwrap().blocks[0],
        Block::Heading { level: 3, .. }
    ));
}

#[test]
fn an_empty_pr_description_shows_nothing() {
    let shop = Shop::with_pull();
    let mut review = shop.open();

    review.set_pr_description("  \r\n ").unwrap();

    assert!(review.pr_description().is_none());
}

#[test]
fn the_last_pr_description_shows_at_once_on_the_next_open() {
    let shop = Shop::with_pull();
    shop.open().set_pr_description("Fixes totals.").unwrap();

    let review = shop.open();

    assert_eq!(
        paragraph_texts(&review.pr_description().unwrap().blocks),
        ["Fixes totals."]
    );
}

#[test]
fn github_gives_the_pr_body_and_an_empty_description_for_none() {
    let described = crate::support::FakeGitHub::serving_pull_described(
        "acme/shop",
        7,
        "b",
        "h",
        r#""Fixes totals""#,
    );
    let undescribed = crate::support::FakeGitHub::serving_pull("acme/shop", 7, "b", "h");
    let pr: PullRequestUrl = PR.parse().unwrap();

    assert_eq!(
        GitHub::at(&described.url)
            .pull_request_description(&pr)
            .unwrap(),
        "Fixes totals"
    );
    assert_eq!(
        GitHub::at(&undescribed.url)
            .pull_request_description(&pr)
            .unwrap(),
        ""
    );
}
