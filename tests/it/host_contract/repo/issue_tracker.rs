use guided_review::issue::IssueUrl;
use guided_review::viewer::{CodeHost, HostError, IssueDetails, IssueDetailsPort};

use crate::host_contract::pull_request::description::carts_issue;
use crate::host_contract::{HostBuilder, PullOnHost, Reach};

fn tracker_of(host: &dyn CodeHost) -> &dyn IssueDetailsPort {
    host.issue_tracker().expect("the host has an issue tracker")
}

fn issue(number: u64) -> IssueUrl {
    IssueUrl::new("acme/shop".parse().unwrap(), number)
}

pub fn each_known_issue_has_its_details_and_an_unknown_one_none(host: HostBuilder) {
    let host = host(PullOnHost {
        closing_issues: vec![carts_issue()],
        ..PullOnHost::default()
    });

    let details = tracker_of(host.as_ref())
        .issue_details(&[issue(12), issue(99)])
        .unwrap();

    assert_eq!(
        details,
        vec![(
            issue(12),
            IssueDetails {
                title: "Carts".into(),
                state: guided_review::viewer::IssueState::Open,
                author: Some("carol".into()),
                labels: ["feature".into(), "shop".into()].into(),
                assignees: ["alice".into()].into(),
                body: IssueDetails::markdown_of("We need **carts**."),
            }
        )]
    );
}

pub fn issue_details_of_a_host_not_answering_fail_as_an_outage(host: HostBuilder) {
    let host = host(PullOnHost {
        closing_issues: vec![carts_issue()],
        reach: Reach::ServerError,
        ..PullOnHost::default()
    });

    let error = tracker_of(host.as_ref())
        .issue_details(&[issue(12)])
        .unwrap_err();

    assert!(matches!(error, HostError::NotAnswering(..)), "{error:?}");
}
