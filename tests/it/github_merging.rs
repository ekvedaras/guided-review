use crate::support::graphql::serve_graphql;
use guided_review::github::GitHub;
use guided_review::pull_request::PullRequestUrl;
use guided_review::viewer::{
    AutoMerge, MergeMethod, MergeMethods, Mergeability, Merging, MergingPort, PrStatus,
    PullRequestStatusPort,
};

fn pr() -> PullRequestUrl {
    "https://github.com/acme/shop/pull/7".parse().unwrap()
}

#[test]
fn how_a_pr_would_merge_is_read_in_a_query_of_its_own() {
    let answer = r#"{"data":{"repository":{
        "autoMergeAllowed":true,"mergeCommitAllowed":false,"squashMergeAllowed":true,"rebaseMergeAllowed":true,
        "viewerPermission":"WRITE",
        "pullRequest":{"mergeStateStatus":"BLOCKED","viewerCanEnableAutoMerge":false,
            "viewerCanDisableAutoMerge":true,"autoMergeRequest":{"mergeMethod":"SQUASH"}}}}}"#;
    let (url, asked) = serve_graphql(vec![answer.into()]);

    let merging = GitHub::at(&url).pull_request_merging(&pr()).unwrap();

    assert_eq!(
        merging,
        Merging {
            mergeability: Mergeability::Blocked,
            auto_merge: AutoMerge::On(MergeMethod::Squash),
            methods: MergeMethods::only(&[MergeMethod::Squash, MergeMethod::Rebase]),
            repo_allows_auto_merge: true,
            viewer_may_enable_auto_merge: false,
            viewer_may_disable_auto_merge: true,
            viewer_may_merge: true,
        }
    );
    assert_eq!(asked.lock().unwrap().len(), 1);
}

#[test]
fn the_status_of_a_pr_asks_nothing_of_merging() {
    let answer = r#"{"data":{"repository":{"pullRequest":{
        "isDraft":false,"viewerCanUpdate":true,"state":"OPEN",
        "commits":{"nodes":[{"commit":{"statusCheckRollup":null}}]}}}}}"#;
    let (url, asked) = serve_graphql(vec![answer.into()]);

    let status = GitHub::at(&url).pull_request_status(&pr()).unwrap();

    assert!(matches!(status, PrStatus::Open(_)), "{status:?}");
    let query = asked.lock().unwrap()[0].clone();
    assert!(!query.contains("mergeStateStatus"), "{query}");
}
