use std::path::Path;
use std::process::Command;

use guided_review::pull_request::HostedRepo;

use crate::host_contract::{HostBuilder, PullOnHost, Reach, pr};

fn origin_of(folder: &Path) -> Option<HostedRepo> {
    let remote = Command::new("git")
        .arg("-C")
        .arg(folder)
        .args(["remote", "get-url", "origin"])
        .output()
        .unwrap();
    HostedRepo::of_remote(String::from_utf8(remote.stdout).unwrap().trim())
}

pub fn a_clone_is_a_git_folder_whose_origin_is_the_repo_on_the_host(host: HostBuilder) {
    let host = host(PullOnHost::default());
    let parent = tempfile::tempdir().unwrap();
    let folder = parent.path().join("shop");

    host.clone_repo(&pr().repo, &folder).unwrap();

    assert_eq!(origin_of(&folder), Some(pr().hosted_repo()));
}

pub fn a_clone_from_a_host_not_answering_fails_and_makes_no_folder(host: HostBuilder) {
    let host = host(PullOnHost {
        reach: Reach::ServerError,
        ..PullOnHost::default()
    });
    let parent = tempfile::tempdir().unwrap();
    let folder = parent.path().join("shop");

    let cloned = host.clone_repo(&pr().repo, &folder);

    assert!(cloned.is_err());
    assert!(!folder.exists());
}
