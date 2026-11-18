use std::path::Path;

use crate::support::folders_in;
use guided_review::viewer::{
    LineAnchor, ListLine, PrAuthor, PrSwitcher, RelevantPullRequest, RelevantPullRequests,
    WhyListed,
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

fn heading(repo: &str, rows: usize) -> ListLine {
    ListLine::Heading {
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
fn the_list_names_each_repo_once_above_its_rows() {
    let switcher = listing([
        "https://github.com/acme/shop/pull/7",
        "https://github.com/acme/blog/pull/3",
        "https://github.com/acme/shop/pull/8",
    ]);

    assert_eq!(
        switcher.lines(),
        [
            heading("acme/shop", 2),
            ListLine::Row(0),
            ListLine::Row(1),
            heading("acme/blog", 1),
            ListLine::Row(2),
        ]
    );
}

#[test]
fn the_switcher_says_it_looks_in_local_folders_until_they_are_read() {
    let mut switcher = PrSwitcher::default();
    switcher.refreshing();
    assert!(switcher.is_finding_local());

    switcher.local_found(Vec::new(), &folders_in(Path::new("/nowhere")));

    assert!(!switcher.is_finding_local());
}

#[test]
fn a_row_is_found_again_on_its_new_line_when_rows_come_in_above_it() {
    let mut switcher = listing([
        "https://github.com/acme/shop/pull/7",
        "https://github.com/acme/blog/pull/3",
    ]);
    let line = switcher.line_of_row(1).unwrap();
    let anchor = switcher.anchor_at(line).unwrap();

    switcher.loaded(
        Ok(RelevantPullRequests {
            pull_requests: [
                "https://github.com/acme/shop/pull/9",
                "https://github.com/acme/shop/pull/7",
                "https://github.com/acme/blog/pull/3",
            ]
            .into_iter()
            .map(relevant)
            .collect(),
            teams_left_out: Vec::new(),
        }),
        &folders_in(Path::new("/nowhere")),
    );

    let now = switcher.line_after_change(&anchor, line);
    assert_eq!(switcher.lines()[now], ListLine::Row(2));
    assert_eq!(
        anchor,
        LineAnchor::Row("https://github.com/acme/blog/pull/3".parse().unwrap())
    );
}

#[test]
fn a_repo_name_is_found_again_and_a_line_that_left_stays_where_it_was() {
    let mut switcher = listing([
        "https://github.com/acme/shop/pull/7",
        "https://github.com/acme/blog/pull/3",
        "https://github.com/acme/blog/pull/4",
    ]);
    let blog = switcher.anchor_at(2).unwrap();
    let gone = switcher.anchor_at(1).unwrap();
    assert_eq!(blog, LineAnchor::Heading("acme/blog".to_owned()));

    switcher.loaded(
        Ok(RelevantPullRequests {
            pull_requests: [
                "https://github.com/acme/blog/pull/3",
                "https://github.com/acme/blog/pull/4",
            ]
            .into_iter()
            .map(relevant)
            .collect(),
            teams_left_out: Vec::new(),
        }),
        &folders_in(Path::new("/nowhere")),
    );

    assert_eq!(switcher.line_after_change(&blog, 2), 0);
    assert_eq!(switcher.line_after_change(&gone, 1), 1);
}
