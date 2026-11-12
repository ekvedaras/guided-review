use std::time::{Duration, Instant};

use guided_review::viewer::{Departure, InFlight, RestartGuard, RestartStep};

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

fn warning(step: RestartStep) -> String {
    match step {
        RestartStep::Warn(text) => text,
        RestartStep::Restart => panic!("it should warn"),
    }
}

#[test]
fn quitting_with_nothing_running_goes_at_once() {
    let mut guard = RestartGuard::default();

    assert_eq!(
        guard.ask_to(Departure::Quit, InFlight::default(), Instant::now()),
        RestartStep::Restart
    );
}

#[test]
fn quitting_with_work_running_warns_first_and_a_second_quit_soon_after_goes_ahead() {
    let mut guard = RestartGuard::default();
    let now = Instant::now();

    let text = warning(guard.ask_to(Departure::Quit, answering(), now));

    assert!(text.contains("Quitting"), "{text}");
    assert!(text.contains("cmd+q"), "{text}");
    assert_eq!(
        guard.ask_to(Departure::Quit, answering(), now + Duration::from_secs(3)),
        RestartStep::Restart
    );
}

#[test]
fn closing_the_window_with_a_hand_off_being_sent_warns_in_its_own_words() {
    let mut guard = RestartGuard::default();
    let sending = InFlight {
        asks_answering: 0,
        delivering_hand_offs: true,
    };

    let text = warning(guard.ask_to(Departure::CloseWindow, sending, Instant::now()));

    assert!(text.contains("cmd+w"), "{text}");
}

#[test]
fn another_way_out_does_not_confirm_a_warning() {
    let mut guard = RestartGuard::default();
    let now = Instant::now();
    guard.ask_to(Departure::Quit, answering(), now);

    assert!(matches!(
        guard.ask_to(
            Departure::CloseWindow,
            answering(),
            now + Duration::from_secs(2)
        ),
        RestartStep::Warn(_)
    ));
}

#[test]
fn a_second_quit_long_after_the_warning_warns_again() {
    let mut guard = RestartGuard::default();
    let now = Instant::now();
    guard.ask_to(Departure::Quit, answering(), now);

    assert!(matches!(
        guard.ask_to(Departure::Quit, answering(), now + Duration::from_secs(30)),
        RestartStep::Warn(_)
    ));
}
