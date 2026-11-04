#[allow(
    dead_code,
    reason = "each test binary uses only part of the shared test support"
)]
mod support;

use support::{FakeGitHub, FixtureRepo, GuidedReview};

#[test]
fn open_without_a_tour_file_says_to_run_the_tour_skill_first() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    let sha = repo.commit("base");
    let github = FakeGitHub::serving_pull("acme/shop", 7, &sha, &sha);

    let run =
        GuidedReview::new(&repo, &github).run(&["open", "https://github.com/acme/shop/pull/7"]);

    assert!(!run.success);
    assert!(
        run.stderr.contains("no tour for https://github.com/acme/shop/pull/7 yet; run `/tour https://github.com/acme/shop/pull/7` first"),
        "stderr: {}",
        run.stderr
    );
}
