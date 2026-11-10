use guided_review::viewer::{InFlight, Reopen, ReopenAfterSave};

fn asks_answering(count: u16) -> InFlight {
    InFlight {
        asks_answering: count,
        ..InFlight::default()
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
        reopen.work_in_flight_changed(asks_answering(1)),
        Reopen::NotAsked
    );
    assert_eq!(
        reopen.work_in_flight_changed(InFlight::default()),
        Reopen::NotAsked
    );
}

#[test]
fn a_save_while_asks_answer_waits_and_says_for_what() {
    let mut reopen = ReopenAfterSave::default();

    assert_eq!(reopen.saved(asks_answering(2)), Reopen::Waiting);
    assert_eq!(
        reopen.waiting_line(),
        Some("Saved. The PR opens again once 2 Asks finish answering.")
    );
}

#[test]
fn the_waiting_line_names_every_kind_of_work_the_reopen_waits_for() {
    let mut reopen = ReopenAfterSave::default();

    reopen.ask_handed_to_agent();
    reopen.ask_handed_to_agent();
    reopen.saved(InFlight {
        asks_answering: 1,
        delivering_hand_offs: true,
    });

    assert_eq!(
        reopen.waiting_line(),
        Some(
            "Saved. The PR opens again once the Ask finishes answering, 2 Asks are handed to your agent and the hand-offs are delivered to the tour agent."
        )
    );
}

#[test]
fn the_waiting_line_for_one_ask_handed_or_hand_offs_alone() {
    let mut reopen = ReopenAfterSave::default();

    reopen.ask_handed_to_agent();
    reopen.saved(InFlight::default());
    assert_eq!(
        reopen.waiting_line(),
        Some("Saved. The PR opens again once the Ask is handed to your agent.")
    );

    reopen.ask_hand_off_ended(InFlight {
        delivering_hand_offs: true,
        ..InFlight::default()
    });
    assert_eq!(
        reopen.waiting_line(),
        Some("Saved. The PR opens again once the hand-offs are delivered to the tour agent.")
    );
}

#[test]
fn a_waiting_reopen_goes_once_the_work_ends() {
    let mut reopen = ReopenAfterSave::default();
    reopen.saved(asks_answering(2));

    assert_eq!(
        reopen.work_in_flight_changed(asks_answering(1)),
        Reopen::Waiting
    );
    assert_eq!(
        reopen.waiting_line(),
        Some("Saved. The PR opens again once the Ask finishes answering.")
    );

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
    reopen.saved(asks_answering(1));

    assert_eq!(reopen.saved(asks_answering(1)), Reopen::Waiting);

    assert_eq!(
        reopen.work_in_flight_changed(InFlight::default()),
        Reopen::Now
    );
    assert_eq!(
        reopen.work_in_flight_changed(InFlight::default()),
        Reopen::NotAsked
    );
}

#[test]
fn an_ask_handed_to_the_agent_holds_a_waiting_reopen_until_its_hand_off_ends() {
    let mut reopen = ReopenAfterSave::default();
    reopen.ask_handed_to_agent();
    reopen.ask_handed_to_agent();
    reopen.saved(InFlight::default());

    assert_eq!(
        reopen.ask_hand_off_ended(InFlight::default()),
        Reopen::Waiting
    );
    assert_eq!(
        reopen.waiting_line(),
        Some("Saved. The PR opens again once the Ask is handed to your agent.")
    );
    assert_eq!(reopen.ask_hand_off_ended(InFlight::default()), Reopen::Now);
    assert_eq!(reopen.waiting_line(), None);
}

#[test]
fn a_hand_off_that_ends_with_no_save_asks_for_no_reopen_and_never_counts_below_none() {
    let mut reopen = ReopenAfterSave::default();
    reopen.ask_handed_to_agent();

    assert_eq!(
        reopen.ask_hand_off_ended(InFlight::default()),
        Reopen::NotAsked
    );
    assert_eq!(
        reopen.ask_hand_off_ended(InFlight::default()),
        Reopen::NotAsked
    );
    assert_eq!(reopen.saved(InFlight::default()), Reopen::Now);
}
