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

mod on_disk {
    use std::path::Path;

    use guided_review::tour::Tour;
    use guided_review::viewer::{ReviewState, ReviewStateFile};

    use super::subject;
    use crate::support::folders_in;

    const LOCAL: &str = "local:/Users/me/Code/shop@feature/discounts";

    #[test]
    fn a_local_change_sets_tours_are_in_a_folder_named_by_its_folder_and_a_hash_of_its_path() {
        let folders = folders_in(Path::new("/home/me"));
        let tours = folders.tours();

        let local = subject(LOCAL);

        assert_eq!(
            Tour::basic_path_for(&folders, &local),
            tours.join("local/shop-c15cfd06/feature%2Fdiscounts.basic.json")
        );
        assert_eq!(
            Tour::path_for(&folders, &local),
            tours.join("local/shop-c15cfd06/feature%2Fdiscounts.json")
        );
        assert_eq!(
            Tour::files_for(&folders, &local),
            tours.join("local/shop-c15cfd06/feature%2Fdiscounts")
        );
    }

    #[test]
    fn two_folders_with_the_same_name_do_not_share_tours() {
        let folders = folders_in(Path::new("/home/me"));

        assert_ne!(
            Tour::path_for(&folders, &subject("local:/Users/me/Code/shop@main")),
            Tour::path_for(&folders, &subject("local:/Users/me/Other/shop@main"))
        );
    }

    #[test]
    fn a_pull_requests_files_stay_where_they_always_were() {
        let folders = folders_in(Path::new("/home/me"));
        let pr = subject("https://github.com/acme/shop/pull/7");

        assert_eq!(
            Tour::path_for(&folders, &pr),
            folders.tours().join("acme/shop/7.json")
        );
        assert_eq!(
            Tour::basic_path_for(&folders, &pr),
            folders.tours().join("acme/shop/7.basic.json")
        );
        assert_eq!(
            Tour::files_for(&folders, &pr),
            folders.tours().join("acme/shop/7")
        );
    }

    #[test]
    fn a_local_reviews_state_is_saved_at_the_matching_path_under_the_state_folder() {
        let home = tempfile::tempdir().unwrap();
        let folders = folders_in(home.path());

        ReviewStateFile::for_subject(&folders, &subject(LOCAL))
            .save(&ReviewState::default())
            .unwrap();

        assert!(
            folders
                .state_files()
                .join("local/shop-c15cfd06/feature%2Fdiscounts.json")
                .is_file()
        );
    }
}
