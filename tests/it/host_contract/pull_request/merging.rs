use guided_review::viewer::{
    AutoMerge, CodeHost, HostError, MergeChange, MergeMethod, MergeMethods, Mergeability, Merging,
    MergingPort, PrStatus,
};

use crate::host_contract::{HostBuilder, PullOnHost, Reach, pr};
use crate::support::fake_ports::blocked_merging;

fn merging_of(host: &dyn CodeHost) -> &dyn MergingPort {
    host.merging().expect("the host offers merging")
}

pub fn merging_reads_as_the_host_has_it(host: HostBuilder) {
    let mergeable_on_squash = Merging {
        mergeability: Mergeability::Now,
        auto_merge: AutoMerge::On(MergeMethod::Squash),
        methods: MergeMethods::only(&[MergeMethod::Squash, MergeMethod::Rebase]),
        repo_allows_auto_merge: true,
        viewer_may_enable_auto_merge: false,
        viewer_may_disable_auto_merge: true,
        viewer_may_merge: false,
    };
    for merging in [mergeable_on_squash, blocked_merging()] {
        let host = host(PullOnHost {
            merging,
            ..PullOnHost::default()
        });

        assert_eq!(
            merging_of(host.as_ref())
                .pull_request_merging(&pr())
                .unwrap(),
            merging
        );
    }
}

pub fn auto_merge_turned_on_and_off_reads_back_and_a_merge_now_merges_the_pr(host: HostBuilder) {
    let host = host(PullOnHost {
        merging: blocked_merging(),
        ..PullOnHost::default()
    });
    let merging = merging_of(host.as_ref());
    let auto_merge = || merging.pull_request_merging(&pr()).unwrap().auto_merge;

    merging
        .change_merging(&pr(), MergeChange::EnableAutoMerge(MergeMethod::Squash))
        .unwrap();
    let on = auto_merge();
    merging
        .change_merging(&pr(), MergeChange::DisableAutoMerge)
        .unwrap();
    let off = auto_merge();
    merging
        .change_merging(&pr(), MergeChange::MergeNow(MergeMethod::Merge))
        .unwrap();

    assert_eq!(
        (on, off),
        (AutoMerge::On(MergeMethod::Squash), AutoMerge::Off)
    );
    assert_eq!(host.pull_request_status(&pr()).unwrap(), PrStatus::Merged);
}

pub fn merging_of_a_host_not_answering_fails_as_an_outage(host: HostBuilder) {
    let host = host(PullOnHost {
        reach: Reach::ServerError,
        ..PullOnHost::default()
    });
    let merging = merging_of(host.as_ref());

    let read = merging.pull_request_merging(&pr()).unwrap_err();
    let write = merging
        .change_merging(&pr(), MergeChange::DisableAutoMerge)
        .unwrap_err();

    for error in [read, write] {
        assert!(matches!(error, HostError::NotAnswering(..)), "{error:?}");
    }
}
