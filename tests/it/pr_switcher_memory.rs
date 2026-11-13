use std::fs;
use std::path::Path;

use guided_review::viewer::{
    PrSwitcher, RelevantPullRequest, RelevantPullRequests, RememberedPullRequestsFile, WhyListed,
};
use tempfile::TempDir;

fn relevant(number: u32) -> RelevantPullRequest {
    RelevantPullRequest {
        url: format!("https://github.com/acme/shop/pull/{number}")
            .parse()
            .unwrap(),
        title: format!("Change {number}"),
        author: "alice".to_owned(),
        is_draft: number == 2,
        updated_at: "2026-11-06T07:00:00Z".to_owned(),
        why_listed: WhyListed::ReviewRequested,
    }
}

fn listing(numbers: impl IntoIterator<Item = u32>) -> RelevantPullRequests {
    RelevantPullRequests {
        pull_requests: numbers.into_iter().map(relevant).collect(),
        teams_left_out: Vec::new(),
    }
}

fn opened(home: &Path) -> PrSwitcher {
    PrSwitcher::remembering_in(RememberedPullRequestsFile::in_home(home), home)
}

fn visible_numbers(switcher: &PrSwitcher) -> Vec<u64> {
    switcher
        .visible()
        .map(|row| row.pull_request.url.number)
        .collect()
}

fn number_on_screen_row(switcher: &PrSwitcher, screen_row: usize) -> u64 {
    switcher
        .visible()
        .nth(switcher.first_shown() + screen_row)
        .unwrap()
        .pull_request
        .url
        .number
}

#[test]
fn the_list_read_before_a_restart_shows_at_once_after_it_while_it_refreshes() {
    let home = TempDir::new().unwrap();
    let mut before = opened(home.path());
    before.loaded(Ok(listing([3, 2, 1])), home.path());

    let after = opened(home.path());

    assert_eq!(visible_numbers(&after), vec![3, 2, 1]);
    let draft = after.visible().nth(1).unwrap();
    assert_eq!(draft.pull_request, relevant(2));
    assert!(!after.is_loading());
    assert!(after.is_refreshing());
}

#[test]
fn with_no_list_remembered_the_switcher_starts_loading() {
    let home = TempDir::new().unwrap();

    let switcher = opened(home.path());

    assert!(switcher.is_loading());
    assert_eq!(switcher.notice(), None);
}

#[test]
fn a_failed_read_leaves_the_remembered_list_as_it_was() {
    let home = TempDir::new().unwrap();
    opened(home.path()).loaded(Ok(listing([3, 2, 1])), home.path());

    opened(home.path()).loaded(Err(anyhow::anyhow!("network is down").into()), home.path());

    assert_eq!(visible_numbers(&opened(home.path())), vec![3, 2, 1]);
}

#[test]
fn a_remembered_list_that_cannot_be_read_is_moved_aside_and_said() {
    let home = TempDir::new().unwrap();
    let state = home.path().join(".guided-review/state");
    fs::create_dir_all(&state).unwrap();
    fs::write(state.join("pull-requests.json"), "not json").unwrap();

    let switcher = opened(home.path());

    assert!(switcher.is_loading());
    assert!(
        switcher
            .notice()
            .unwrap()
            .contains("pull-requests.broken.json")
    );
    assert!(state.join("pull-requests.broken.json").exists());
}

#[test]
fn the_hovered_pull_request_keeps_its_place_on_screen_when_new_ones_come_in_above() {
    let home = TempDir::new().unwrap();
    let mut switcher = opened(home.path());
    switcher.loaded(Ok(listing((1..=20).rev())), home.path());
    switcher.scroll_by(3);
    switcher.hover(switcher.first_shown() + 4);
    let hovered = number_on_screen_row(&switcher, 4);

    switcher.refreshing();
    switcher.loaded(Ok(listing((1..=22).rev())), home.path());

    assert_eq!(number_on_screen_row(&switcher, 4), hovered);
}

#[test]
fn with_no_hover_the_selected_pull_request_keeps_its_place_on_screen_and_stays_selected() {
    let home = TempDir::new().unwrap();
    let mut switcher = opened(home.path());
    switcher.loaded(Ok(listing((1..=20).rev())), home.path());
    switcher.select(2);
    let selected = number_on_screen_row(&switcher, 2);

    switcher.loaded(Ok(listing((1..=23).rev())), home.path());

    assert_eq!(number_on_screen_row(&switcher, 2), selected);
    let now_selected = switcher.visible().nth(switcher.selected()).unwrap();
    assert_eq!(now_selected.pull_request.url.number, selected);
}

#[test]
fn the_selection_follows_its_pull_request_while_the_hovered_one_keeps_its_place() {
    let home = TempDir::new().unwrap();
    let mut switcher = opened(home.path());
    switcher.loaded(Ok(listing((1..=20).rev())), home.path());
    switcher.select(1);
    switcher.hover(5);
    let (selected, hovered) = (
        number_on_screen_row(&switcher, 1),
        number_on_screen_row(&switcher, 5),
    );

    switcher.loaded(Ok(listing((1..=21).rev())), home.path());

    assert_eq!(number_on_screen_row(&switcher, 5), hovered);
    let now_selected = switcher.visible().nth(switcher.selected()).unwrap();
    assert_eq!(now_selected.pull_request.url.number, selected);
}

#[test]
fn a_hovered_pull_request_near_the_top_cannot_scroll_above_the_first_row() {
    let home = TempDir::new().unwrap();
    let mut switcher = opened(home.path());
    switcher.loaded(Ok(listing((1..=20).rev())), home.path());
    switcher.hover(0);

    switcher.loaded(Ok(listing((1..=19).rev())), home.path());

    assert_eq!(switcher.first_shown(), 0);
}

#[test]
fn when_the_hovered_pull_request_leaves_the_list_the_rows_stay_where_they_were() {
    let home = TempDir::new().unwrap();
    let mut switcher = opened(home.path());
    switcher.loaded(Ok(listing((1..=20).rev())), home.path());
    switcher.scroll_by(4);
    switcher.hover(switcher.first_shown() + 6);
    let gone = number_on_screen_row(&switcher, 6);

    switcher.loaded(
        Ok(listing(
            (1..=20).rev().filter(|&number| u64::from(number) != gone),
        )),
        home.path(),
    );

    assert_eq!(switcher.first_shown(), 4);
}

#[test]
fn leaving_a_row_after_entering_the_next_keeps_the_next_one_hovered() {
    let home = TempDir::new().unwrap();
    let mut switcher = opened(home.path());
    switcher.loaded(Ok(listing((1..=20).rev())), home.path());
    switcher.select(9);
    switcher.hover(3);
    switcher.hover(4);
    switcher.unhover(3);
    let hovered = number_on_screen_row(&switcher, 4);

    switcher.loaded(Ok(listing((1..=22).rev())), home.path());

    assert_eq!(number_on_screen_row(&switcher, 4), hovered);
}

#[test]
fn the_hovered_pull_request_is_the_one_under_the_pointer_after_the_wheel_moved_the_rows() {
    let home = TempDir::new().unwrap();
    let mut switcher = opened(home.path());
    switcher.loaded(Ok(listing((1..=20).rev())), home.path());
    switcher.hover(2);
    switcher.scroll_by(5);
    let under_pointer = number_on_screen_row(&switcher, 2);

    switcher.loaded(Ok(listing((1..=22).rev())), home.path());

    assert_eq!(number_on_screen_row(&switcher, 2), under_pointer);
}
