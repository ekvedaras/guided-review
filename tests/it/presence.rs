use std::time::{Duration, Instant};

use guided_review::viewer::{Closing, Motion, Presence, Shown};

const TOTAL: Duration = Duration::from_millis(200);

fn presence() -> Presence {
    Presence::lasting(TOTAL)
}

fn after(start: Instant, millis: u64) -> Instant {
    start + Duration::from_millis(millis)
}

fn near(left: f32, right: f32) -> bool {
    (left - right).abs() < 0.001
}

fn leave(presence: &mut Presence, now: Instant) -> Closing {
    presence.close(Motion::Full, now)
}

#[test]
fn a_new_presence_is_not_drawn() {
    let presence = presence();

    assert!(!presence.is_open());
    assert_eq!(presence.shown(), None);
}

#[test]
fn an_opened_overlay_is_open_and_entering() {
    let mut presence = presence();
    presence.open(Instant::now());

    assert!(presence.is_open());
    assert_eq!(presence.shown(), Some(Shown::Entering));
}

#[test]
fn closing_with_motion_keeps_it_drawn_but_closed_to_input() {
    let (mut presence, now) = (presence(), Instant::now());
    presence.open(now);

    assert!(matches!(
        leave(&mut presence, after(now, 500)),
        Closing::Leaving { .. }
    ));

    assert!(!presence.is_open());
    assert_eq!(presence.shown(), Some(Shown::Leaving));
}

#[test]
fn ending_the_leave_drops_it() {
    let (mut presence, now) = (presence(), Instant::now());
    presence.open(now);
    let Closing::Leaving { ticket, .. } = leave(&mut presence, after(now, 500)) else {
        panic!("it should leave");
    };

    presence.end_leaving(ticket);

    assert_eq!(presence.shown(), None);
}

#[test]
fn with_reduced_motion_closing_drops_it_at_once() {
    let (mut presence, now) = (presence(), Instant::now());
    presence.open(now);

    assert_eq!(presence.close(Motion::Reduced, now), Closing::AtOnce);

    assert_eq!(presence.shown(), None);
}

#[test]
fn closing_what_is_not_open_does_nothing() {
    let (mut presence, now) = (presence(), Instant::now());
    assert_eq!(leave(&mut presence, now), Closing::AtOnce);
    assert_eq!(presence.shown(), None);

    presence.open(now);
    leave(&mut presence, after(now, 500));
    assert_eq!(
        leave(&mut presence, after(now, 600)),
        Closing::AtOnce,
        "a second close while leaving starts no second leave"
    );
    assert_eq!(presence.shown(), Some(Shown::Leaving));
}

#[test]
fn opening_during_the_leave_brings_it_back_and_the_old_leave_cannot_drop_it() {
    let (mut presence, now) = (presence(), Instant::now());
    presence.open(now);
    let Closing::Leaving { ticket, .. } = leave(&mut presence, after(now, 500)) else {
        panic!("it should leave");
    };

    presence.open(after(now, 520));
    presence.end_leaving(ticket);

    assert!(presence.is_open());
    assert_eq!(presence.shown(), Some(Shown::Entering));
}

#[test]
fn a_leave_cannot_drop_a_later_leave_of_a_later_opening() {
    let (mut presence, now) = (presence(), Instant::now());
    presence.open(now);
    let Closing::Leaving { ticket: first, .. } = leave(&mut presence, after(now, 500)) else {
        panic!("it should leave");
    };
    presence.open(after(now, 520));
    leave(&mut presence, after(now, 900));

    presence.end_leaving(first);

    assert_eq!(presence.shown(), Some(Shown::Leaving));
}

#[test]
fn opening_what_is_open_does_not_replay_its_move() {
    let (mut presence, now) = (presence(), Instant::now());
    presence.open(now);
    let turn = presence.turn();

    presence.open(after(now, 10));

    assert_eq!(presence.turn(), turn);
}

#[test]
fn every_opening_and_every_leave_has_its_own_turn() {
    let (mut presence, now) = (presence(), Instant::now());
    presence.open(now);
    let opened = presence.turn();
    leave(&mut presence, after(now, 500));
    let leaving_turn = presence.turn();
    presence.open(after(now, 510));
    let reopened = presence.turn();

    assert_ne!(opened, leaving_turn);
    assert_ne!(leaving_turn, reopened);
    assert_ne!(opened, reopened);
}

#[test]
fn a_first_opening_travels_the_whole_timeline() {
    let (mut presence, now) = (presence(), Instant::now());

    presence.open(now);

    let travel = presence.travel();
    assert!(near(travel.from, 0.) && near(travel.to, 1.));
    assert_eq!(travel.lasts, TOTAL);
}

#[test]
fn a_leave_of_a_settled_overlay_travels_the_whole_timeline_back() {
    let (mut presence, now) = (presence(), Instant::now());
    presence.open(now);

    let Closing::Leaving { lasts, .. } = leave(&mut presence, after(now, 500)) else {
        panic!("it should leave");
    };

    let travel = presence.travel();
    assert!(near(travel.from, 1.) && near(travel.to, 0.));
    assert_eq!(lasts, TOTAL);
}

#[test]
fn reopening_at_mid_leave_continues_from_that_point_and_takes_only_the_time_left() {
    let (mut presence, now) = (presence(), Instant::now());
    presence.open(now);
    leave(&mut presence, after(now, 500));

    presence.open(after(now, 550));

    let travel = presence.travel();
    assert!(near(travel.from, 0.75), "from {}", travel.from);
    assert!(near(travel.to, 1.));
    assert_eq!(travel.lasts, Duration::from_millis(50));
}

#[test]
fn closing_during_the_entrance_leaves_from_where_it_had_got_to() {
    let (mut presence, now) = (presence(), Instant::now());
    presence.open(now);

    let Closing::Leaving { lasts, .. } = leave(&mut presence, after(now, 50)) else {
        panic!("it should leave");
    };

    let travel = presence.travel();
    assert!(near(travel.from, 0.25), "from {}", travel.from);
    assert!(near(travel.to, 0.));
    assert_eq!(lasts, Duration::from_millis(50));
}

#[test]
fn reopening_after_the_leave_has_played_starts_from_nothing() {
    let (mut presence, now) = (presence(), Instant::now());
    presence.open(now);
    leave(&mut presence, after(now, 500));

    presence.open(after(now, 900));

    assert!(near(presence.travel().from, 0.));
    assert_eq!(presence.travel().lasts, TOTAL);
}

#[test]
fn the_position_follows_the_travel_with_time() {
    let (mut presence, now) = (presence(), Instant::now());
    presence.open(now);

    assert!(near(presence.position(after(now, 100)), 0.5));
    assert!(near(presence.position(after(now, 900)), 1.));
}
