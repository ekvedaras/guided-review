use guided_review::navigation::{Column, Word};
use guided_review::viewer::{HoveredName, LineLocation, NameHover, NameUnderPointer, Side};

use crate::support::{file_path, line};

fn on(word: &str, from: usize) -> NameUnderPointer {
    NameUnderPointer {
        name: HoveredName {
            at: LineLocation {
                path: file_path("src/cart.ts"),
                side: Side::Head,
                line: line(2),
            },
            column: Column::try_from(from).unwrap(),
            word: Word::new(word).unwrap(),
        },
        range: from..from + word.len(),
    }
}

fn shown_word(hover: &NameHover) -> Option<String> {
    hover.shown().map(|name| name.word.to_string())
}

#[test]
fn the_card_opens_when_the_pointer_rests_on_a_name() {
    let mut hover = NameHover::default();
    let ticket = hover.pointer_on(on("total", 4)).unwrap();
    assert_eq!(shown_word(&hover), None);
    let opened = hover.rested(ticket).unwrap();
    assert_eq!(opened.word.as_str(), "total");
    assert_eq!(shown_word(&hover).as_deref(), Some("total"));
}

#[test]
fn moving_on_to_another_name_cancels_the_rest_on_the_first() {
    let mut hover = NameHover::default();
    let first = hover.pointer_on(on("total", 4)).unwrap();
    let second = hover.pointer_on(on("price", 12)).unwrap();
    assert!(hover.rested(first).is_none());
    assert!(hover.rested(second).is_some());
    assert_eq!(shown_word(&hover).as_deref(), Some("price"));
}

#[test]
fn moving_within_one_name_does_not_restart_the_rest() {
    let mut hover = NameHover::default();
    let ticket = hover.pointer_on(on("total", 4)).unwrap();
    assert!(hover.pointer_on(on("total", 4)).is_none());
    assert!(hover.rested(ticket).is_some());
    assert!(hover.pointer_on(on("total", 4)).is_none());
    assert_eq!(shown_word(&hover).as_deref(), Some("total"));
}

#[test]
fn the_same_word_twice_in_a_line_is_two_names() {
    let mut hover = NameHover::default();
    hover.pointer_on(on("total", 4));
    assert!(hover.pointer_on(on("total", 20)).is_some());
}

#[test]
fn leaving_the_name_closes_the_card_after_a_grace_that_the_card_can_use() {
    let mut hover = NameHover::default();
    let ticket = hover.pointer_on(on("total", 4)).unwrap();
    hover.rested(ticket);

    let leaving = hover.pointer_off().unwrap();
    hover.card_entered();
    hover.grace_over(leaving);
    assert_eq!(shown_word(&hover).as_deref(), Some("total"));

    let leaving = hover.card_left().unwrap();
    hover.grace_over(leaving);
    assert_eq!(shown_word(&hover), None);
}

#[test]
fn leaving_for_nowhere_closes_the_card_when_the_grace_is_over() {
    let mut hover = NameHover::default();
    let ticket = hover.pointer_on(on("total", 4)).unwrap();
    hover.rested(ticket);
    let leaving = hover.pointer_off().unwrap();
    assert_eq!(shown_word(&hover).as_deref(), Some("total"));
    hover.grace_over(leaving);
    assert_eq!(shown_word(&hover), None);
}

#[test]
fn coming_back_to_the_name_within_the_grace_keeps_the_card() {
    let mut hover = NameHover::default();
    let ticket = hover.pointer_on(on("total", 4)).unwrap();
    hover.rested(ticket);
    let leaving = hover.pointer_off().unwrap();
    assert!(hover.pointer_on(on("total", 4)).is_none());
    hover.grace_over(leaving);
    assert_eq!(shown_word(&hover).as_deref(), Some("total"));
}

#[test]
fn another_name_closes_the_card_at_once_and_opens_its_own_after_a_rest() {
    let mut hover = NameHover::default();
    let ticket = hover.pointer_on(on("total", 4)).unwrap();
    hover.rested(ticket);
    let next = hover.pointer_on(on("price", 12)).unwrap();
    assert_eq!(shown_word(&hover), None);
    hover.rested(next);
    assert_eq!(shown_word(&hover).as_deref(), Some("price"));
}

#[test]
fn dismissing_keeps_the_card_closed_until_the_pointer_goes_to_another_name() {
    let mut hover = NameHover::default();
    let ticket = hover.pointer_on(on("total", 4)).unwrap();
    hover.rested(ticket);
    hover.dismiss();
    assert_eq!(shown_word(&hover), None);
    assert!(hover.pointer_on(on("total", 4)).is_none());
    assert!(hover.pointer_on(on("price", 12)).is_some());
}
