use std::fs;
use std::path::Path;

use crate::support::fake_ports::{FakeGitHub, GitHubFault, SentLog};
use guided_review::pull_request::PullRequestUrl;
use guided_review::tour::Tour;
use guided_review::viewer::{
    ActivePr, NotPickable, Pick, PrSwitcher, RelevantPullRequest, RelevantPullRequests,
    RelevantPullRequestsPort, TourFact, WhyListed,
};
use tempfile::TempDir;

const SHOP_7: &str = "https://github.com/acme/shop/pull/7";
const BLOG_7: &str = "https://github.com/acme/blog/pull/7";
const API_31: &str = "https://github.com/acme/api/pull/31";

fn pr(url: &str) -> PullRequestUrl {
    url.parse().unwrap()
}

fn relevant(url: &str, title: &str, author: &str, why: WhyListed) -> RelevantPullRequest {
    RelevantPullRequest {
        url: pr(url),
        title: title.to_owned(),
        author: author.to_owned(),
        is_draft: false,
        updated_at: "2026-11-06T07:00:00Z".to_owned(),
        why_listed: why,
    }
}

fn three_pull_requests() -> FakeGitHub {
    let mut draft = relevant(BLOG_7, "Fix feed", "ekvedaras", WhyListed::Mine);
    draft.is_draft = true;
    FakeGitHub::with_pull("a", "b", &SentLog::default()).with_relevant_pull_requests(
        RelevantPullRequests {
            pull_requests: vec![
                draft,
                relevant(API_31, "Rate limits", "dave", WhyListed::ReviewRequested),
                relevant(SHOP_7, "Add checkout", "alice", WhyListed::Involved),
            ],
            teams_left_out: Vec::new(),
        },
    )
}

fn write_tour(home: &Path, url: &str) {
    let path = Tour::path_for(home, &pr(url));
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, "{}").unwrap();
}

fn home_with_tour_of(url: &str) -> TempDir {
    let home = TempDir::new().unwrap();
    write_tour(home.path(), url);
    home
}

fn loaded(github: &FakeGitHub, home: &Path) -> PrSwitcher {
    let mut switcher = PrSwitcher::default();
    switcher.loaded(github.relevant_pull_requests(), home);
    switcher
}

fn visible_urls(switcher: &PrSwitcher) -> Vec<String> {
    switcher
        .visible()
        .map(|row| row.pull_request.url.to_string())
        .collect()
}

#[test]
fn a_row_carries_what_the_reviewer_needs_to_pick_without_opening() {
    let home = home_with_tour_of(API_31);

    let switcher = loaded(&three_pull_requests(), home.path());

    let rows: Vec<_> = switcher.visible().collect();
    let [feed, limits, _] = rows.as_slice() else {
        panic!("expected three rows");
    };
    assert_eq!(feed.pull_request.url.short(), "acme/blog#7");
    assert_eq!(feed.pull_request.title, "Fix feed");
    assert!(feed.pull_request.is_draft);
    assert_eq!(feed.pull_request.why_listed, WhyListed::Mine);
    assert_eq!(limits.pull_request.author, "dave");
}

#[test]
fn a_row_says_whether_the_pull_request_has_a_tour() {
    let home = home_with_tour_of(API_31);

    let switcher = loaded(&three_pull_requests(), home.path());

    let tours: Vec<_> = switcher.visible().map(|row| row.tour).collect();
    assert_eq!(
        tours,
        [TourFact::Missing, TourFact::Ready, TourFact::Missing]
    );
}

#[test]
fn a_tour_written_since_the_list_loaded_shows_when_the_tours_are_checked_again() {
    let home = TempDir::new().unwrap();
    let mut switcher = loaded(&three_pull_requests(), home.path());
    write_tour(home.path(), SHOP_7);

    switcher.check_tours(home.path(), &ActivePr::default());

    let tours: Vec<_> = switcher.visible().map(|row| row.tour).collect();
    assert_eq!(
        tours,
        [TourFact::Missing, TourFact::Missing, TourFact::Ready]
    );
}

#[test]
fn typing_filters_by_repo_number_title_and_author_ignoring_case() {
    let home = TempDir::new().unwrap();
    let mut switcher = loaded(&three_pull_requests(), home.path());

    switcher.filter("ALICE");
    assert_eq!(visible_urls(&switcher), [SHOP_7]);

    switcher.filter("blog 7");
    assert_eq!(visible_urls(&switcher), [BLOG_7]);

    switcher.filter("rate");
    assert_eq!(visible_urls(&switcher), [API_31]);

    switcher.filter("");
    assert_eq!(visible_urls(&switcher), [BLOG_7, API_31, SHOP_7]);
}

#[test]
fn picking_a_pull_request_with_a_tour_switches_to_it() {
    let home = home_with_tour_of(API_31);
    let switcher = loaded(&three_pull_requests(), home.path());

    assert_eq!(switcher.pick(1).unwrap(), Pick::Show(pr(API_31)));
}

#[test]
fn picking_a_pull_request_with_no_tour_writes_it() {
    let home = TempDir::new().unwrap();
    let switcher = loaded(&three_pull_requests(), home.path());

    assert_eq!(switcher.pick(0).unwrap(), Pick::Write(pr(BLOG_7)));
    assert_eq!(
        switcher.tour_label(switcher.visible().next().unwrap()),
        "no tour yet"
    );
}

