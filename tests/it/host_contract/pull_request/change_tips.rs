use guided_review::pull_request::Sha;
use guided_review::viewer::HostError;

use crate::host_contract::{HostBuilder, PullOnHost, Reach, pr};

pub fn the_tips_are_the_base_branch_tip_and_the_head(host: HostBuilder) {
    let host = host(PullOnHost {
        base_branch_tip: "main-tip",
        head: "cart-head",
        ..PullOnHost::default()
    });

    let tips = host.pull_request_tips(&pr()).unwrap();

    assert_eq!(
        (tips.base_branch_tip, tips.head),
        (Sha::new("main-tip"), Sha::new("cart-head"))
    );
}

pub fn change_tips_of_a_host_not_answering_fail_as_an_outage(host: HostBuilder) {
    let host = host(PullOnHost {
        reach: Reach::ServerError,
        ..PullOnHost::default()
    });

    let Err(error) = host.pull_request_tips(&pr()) else {
        panic!("a host not answering gave the tips");
    };

    assert!(matches!(error, HostError::NotAnswering(..)), "{error:?}");
}
