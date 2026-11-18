use crate::support::folders_in;
use std::fs;
use std::path::Path;

use guided_review::viewer::{
    PrAuthor, PrSwitcher, RelevantPullRequest, RelevantPullRequests, RememberedPullRequestsFile,
    WhyListed,
};
use tempfile::TempDir;

fn relevant(number: u32) -> RelevantPullRequest {
    RelevantPullRequest {
        url: format!("https://github.com/acme/shop/pull/{number}")
            .parse()
            .unwrap(),
        title: format!("Change {number}"),
        author: PrAuthor {
            login: "alice".into(),
            avatar_url: Some("https://avatars.githubusercontent.com/alice?s=64".into()),
        },
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
    PrSwitcher::remembering_in(
        RememberedPullRequestsFile::in_folders(&folders_in(home)),
        &folders_in(home),
    )
}

fn visible_numbers(switcher: &PrSwitcher) -> Vec<u64> {
    switcher
        .visible()
        .map(|row| row.pull_request().unwrap().url.number)
        .collect()
}

#[test]
fn the_list_read_before_a_restart_shows_at_once_after_it_while_it_refreshes() {
    let home = TempDir::new().unwrap();
    let mut before = opened(home.path());
    before.loaded(Ok(listing([3, 2, 1])), &folders_in(home.path()));

    let after = opened(home.path());

    assert_eq!(visible_numbers(&after), vec![3, 2, 1]);
    let draft = after.visible().nth(1).unwrap();
    assert_eq!(draft.pull_request(), Some(&relevant(2)));
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
    opened(home.path()).loaded(Ok(listing([3, 2, 1])), &folders_in(home.path()));

    opened(home.path()).loaded(
        Err(anyhow::anyhow!("network is down").into()),
        &folders_in(home.path()),
    );

    assert_eq!(visible_numbers(&opened(home.path())), vec![3, 2, 1]);
}

#[test]
fn a_remembered_list_that_cannot_be_read_is_moved_aside_and_said() {
    let home = TempDir::new().unwrap();
    let state = home.path().join(".local/state/guided-review/state");
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
fn the_selected_pull_request_stays_selected_when_new_ones_come_in_above() {
    let home = TempDir::new().unwrap();
    let mut switcher = opened(home.path());
    switcher.loaded(Ok(listing((1..=20).rev())), &folders_in(home.path()));
    switcher.select(2);
    let selected = visible_numbers(&switcher)[2];

    switcher.loaded(Ok(listing((1..=23).rev())), &folders_in(home.path()));

    assert_eq!(visible_numbers(&switcher)[switcher.selected()], selected);
}

#[test]
fn leaving_a_row_after_entering_the_next_keeps_the_next_one_hovered() {
    let home = TempDir::new().unwrap();
    let mut switcher = opened(home.path());
    switcher.loaded(Ok(listing((1..=20).rev())), &folders_in(home.path()));
    switcher.hover(3);
    switcher.hover(4);
    switcher.unhover(3);

    assert_eq!(switcher.hovered(), Some(4));
}