#[test]
fn without_herdr_a_pull_request_with_no_tour_says_writing_one_needs_herdr() {
    let home = home_with_tour_of(API_31);
    let mut switcher = loaded(&three_pull_requests(), home.path());

    switcher.agent_link(Err("`hc` is not on PATH".to_owned()));

    let refusal = switcher.pick(0).unwrap_err().to_string();
    assert!(
        refusal.contains("needs herdr") && refusal.contains("not on PATH"),
        "{refusal}"
    );
    assert_eq!(switcher.pick(1).unwrap(), Pick::Show(pr(API_31)));
    let rows: Vec<_> = switcher.visible().collect();
    let [no_tour, with_tour, _] = rows.as_slice() else {
        panic!("three rows");
    };
    assert_eq!(switcher.tour_label(no_tour), "needs herdr");
    assert_eq!(switcher.tour_label(with_tour), "tour");
}

#[test]
fn rows_say_which_tours_are_being_written_failed_or_have_an_unseen_agent_reply() {
    let home = home_with_tour_of(API_31);
    let mut switcher = loaded(&three_pull_requests(), home.path());
    let mut active = ActivePr::default();
    active.write(pr(BLOG_7));
    active.write(pr(SHOP_7));
    active.writer_started(&pr(SHOP_7), Err(anyhow::anyhow!("no room")));
    active.agent_replied(&pr(API_31));

    switcher.check_tours(home.path(), &active);

    let rows: Vec<_> = switcher.visible().collect();
    let tours: Vec<_> = rows.iter().map(|row| row.tour).collect();
    assert_eq!(
        tours,
        [TourFact::Writing, TourFact::Ready, TourFact::WritingFailed]
    );
    let labels: Vec<_> = rows.iter().map(|row| switcher.tour_label(row)).collect();
    assert_eq!(labels, ["writing", "tour", "writing failed"]);
    let unseen: Vec<_> = rows.iter().map(|row| row.has_unseen_reply).collect();
    assert_eq!(unseen, [false, true, false]);
    assert_eq!(
        switcher.pick(0).unwrap(),
        Pick::Write(pr(BLOG_7)),
        "shows its writing"
    );
}

#[test]
fn picking_counts_rows_as_filtered() {
    let home = home_with_tour_of(SHOP_7);
    let mut switcher = loaded(&three_pull_requests(), home.path());

    switcher.filter("checkout");

    assert_eq!(switcher.pick(0).unwrap(), Pick::Show(pr(SHOP_7)));
    assert_eq!(switcher.pick(1), Err(NotPickable::NoSuchRow));
}

#[test]
fn github_that_cannot_be_reached_is_said_in_the_switcher_and_the_list_stays_empty() {
    let github = three_pull_requests();
    github.inject(GitHubFault::RelevantListFails);

    let switcher = loaded(&github, TempDir::new().unwrap().path());

    assert_eq!(switcher.visible().count(), 0);
    let notice = switcher.notice().unwrap();
    assert!(notice.contains("network is down"), "{notice}");
}

#[test]
fn a_failed_refresh_keeps_the_rows_already_listed_and_gives_the_reason() {
    let github = three_pull_requests();
    let home = TempDir::new().unwrap();
    let mut switcher = loaded(&github, home.path());

    github.inject(GitHubFault::RelevantListFails);
    switcher.refreshing();
    switcher.loaded(github.relevant_pull_requests(), home.path());

    assert_eq!(visible_urls(&switcher), [BLOG_7, API_31, SHOP_7]);
    let notice = switcher.notice().unwrap();
    assert!(notice.contains("network is down"), "{notice}");
    assert!(!switcher.is_refreshing());
}

#[test]
fn while_a_refresh_runs_the_old_rows_stay_and_the_switcher_says_it_refreshes() {
    let home = TempDir::new().unwrap();
    let mut switcher = loaded(&three_pull_requests(), home.path());
    assert!(!switcher.is_refreshing());

    switcher.refreshing();

    assert!(switcher.is_refreshing());
    assert!(!switcher.is_loading(), "there is a list to show");
    assert_eq!(switcher.visible().count(), 3);
}

#[test]
fn while_the_list_loads_the_switcher_says_so() {
    let mut switcher = PrSwitcher::default();
    assert!(switcher.is_loading());

    switcher.loaded(Ok(RelevantPullRequests::default()), Path::new("/nowhere"));

    assert!(!switcher.is_loading());
    assert_eq!(switcher.notice(), None);
}

#[test]
fn teams_that_were_not_searched_are_named() {
    let mut switcher = PrSwitcher::default();

    switcher.loaded(
        Ok(RelevantPullRequests {
            pull_requests: Vec::new(),
            teams_left_out: vec!["acme/team11".to_owned(), "acme/team12".to_owned()],
        }),
        Path::new("/nowhere"),
    );

    let notice = switcher.notice().unwrap();
    assert!(
        notice.contains("acme/team11") && notice.contains("acme/team12"),
        "{notice}"
    );
}

#[test]
fn a_reload_after_a_failure_clears_the_notice_and_keeps_the_filter() {
    let github = three_pull_requests();
    github.inject(GitHubFault::RelevantListFails);
    let home = TempDir::new().unwrap();
    let mut switcher = loaded(&github, home.path());
    switcher.filter("alice");

    github.heal(&GitHubFault::RelevantListFails);
    switcher.loaded(github.relevant_pull_requests(), home.path());

    assert_eq!(switcher.notice(), None);
    assert_eq!(visible_urls(&switcher), [SHOP_7]);
}
