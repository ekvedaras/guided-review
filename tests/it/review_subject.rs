use guided_review::pull_request::PullRequestUrl;
use guided_review::review_subject::ReviewSubject;

fn subject(key: &str) -> ReviewSubject {
    key.parse().unwrap()
}

#[test]
fn a_pull_requests_key_is_exactly_its_url() {
    let url = "https://github.com/acme/shop/pull/7";

    let parsed = subject(url);

    assert_eq!(parsed.to_string(), url);
    assert_eq!(
        parsed.pull_request(),
        Some(&url.parse::<PullRequestUrl>().unwrap())
    );
    assert!(parsed.local().is_none());
}

#[test]
fn a_local_change_sets_key_names_its_folder_and_branch_and_round_trips() {
    let key = "local:/Users/me/Code/shop@feature/discounts";

    let parsed = subject(key);

    assert_eq!(parsed.to_string(), key);
    let local = parsed.local().unwrap();
    assert_eq!(
        local.folder().as_path().to_str(),
        Some("/Users/me/Code/shop")
    );
    assert_eq!(local.branch().as_str(), "feature/discounts");
    assert!(parsed.pull_request().is_none());
}

#[test]
fn an_at_sign_in_the_folder_or_the_branch_survives_the_round_trip() {
    let key = "local:/Users/me/GoogleDrive-me%40mail.com/shop@dependabot/npm/@types/node-20";

    let parsed = subject(key);

    let local = parsed.local().unwrap();
    assert_eq!(
        local.folder().as_path().to_str(),
        Some("/Users/me/GoogleDrive-me@mail.com/shop")
    );
    assert_eq!(local.branch().as_str(), "dependabot/npm/@types/node-20");
    assert_eq!(parsed.to_string(), key);
}

#[test]
fn a_local_key_needs_an_absolute_folder_and_a_branch() {
    for key in [
        "local:shop@main",
        "local:/Users/me/shop",
        "local:/Users/me/shop@",
        "local:",
    ] {
        assert!(
            key.parse::<ReviewSubject>().is_err(),
            "{key} should be refused"
        );
    }
}

#[test]
fn text_that_is_neither_a_pr_link_nor_a_local_key_is_refused() {
    let refused = "acme/shop#7".parse::<ReviewSubject>().unwrap_err();

    assert!(refused.to_string().contains("acme/shop#7"), "{refused:#}");
}

#[test]
fn a_local_change_sets_identity_is_its_folder_and_branch() {
    assert_eq!(
        subject("local:/Users/me/shop@main"),
        subject("local:/Users/me/shop@main")
    );
    assert_ne!(
        subject("local:/Users/me/shop@main"),
        subject("local:/Users/me/shop-2@main")
    );
    assert_ne!(
        subject("local:/Users/me/shop@main"),
        subject("local:/Users/me/shop@other")
    );
}
