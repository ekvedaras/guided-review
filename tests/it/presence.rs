use guided_review::viewer::{Closing, Motion, Presence, Shown};

fn leaving(presence: &mut Presence) -> Closing {
    presence.close(Motion::Full)
}

#[test]
fn a_new_presence_is_not_drawn() {
    let presence = Presence::default();

    assert!(!presence.is_open());
    assert_eq!(presence.shown(), None);
}

#[test]
fn an_opened_overlay_is_open_and_entering() {
    let mut presence = Presence::default();
    presence.open();

    assert!(presence.is_open());
    assert_eq!(presence.shown(), Some(Shown::Entering));
}

#[test]
fn closing_with_motion_keeps_it_drawn_but_closed_to_input() {
    let mut presence = Presence::default();
    presence.open();

    assert!(matches!(leaving(&mut presence), Closing::Leaving(_)));

    assert!(!presence.is_open());
    assert_eq!(presence.shown(), Some(Shown::Leaving));
}

#[test]
fn ending_the_leave_drops_it() {
    let mut presence = Presence::default();
    presence.open();
    let Closing::Leaving(ticket) = leaving(&mut presence) else {
        panic!("it should leave");
    };

    presence.end_leaving(ticket);

    assert_eq!(presence.shown(), None);
}

#[test]
fn with_reduced_motion_closing_drops_it_at_once() {
    let mut presence = Presence::default();
    presence.open();

    assert_eq!(presence.close(Motion::Reduced), Closing::AtOnce);

    assert_eq!(presence.shown(), None);
}

#[test]
fn closing_what_is_not_open_does_nothing() {
    let mut presence = Presence::default();
    assert_eq!(leaving(&mut presence), Closing::AtOnce);
    assert_eq!(presence.shown(), None);

    presence.open();
    leaving(&mut presence);
    assert_eq!(
        leaving(&mut presence),
        Closing::AtOnce,
        "a second close while leaving starts no second leave"
    );
    assert_eq!(presence.shown(), Some(Shown::Leaving));
}

#[test]
fn opening_during_the_leave_brings_it_back_and_the_old_leave_cannot_drop_it() {
    let mut presence = Presence::default();
    presence.open();
    let Closing::Leaving(old_leave) = leaving(&mut presence) else {
        panic!("it should leave");
    };

    presence.open();
    presence.end_leaving(old_leave);

    assert!(presence.is_open());
    assert_eq!(presence.shown(), Some(Shown::Entering));
}

#[test]
fn a_leave_cannot_drop_a_later_leave_of_a_later_opening() {
    let mut presence = Presence::default();
    presence.open();
    let Closing::Leaving(first) = leaving(&mut presence) else {
        panic!("it should leave");
    };
    presence.open();
    leaving(&mut presence);

    presence.end_leaving(first);

    assert_eq!(presence.shown(), Some(Shown::Leaving));
}

#[test]
fn opening_what_is_open_does_not_replay_its_move() {
    let mut presence = Presence::default();
    presence.open();
    let turn = presence.turn();

    presence.open();

    assert_eq!(presence.turn(), turn);
}

#[test]
fn every_opening_and_every_leave_has_its_own_turn() {
    let mut presence = Presence::default();
    presence.open();
    let opened = presence.turn();
    leaving(&mut presence);
    let leaving_turn = presence.turn();
    presence.open();
    let reopened = presence.turn();

    assert_ne!(opened, leaving_turn);
    assert_ne!(leaving_turn, reopened);
    assert_ne!(opened, reopened);
}
