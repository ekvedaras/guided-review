use std::sync::Arc;

use crate::support::FixtureRepo;
use guided_review::open_review::{OpenedReview, open_review};
use guided_review::pull_request::PullRequestUrl;
use guided_review::tour::Tour;
use serde_json::json;

const PR: &str = "https://github.com/acme/shop/pull/7";

fn pr() -> PullRequestUrl {
    PR.parse().unwrap()
}

fn shop_with_a_tour() -> FixtureRepo {
    let repo = FixtureRepo::cloned_from("acme/shop");
    repo.write("README.md", "hello\n");
    let base = repo.commit("base");
    repo.write("README.md", "hello\nworld\n");
    let head = repo.commit("head");
    let tour = json!({
        "pr": PR,
        "base": base,
        "head": head,
        "title": "Hello world",
        "summary": [],
        "steps": [{"id": "readme", "title": "Readme", "why": "w", "pieces": [{"file": "README.md"}]}],
    });
    let path = Tour::path_for(repo.home(), &pr());
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, tour.to_string()).unwrap();
    repo
}

#[test]
fn a_dropped_review_frees_its_code_source() {
    let repo = shop_with_a_tour();
    let opened = open_review(&pr(), repo.path(), repo.home()).unwrap();
    let source = Arc::downgrade(&opened.source);

    drop(opened);

    assert!(source.upgrade().is_none());
}

#[test]
fn the_reloader_reads_the_tour_file_again() {
    let repo = shop_with_a_tour();
    let opened = open_review(&pr(), repo.path(), repo.home()).unwrap();
    assert_eq!(opened.review.title(), "Hello world");
    let path = Tour::path_for(repo.home(), &pr());
    let rewritten = std::fs::read_to_string(&path)
        .unwrap()
        .replace("Hello world", "Hello again");
    std::fs::write(path, rewritten).unwrap();

    let reloaded = opened.reloader.run().unwrap();

    assert_eq!(reloaded.review.title(), "Hello again");
}

#[test]
fn an_opened_review_can_be_built_off_the_ui_thread() {
    fn is_send<T: Send>() {}
    is_send::<OpenedReview>();
}

#[test]
fn a_pr_without_a_tour_is_refused_with_the_command_that_writes_one() {
    let repo = FixtureRepo::cloned_from("acme/shop");

    let error = open_review(&pr(), repo.path(), repo.home()).err().unwrap();

    assert_eq!(
        error.to_string(),
        format!("no tour for {PR} yet; run `/tour {PR}` first")
    );
}
