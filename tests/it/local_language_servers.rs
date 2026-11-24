use guided_review::pull_request::{PullRequestUrl, RepoName};
use guided_review::review_subject::{BranchName, LocalChangeSet, ReviewSubject, WorkTreeRoot};
use tempfile::TempDir;

fn local_in(folder: &std::path::Path, origin: Option<RepoName>) -> ReviewSubject {
    ReviewSubject::from(LocalChangeSet::new(
        WorkTreeRoot::new(folder.to_path_buf()).unwrap(),
        BranchName::new("feature").unwrap(),
        origin,
    ))
}

#[test]
fn a_local_review_is_opted_in_as_the_repo_of_its_origin_which_its_pr_reviews_share() {
    let folder = TempDir::new().unwrap();
    let local = local_in(folder.path(), Some(RepoName::new("acme", "shop")));
    let pull_request: PullRequestUrl = "https://github.com/acme/shop/pull/7".parse().unwrap();

    assert_eq!(
        local.language_server_repo(),
        ReviewSubject::from(pull_request).language_server_repo()
    );
    assert_eq!(local.language_server_repo(), RepoName::new("acme", "shop"));
}

#[test]
fn a_folder_with_no_github_origin_is_opted_in_by_its_folder() {
    let here = TempDir::new().unwrap();
    let there = TempDir::new().unwrap();
    std::fs::create_dir(here.path().join("shop")).unwrap();
    std::fs::create_dir(there.path().join("shop")).unwrap();

    let here_key = local_in(&here.path().join("shop"), None).language_server_repo();
    let there_key = local_in(&there.path().join("shop"), None).language_server_repo();

    assert!(here_key.name().starts_with("shop-"), "{here_key}");
    assert_ne!(here_key, there_key);
    assert_ne!(here_key, RepoName::new("acme", "shop"));
    assert_eq!(
        here_key,
        local_in(&here.path().join("shop"), None).language_server_repo()
    );
}
