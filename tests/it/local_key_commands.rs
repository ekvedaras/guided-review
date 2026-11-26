use crate::support::{FakeGitHub, FixtureRepo, GuidedReview};

#[test]
fn a_command_given_neither_a_pr_link_nor_a_local_key_names_what_it_takes() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    let github = FakeGitHub::serving_pull("acme/shop", 7, "aaa", "bbb");

    let run = GuidedReview::new(&repo, &github).run(&["outline", "acme/shop#7"]);

    assert!(!run.success);
    assert!(
        run.stderr
            .contains("neither a PR link of a code host guided-review reads (GitHub) nor a local change set key"),
        "{}",
        run.stderr
    );
}
