use guided_review::viewer::{RelevantPullRequest, RelevantPullRequests, WhyListed};

use crate::host_contract::{HostBuilder, PullOnHost, UPDATED_AT, VIEWER, author_of, pr};

pub fn the_pr_is_listed_as_the_viewers_own_as_asked_of_them_or_as_involving_them(
    host: HostBuilder,
) {
    for (pull, why_listed) in [
        (
            PullOnHost {
                author: VIEWER,
                ..PullOnHost::default()
            },
            WhyListed::Mine,
        ),
        (
            PullOnHost {
                asks_viewer_to_review: true,
                ..PullOnHost::default()
            },
            WhyListed::ReviewRequested,
        ),
        (PullOnHost::default(), WhyListed::Involved),
    ] {
        let listed = RelevantPullRequest {
            url: pr(),
            title: pull.title.to_owned(),
            author: author_of(&pull),
            is_draft: false,
            updated_at: UPDATED_AT.to_owned(),
            why_listed,
        };

        let relevant = host(pull).relevant_pull_requests().unwrap();

        assert_eq!(
            relevant,
            RelevantPullRequests {
                pull_requests: vec![listed],
                teams_left_out: Vec::new(),
            },
            "{why_listed:?}"
        );
    }
}
