use std::sync::Arc;

use guided_review::viewer::{
    CheckRun, CheckState, Checks, HostError, OpenPrStatus, PrStatus, Readiness,
};

use crate::host_contract::{HostBuilder, PullOnHost, Reach, open, pr};

fn readiness_of(status: &PrStatus) -> Option<Readiness> {
    status.open().map(|open| open.readiness)
}

pub fn the_status_reads_as_the_host_has_it(host: HostBuilder) {
    let draft_with_checks = PrStatus::Open(OpenPrStatus {
        readiness: Readiness::Draft,
        checks: Checks::Failing,
        check_runs: Arc::new([
            CheckRun {
                name: "build".into(),
                state: CheckState::Failing,
                page: Some("https://ci.example/build/1".into()),
            },
            CheckRun {
                name: "lint".into(),
                state: CheckState::Passing,
                page: None,
            },
            CheckRun {
                name: "deploy".into(),
                state: CheckState::Running,
                page: None,
            },
        ]),
        viewer_may_change_readiness: false,
    });
    for status in [
        draft_with_checks,
        open(Readiness::Ready),
        PrStatus::Merged,
        PrStatus::Closed,
    ] {
        let host = host(PullOnHost {
            status: status.clone(),
            ..PullOnHost::default()
        });

        assert_eq!(host.pull_request_status(&pr()).unwrap(), status);
    }
}

pub fn a_readiness_set_reads_back(host: HostBuilder) {
    let host = host(PullOnHost {
        status: open(Readiness::Ready),
        ..PullOnHost::default()
    });

    host.set_pull_request_readiness(&pr(), Readiness::Draft)
        .unwrap();
    let draft = readiness_of(&host.pull_request_status(&pr()).unwrap());
    host.set_pull_request_readiness(&pr(), Readiness::Ready)
        .unwrap();
    let ready = readiness_of(&host.pull_request_status(&pr()).unwrap());

    assert_eq!(
        (draft, ready),
        (Some(Readiness::Draft), Some(Readiness::Ready))
    );
}

pub fn the_status_without_a_token_is_refused(host: HostBuilder) {
    let host = host(PullOnHost {
        reach: Reach::NoToken,
        ..PullOnHost::default()
    });

    let read = host.pull_request_status(&pr()).unwrap_err();
    let write = host
        .set_pull_request_readiness(&pr(), Readiness::Draft)
        .unwrap_err();

    for error in [read, write] {
        assert!(matches!(error, HostError::NoAccess(_)), "{error:?}");
    }
}

pub fn the_status_of_a_host_not_answering_fails_as_an_outage(host: HostBuilder) {
    let host = host(PullOnHost {
        reach: Reach::ServerError,
        ..PullOnHost::default()
    });

    let read = host.pull_request_status(&pr()).unwrap_err();
    let write = host
        .set_pull_request_readiness(&pr(), Readiness::Draft)
        .unwrap_err();

    for error in [read, write] {
        assert!(matches!(error, HostError::NotAnswering(..)), "{error:?}");
    }
}
