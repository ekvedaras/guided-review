//! A repo's path on its host: GitHub's `owner/name`, GitLab's `group/sub/project`.

use guided_review::pull_request::RepoName;

#[test]
fn a_repo_path_of_three_segments_reads_as_its_namespace_and_name_and_writes_back_the_same() {
    let read = "group/sub/project".parse::<RepoName>().ok();

    assert_eq!(
        read.map(|repo| (
            repo.namespace().to_owned(),
            repo.name().to_owned(),
            repo.to_string()
        )),
        Some((
            "group/sub".to_owned(),
            "project".to_owned(),
            "group/sub/project".to_owned()
        ))
    );
}
