use std::time::{Duration, Instant};

use guided_review::viewer::{InFlight, RestartGuard, RestartStep};

fn answering() -> InFlight {
    InFlight {
        asks_answering: 1,
        delivering_hand_offs: false,
    }
}

#[test]
fn with_nothing_running_a_restart_goes_at_once() {
    let mut guard = RestartGuard::default();

    assert_eq!(
        guard.ask(InFlight::default(), Instant::now()),
        RestartStep::Restart
    );
}

#[test]
fn with_work_running_the_first_ask_warns_and_a_second_soon_after_restarts() {
    let mut guard = RestartGuard::default();
    let now = Instant::now();

    assert!(matches!(guard.ask(answering(), now), RestartStep::Warn(_)));
    assert_eq!(
        guard.ask(answering(), now + Duration::from_secs(3)),
        RestartStep::Restart
    );
}

#[test]
fn a_second_ask_long_after_the_warning_warns_again() {
    let mut guard = RestartGuard::default();
    let now = Instant::now();
    guard.ask(answering(), now);

    assert!(matches!(
        guard.ask(answering(), now + Duration::from_secs(30)),
        RestartStep::Warn(_)
    ));
}
