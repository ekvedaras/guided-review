use guided_review::viewer::{InFlight, Reopen, ReopenAfterSave};

fn delivering() -> InFlight {
    InFlight {
        delivering_hand_offs: true,
    }
}

#[test]
fn a_save_with_nothing_in_flight_reopens_at_once() {
    let mut reopen = ReopenAfterSave::default();

    assert_eq!(reopen.saved(InFlight::default()), Reopen::Now);
    assert_eq!(reopen.waiting_line(), None);
    assert_eq!(
        reopen.work_in_flight_changed(InFlight::default()),
        Reopen::NotAsked
    );
}

#[test]
fn work_in_flight_with_no_save_asks_for_no_reopen() {
    let mut reopen = ReopenAfterSave::default();

    assert_eq!(
        reopen.work_in_flight_changed(delivering()),
        Reopen::NotAsked
    );
    assert_eq!(
        reopen.work_in_flight_changed(InFlight::default()),
        Reopen::NotAsked
    );
}

#[test]
fn a_save_while_hand_offs_are_delivered_waits_and_says_for_what() {
    let mut reopen = ReopenAfterSave::default();

    assert_eq!(reopen.saved(delivering()), Reopen::Waiting);
    assert_eq!(
        reopen.waiting_line(),
        Some("Saved. The PR opens again once the hand-offs are delivered to the tour agent.")
    );
}

#[test]
fn a_waiting_reopen_goes_once_the_work_ends() {
    let mut reopen = ReopenAfterSave::default();
    reopen.saved(delivering());

    assert_eq!(reopen.work_in_flight_changed(delivering()), Reopen::Waiting);

    assert_eq!(
        reopen.work_in_flight_changed(InFlight::default()),
        Reopen::Now
    );
    assert_eq!(reopen.waiting_line(), None);
    assert_eq!(
        reopen.work_in_flight_changed(InFlight::default()),
        Reopen::NotAsked
    );
}

#[test]
fn saving_again_while_a_reopen_waits_is_still_one_reopen() {
    let mut reopen = ReopenAfterSave::default();
    reopen.saved(delivering());

    assert_eq!(reopen.saved(delivering()), Reopen::Waiting);

    assert_eq!(
        reopen.work_in_flight_changed(InFlight::default()),
        Reopen::Now
    );
    assert_eq!(
        reopen.work_in_flight_changed(InFlight::default()),
        Reopen::NotAsked
    );
}
