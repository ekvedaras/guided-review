//! What each host offers. A capability a host lacks is `None`, so the review hides it.

use guided_review::viewer::{CodeHost, HostError, NoHost, ReviewThreadsPort};

use super::{PullOnHost, hosts, pr};

fn offered(host: &dyn CodeHost) -> [bool; 5] {
    [
        host.viewed_marks().is_some(),
        host.merging().is_some(),
        host.pull_request_reactions().is_some(),
        host.picture_uploads().is_some(),
        host.issue_tracker().is_some(),
    ]
}

#[test]
fn github_offers_every_capability() {
    assert_eq!(
        offered(hosts::github(PullOnHost::default()).as_ref()),
        [true; 5]
    );
}

#[test]
fn no_host_offers_no_capability() {
    assert_eq!(offered(&NoHost::default()), [false; 5]);
}

#[test]
fn no_host_refuses_the_core_ports_it_has_no_pull_request_for() {
    let error = NoHost::default().review_threads(&pr()).unwrap_err();

    assert!(matches!(error, HostError::NoAccess(_)), "{error:?}");
}
