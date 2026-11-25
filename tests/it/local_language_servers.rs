use guided_review::codebase::Codebase;
use guided_review::pull_request::{HostSite, HostedRepo, PullRequestUrl, RepoName};
use guided_review::review_subject::{BranchName, LocalChangeSet, ReviewSubject, WorkTreeRoot};
use tempfile::TempDir;

fn local_in(folder: &std::path::Path, origin: Option<RepoName>) -> ReviewSubject {
    ReviewSubject::from(LocalChangeSet::new(
        WorkTreeRoot::new(folder.to_path_buf()).unwrap(),
        BranchName::new("feature").unwrap(),
        origin.map(|repo| HostedRepo {
            host: HostSite::GitHub,
            repo,
        }),
    ))
}

#[test]
fn a_local_review_is_opted_in_as_the_repo_of_its_origin_which_its_pr_reviews_share() {
    let folder = TempDir::new().unwrap();
    let local = local_in(folder.path(), Some(RepoName::new("acme", "shop")));
    let pull_request: PullRequestUrl = "https://github.com/acme/shop/pull/7".parse().unwrap();

    assert_eq!(
        Codebase::of(&local),
        Codebase::of(&ReviewSubject::from(pull_request))
    );
    assert_eq!(
        Codebase::of(&local),
        Codebase::Hosted(crate::support::shop_on_github())
    );
}

#[test]
fn a_codebase_key_that_names_its_host_is_the_codebase_its_repo_written_bare_is() {
    assert_eq!(
        "github:acme/shop".parse::<Codebase>().ok(),
        "acme/shop".parse::<Codebase>().ok()
    );
}

#[test]
fn a_folders_state_folder_is_its_name_and_a_hash_of_its_path_apart_from_every_repos() {
    let folder = |path: &str| Codebase::Folder(WorkTreeRoot::new(path.into()).unwrap());

    let here = folder("/here/shop").state_folder();
    let there = folder("/there/shop").state_folder();

    let here_name = here.to_string_lossy();
    assert!(here_name.starts_with("_folders/shop-"), "{here_name}");
    assert_eq!(here_name.len(), "_folders/shop-".len() + 8);
    assert_ne!(here, there);
    assert_eq!(here, folder("/here/shop").state_folder());
    assert_eq!(
        Codebase::Hosted(crate::support::shop_on_github()).state_folder(),
        std::path::PathBuf::from("acme/shop")
    );
}

#[test]
fn a_folder_with_no_github_origin_is_opted_in_by_its_folder() {
    let here = TempDir::new().unwrap();
    let there = TempDir::new().unwrap();
    std::fs::create_dir(here.path().join("shop")).unwrap();
    std::fs::create_dir(there.path().join("shop")).unwrap();

    let here_key = Codebase::of(&local_in(&here.path().join("shop"), None));
    let there_key = Codebase::of(&local_in(&there.path().join("shop"), None));

    assert!(matches!(here_key, Codebase::Folder(_)), "{here_key}");
    assert_ne!(here_key, there_key);
    assert_eq!(
        here_key,
        Codebase::of(&local_in(&here.path().join("shop"), None))
    );
}
