use crate::support::{FakeGitHub, FixtureRepo, GuidedReview};

const LOCAL_KEY: &str = "local:/Users/me/Code/shop@feature/discounts";

#[test]
fn a_command_given_a_local_change_sets_key_says_local_review_is_not_built_yet() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    let github = FakeGitHub::serving_pull("acme/shop", 7, "aaa", "bbb");
    let guided_review = GuidedReview::new(&repo, &github);

    for args in [
        vec!["outline", LOCAL_KEY],
        vec!["show", LOCAL_KEY, "README.md"],
        vec!["tour-path", LOCAL_KEY],
        vec!["check", LOCAL_KEY],
        vec!["repin", LOCAL_KEY],
        vec!["refresh", LOCAL_KEY],
        vec!["open", LOCAL_KEY],
        vec!["restart", LOCAL_KEY],
        vec!["agent-reply", LOCAL_KEY, "h0", "--working"],
    ] {
        let run = guided_review.run(&args);

        assert!(!run.success, "{args:?} should be refused");
        assert!(
            run.stderr.contains("local review is not built yet"),
            "{args:?} said:\n{}",
            run.stderr
        );
    }
}

#[test]
fn a_command_given_neither_a_pr_link_nor_a_local_key_names_what_it_takes() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    let github = FakeGitHub::serving_pull("acme/shop", 7, "aaa", "bbb");

    let run = GuidedReview::new(&repo, &github).run(&["outline", "acme/shop#7"]);

    assert!(!run.success);
    assert!(
        run.stderr
            .contains("neither a GitHub PR link nor a local change set key"),
        "{}",
        run.stderr
    );
}
