use guided_review::review_subject::BranchName;
use guided_review::viewer::{HeadBranch, HostError, MergeRoute, PullRequestOrigin};

use crate::host_contract::{HostBuilder, PullOnHost, Reach, author_of, pr};

fn branch(name: &str) -> BranchName {
    BranchName::new(name).unwrap()
}

pub fn the_origin_is_the_author_and_the_branches_of_the_repo_or_a_fork(host: HostBuilder) {
    let own = PullOnHost {
        base_branch: "main",
        head_branch: "cart",
        ..PullOnHost::default()
    };
    let fork = PullOnHost {
        author: "mallory",
        base_branch: "release",
        fork_owner: Some("mallory"),
        ..PullOnHost::default()
    };
    for (pull, route) in [
        (
            own,
            MergeRoute {
                base: branch("main"),
                head: HeadBranch::of_repo(branch("cart")),
            },
        ),
        (
            fork,
            MergeRoute {
                base: branch("release"),
                head: HeadBranch::of_fork("mallory", branch("cart")),
            },
        ),
    ] {
        let author = author_of(&pull);

        let origin = host(pull).pull_request_origin(&pr()).unwrap();

        assert_eq!(
            origin,
            PullRequestOrigin {
                author: Some(author),
                route,
            }
        );
    }
}

pub fn the_origin_of_a_host_not_answering_fails_as_an_outage(host: HostBuilder) {
    let host = host(PullOnHost {
        reach: Reach::ServerError,
        ..PullOnHost::default()
    });

    let error = host.pull_request_origin(&pr()).unwrap_err();

    assert!(matches!(error, HostError::NotAnswering(..)), "{error:?}");
}
