use guided_review::review_subject::BranchName;
use guided_review::viewer::{HostError, PrStatus, PullOfBranch};

use crate::host_contract::{HostBuilder, PullOnHost, Reach, pr};
use crate::support::shop_on_github;

fn branch(name: &str) -> BranchName {
    BranchName::new(name).unwrap()
}

pub fn the_open_pr_of_its_head_branch_is_found_and_no_other_branch_has_one(host: HostBuilder) {
    let open = host(PullOnHost::default());
    let merged = host(PullOnHost {
        status: PrStatus::Merged,
        ..PullOnHost::default()
    });

    let found = open.open_pull_of_branch(&shop_on_github(), &branch("cart"));
    let other = open.open_pull_of_branch(&shop_on_github(), &branch("other"));
    let of_merged = merged.open_pull_of_branch(&shop_on_github(), &branch("cart"));

    assert_eq!(
        found.unwrap(),
        Some(PullOfBranch {
            pr: pr(),
            title: "Add a cart".to_owned(),
        })
    );
    assert_eq!(other.unwrap(), None);
    assert_eq!(of_merged.unwrap(), None);
}

pub fn the_head_branch_of_a_pr_from_the_repo_itself_is_known_and_of_a_fork_none(host: HostBuilder) {
    let own = host(PullOnHost::default());
    let fork = host(PullOnHost {
        fork_owner: Some("mallory"),
        ..PullOnHost::default()
    });

    assert_eq!(
        own.pull_request_head_branch(&pr()).unwrap(),
        Some(branch("cart"))
    );
    assert_eq!(fork.pull_request_head_branch(&pr()).unwrap(), None);
}

pub fn pulls_of_branch_of_a_host_not_answering_fail_as_an_outage(host: HostBuilder) {
    let host = host(PullOnHost {
        reach: Reach::ServerError,
        ..PullOnHost::default()
    });

    let of_branch = host
        .open_pull_of_branch(&shop_on_github(), &branch("cart"))
        .unwrap_err();
    let head = host.pull_request_head_branch(&pr()).unwrap_err();

    for error in [of_branch, head] {
        assert!(matches!(error, HostError::NotAnswering(..)), "{error:?}");
    }
}
