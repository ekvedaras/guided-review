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
fn a_local_change_sets_folder_is_its_canonical_path_so_two_names_of_it_are_one_review() {
    let real = tempfile::tempdir().unwrap();
    let links = tempfile::tempdir().unwrap();
    let link = links.path().join("shop-link");
    std::os::unix::fs::symlink(real.path(), &link).unwrap();
    let canonical = real.path().canonicalize().unwrap();

    let through_link = subject(&format!("local:{}@main", link.display()));

    assert_eq!(
        through_link,
        subject(&format!("local:{}@main", canonical.display()))
    );
    assert_eq!(
        through_link.to_string(),
        format!("local:{}@main", canonical.display())
    );
}

#[test]
fn what_the_app_tells_a_refused_open_is_one_short_line_naming_what_open_takes() {
    assert_eq!(
        ReviewSubject::not_a_subject("acme/shop#7"),
        "`acme/shop#7` is not a PR link or a local key"
    );
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

    /// A local review's state is saved only while its tour is on disk.
    fn with_its_tour(
        folders: &guided_review::app_folders::AppFolders,
        subject: &super::ReviewSubject,
    ) {
        let path = Tour::path_for(folders, subject);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            format!(r#"{{"local": "{subject}", "base": "aaa", "head": "bbb"}}"#),
        )
        .unwrap();
    }

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
        with_its_tour(&folders, &subject(LOCAL));

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

    #[test]
    fn a_tour_file_pinned_to_another_subject_is_refused_when_it_is_loaded_for_this_one() {
        let home = tempfile::tempdir().unwrap();
        let folders = folders_in(home.path());
        let this = subject(LOCAL);
        let other = subject("local:/Users/me/Other/shop@feature/discounts");
        let path = Tour::path_for(&folders, &this);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            format!(r#"{{"local": "{other}", "base": "aaa", "head": "bbb"}}"#),
        )
        .unwrap();

        let refused = Tour::load_of(&path, &this).unwrap_err();

        assert!(
            format!("{refused:#}").contains(&other.to_string()),
            "{refused:#}"
        );
    }

    #[test]
    fn a_tour_file_pinned_to_this_subject_loads() {
        let home = tempfile::tempdir().unwrap();
        let folders = folders_in(home.path());
        let this = subject(LOCAL);
        let path = Tour::path_for(&folders, &this);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            format!(r#"{{"local": "{this}", "base": "aaa", "head": "bbb"}}"#),
        )
        .unwrap();

        assert!(Tour::load_of(&path, &this).is_ok());
    }

    #[test]
    fn a_state_file_of_another_subject_is_refused_when_it_is_loaded_for_this_one() {
        let home = tempfile::tempdir().unwrap();
        let folders = folders_in(home.path());
        let this = subject(LOCAL);
        let other = subject("local:/Users/me/Other/shop@feature/discounts");
        with_its_tour(&folders, &other);
        ReviewStateFile::for_subject(&folders, &other)
            .save(&ReviewState::default())
            .unwrap();
        let state_path = |subject: &super::ReviewSubject| {
            folders
                .state_files()
                .join(subject.files_folder())
                .join(format!("{}.json", subject.file_stem()))
        };
        std::fs::create_dir_all(state_path(&this).parent().unwrap()).unwrap();
        std::fs::copy(state_path(&other), state_path(&this)).unwrap();

        let refused = ReviewStateFile::for_subject(&folders, &this)
            .load()
            .unwrap_err();

        assert!(
            format!("{refused:#}").contains(&other.to_string()),
            "{refused:#}"
        );
    }

    #[test]
    fn a_state_file_keeps_its_subjects_full_key_through_a_save() {
        let home = tempfile::tempdir().unwrap();
        let folders = folders_in(home.path());
        let this = subject(LOCAL);
        let file = ReviewStateFile::for_subject(&folders, &this);
        with_its_tour(&folders, &this);

        file.save(&ReviewState::default()).unwrap();
        file.update(|_| {}).unwrap();

        assert!(file.load().is_ok());
        let text = std::fs::read_to_string(
            folders
                .state_files()
                .join("local/shop-c15cfd06/feature%2Fdiscounts.json"),
        )
        .unwrap();
        assert!(text.contains(LOCAL), "{text}");
    }
}
