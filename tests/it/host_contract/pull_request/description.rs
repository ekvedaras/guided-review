use std::sync::Arc;

use guided_review::issue::{IssueLink, IssueUrl};
use guided_review::review_subject::BranchName;
use guided_review::viewer::{HostError, IssueState, PullRequestIssues, RelatedIssue};

use crate::host_contract::{HostBuilder, PullOnHost, Reach, SeedIssue, pr};

pub fn carts_issue() -> SeedIssue {
    SeedIssue {
        number: 12,
        title: "Carts",
        state: IssueState::Open,
        author: "carol",
        labels: vec!["feature", "shop"],
        assignees: vec!["alice"],
        body: "We need **carts**.",
    }
}

pub fn the_title_and_description_read_as_the_host_has_them(host: HostBuilder) {
    let host = host(PullOnHost {
        title: "Add a cart",
        description: "Adds a cart.\n\nCloses #12",
        ..PullOnHost::default()
    });

    assert_eq!(host.pull_request_title(&pr()).unwrap(), "Add a cart");
    assert_eq!(
        host.pull_request_description(&pr()).unwrap(),
        "Adds a cart.\n\nCloses #12"
    );
}

pub fn a_description_written_reads_back(host: HostBuilder) {
    let host = host(PullOnHost::default());

    host.set_pull_request_description(&pr(), "Adds a cart.\n\n![Before](https://x/y.png)")
        .unwrap();

    assert_eq!(
        host.pull_request_description(&pr()).unwrap(),
        "Adds a cart.\n\n![Before](https://x/y.png)"
    );
    assert_eq!(host.pull_request_title(&pr()).unwrap(), "Add a cart");
}

pub fn the_issues_the_pr_closes_are_linked_with_their_titles_and_the_head_branch(
    host: HostBuilder,
) {
    let host = host(PullOnHost {
        closing_issues: vec![carts_issue()],
        ..PullOnHost::default()
    });

    let issues = host.pull_request_issues(&pr()).unwrap();

    assert_eq!(
        issues,
        PullRequestIssues {
            linked: vec![RelatedIssue {
                link: IssueLink::GitHub(IssueUrl::new("acme/shop".parse().unwrap(), 12)),
                title: Some(Arc::from("Carts")),
            }],
            head_branch: Some(BranchName::new("cart").unwrap()),
        }
    );
}

pub fn the_description_of_a_host_not_answering_fails_as_an_outage(host: HostBuilder) {
    let host = host(PullOnHost {
        reach: Reach::ServerError,
        ..PullOnHost::default()
    });

    let title = host.pull_request_title(&pr()).unwrap_err();
    let description = host.pull_request_description(&pr()).unwrap_err();
    let issues = host.pull_request_issues(&pr()).unwrap_err();
    let written = host.set_pull_request_description(&pr(), "x");

    for error in [title, description, issues] {
        assert!(matches!(error, HostError::NotAnswering(..)), "{error:?}");
    }
    assert!(
        written.is_err(),
        "a description written to a host not answering went through"
    );
}
