use std::path::Path;

use crate::support::folders_in;
use guided_review::viewer::{
    PrAuthor, PrSwitcher, RelevantPullRequest, RelevantPullRequests, ScreenLine, WhyListed,
};

fn relevant(url: &str) -> RelevantPullRequest {
    RelevantPullRequest {
        url: url.parse().unwrap(),
        title: format!("Change {url}"),
        author: PrAuthor {
            login: "alice".into(),
            avatar_url: None,
        },
        is_draft: false,
        updated_at: "2026-11-06T07:00:00Z".to_owned(),
        why_listed: WhyListed::Involved,
    }
}

fn listing<'a>(urls: impl IntoIterator<Item = &'a str>) -> PrSwitcher {
    let mut switcher = PrSwitcher::default();
    switcher.loaded(
        Ok(RelevantPullRequests {
            pull_requests: urls.into_iter().map(relevant).collect(),
            teams_left_out: Vec::new(),
        }),
        &folders_in(Path::new("/nowhere")),
    );
    switcher
}

fn visible_urls(switcher: &PrSwitcher) -> Vec<String> {
    switcher
        .visible()
        .map(|row| row.pull_request().unwrap().url.short())
        .collect()
}

fn heading(repo: &str, rows: usize) -> ScreenLine {
    ScreenLine::Heading {
        name: repo.to_owned(),
        rows,
    }
}

#[test]
fn rows_of_a_repo_are_together_and_the_repos_come_in_the_order_of_their_newest_row() {
    let switcher = listing([
        "https://github.com/acme/shop/pull/7",
        "https://github.com/acme/blog/pull/3",
        "https://github.com/acme/shop/pull/8",
        "https://github.com/acme/api/pull/31",
        "https://github.com/acme/blog/pull/9",
    ]);

    assert_eq!(
        visible_urls(&switcher),
        [
            "acme/shop#7",
            "acme/shop#8",
            "acme/blog#3",
            "acme/blog#9",
            "acme/api#31"
        ]
    );
}

#[test]
fn the_screen_names_each_repo_once_above_its_rows() {
    let switcher = listing([
        "https://github.com/acme/shop/pull/7",
        "https://github.com/acme/blog/pull/3",
        "https://github.com/acme/shop/pull/8",
    ]);

    assert_eq!(
        switcher.screen(),
        [
            heading("acme/shop", 2),
            ScreenLine::Row(0),
            ScreenLine::Row(1),
            heading("acme/blog", 1),
            ScreenLine::Row(2),
        ]
    );
}

fn twelve_of_one_repo() -> PrSwitcher {
    let urls: Vec<String> = (1..=12)
        .map(|number| format!("https://github.com/acme/shop/pull/{number}"))
        .collect();
    listing(urls.iter().map(String::as_str))
}

#[test]
fn scrolled_down_the_top_row_still_names_its_repo() {
    let mut switcher = twelve_of_one_repo();

    switcher.scroll_by(2);

    let expected: Vec<_> = std::iter::once(heading("acme/shop", 12))
        .chain((2..10).map(ScreenLine::Row))
        .collect();
    assert_eq!(switcher.screen(), expected);
}

#[test]
fn repo_names_take_room_so_fewer_rows_fit_when_every_row_is_of_another_repo() {
    let urls: Vec<String> = (1..=12)
        .map(|number| format!("https://github.com/acme/repo-{number}/pull/1"))
        .collect();
    let mut switcher = listing(urls.iter().map(String::as_str));
    assert_eq!(switcher.rows_on_screen(), 5);

    for _ in 0..5 {
        switcher.select_next();
    }

    assert_eq!((switcher.selected(), switcher.first_shown()), (5, 1));
    switcher.scroll_by(99);
    assert_eq!(switcher.first_shown(), 7, "the last row ends the screen");
}
