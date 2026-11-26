use guided_review::viewer::{
    CodeHost, HostError, PullRequestReactionsPort, Reaction, ReactionChange,
};

use crate::host_contract::{HostBuilder, PullOnHost, Reach, pr};

fn reactions_of(host: &dyn CodeHost) -> &dyn PullRequestReactionsPort {
    host.pull_request_reactions()
        .expect("the host offers reactions on a PR")
}

/// No port reads a PR's reactions back, so only taking them is the contract.
pub fn reactions_on_the_pr_are_taken_added_and_removed(host: HostBuilder) {
    let host = host(PullOnHost::default());
    let reactions = reactions_of(host.as_ref());

    let added = reactions.set_pull_request_reaction(&pr(), Reaction::Eyes, ReactionChange::Add);
    let removed =
        reactions.set_pull_request_reaction(&pr(), Reaction::Eyes, ReactionChange::Remove);

    assert!(added.is_ok(), "{added:?}");
    assert!(removed.is_ok(), "{removed:?}");
}

pub fn reactions_on_the_pr_without_a_token_are_refused(host: HostBuilder) {
    let host = host(PullOnHost {
        reach: Reach::NoToken,
        ..PullOnHost::default()
    });

    let error = reactions_of(host.as_ref())
        .set_pull_request_reaction(&pr(), Reaction::Eyes, ReactionChange::Add)
        .unwrap_err();

    assert!(matches!(error, HostError::NoAccess(_)), "{error:?}");
}

pub fn reactions_on_the_pr_of_a_host_not_answering_fail_as_an_outage(host: HostBuilder) {
    let host = host(PullOnHost {
        reach: Reach::ServerError,
        ..PullOnHost::default()
    });

    let error = reactions_of(host.as_ref())
        .set_pull_request_reaction(&pr(), Reaction::Eyes, ReactionChange::Add)
        .unwrap_err();

    assert!(matches!(error, HostError::NotAnswering(..)), "{error:?}");
}
