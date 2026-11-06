use std::sync::Arc;

use crate::support::FixtureRepo;
use guided_review::open_review::{OpenedReview, open_review};
use guided_review::pull_request::PullRequestUrl;
use guided_review::tour::Tour;
use guided_review::viewer::{ActivePr, Finished, OpenDecision, Problem, ReviewStateFile};
use serde_json::json;

const A: &str = "https://github.com/acme/shop/pull/7";
const B: &str = "https://github.com/acme/shop/pull/8";
const NO_TOUR: &str = "https://github.com/acme/shop/pull/9";

fn pr(url: &str) -> PullRequestUrl {
    url.parse().unwrap()
}

fn shop_with_tours_of_a_and_b() -> FixtureRepo {
    let repo = FixtureRepo::cloned_from("acme/shop");
    repo.write("README.md", "hello\n");
    let base = repo.commit("base");
    repo.write("README.md", "hello\nworld\n");
    let head = repo.commit("head");
    for url in [A, B] {
        let tour = json!({
            "pr": url,
            "base": base,
            "head": head,
            "title": format!("Tour of {url}"),
            "summary": [],
            "steps": [{"id": "readme", "title": "Readme", "why": "w", "pieces": [{"file": "README.md"}]}],
        });
        let path = Tour::path_for(repo.home(), &pr(url));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, tour.to_string()).unwrap();
    }
    repo
}

/// What the app's window does with a request, without the window.
struct Window<'a> {
    repo: &'a FixtureRepo,
    active: ActivePr,
    shown: Option<OpenedReview>,
}

impl<'a> Window<'a> {
    fn new(repo: &'a FixtureRepo) -> Self {
        Self {
            repo,
            active: ActivePr::default(),
            shown: None,
        }
    }

    fn open(&mut self, url: &str) -> Option<String> {
        let OpenDecision::Build(ticket) = self.active.request(pr(url)) else {
            return None;
        };
        let built = open_review(&pr(url), self.repo.path(), self.repo.home());
        match self.active.finished(ticket, built) {
            Finished::Switch(opened) => {
                self.shown = Some(opened);
                None
            }
            Finished::Stay { reason } => Some(reason),
            Finished::Superseded => None,
        }
    }

    fn title(&self) -> &str {
        self.shown.as_ref().unwrap().review.title()
    }

    fn is_ticked(&self) -> bool {
        self.shown
            .as_ref()
            .unwrap()
            .review
            .is_ticked(&"readme".into())
    }
}

fn shown_after(active: &mut ActivePr, url: &str) {
    let OpenDecision::Build(ticket) = active.request(pr(url)) else {
        panic!("builds");
    };
    active.finished(ticket, Ok::<_, anyhow::Error>(()));
}

#[test]
fn a_request_for_the_pr_on_screen_reloads_it() {
    let mut active = ActivePr::default();
    shown_after(&mut active, A);

    assert!(matches!(active.request(pr(A)), OpenDecision::Reload));
    assert_eq!(active.shown(), Some(&pr(A)));
}

#[test]
fn a_request_for_another_pr_builds_it_while_the_first_stays_shown() {
    let mut active = ActivePr::default();
    shown_after(&mut active, A);

    assert!(matches!(active.request(pr(B)), OpenDecision::Build(_)));
    assert_eq!(active.shown(), Some(&pr(A)));
}

#[test]
fn a_newer_request_wins_over_a_build_still_running() {
    let mut active = ActivePr::default();
    let OpenDecision::Build(first) = active.request(pr(A)) else {
        panic!("builds");
    };
    let OpenDecision::Build(second) = active.request(pr(B)) else {
        panic!("builds");
    };

    let late = active.finished(first, Ok::<_, anyhow::Error>("a"));
    let current = active.finished(second, Ok::<_, anyhow::Error>("b"));

    assert!(matches!(late, Finished::Superseded));
    assert!(matches!(current, Finished::Switch("b")));
    assert_eq!(active.shown(), Some(&pr(B)));
}

#[test]
fn a_reload_of_the_shown_pr_beats_a_build_of_another_one_still_running() {
    let mut active = ActivePr::default();
    shown_after(&mut active, A);
    let OpenDecision::Build(slow) = active.request(pr(B)) else {
        panic!("builds");
    };

    assert!(matches!(active.request(pr(A)), OpenDecision::Reload));

    let late = active.finished(slow, Ok::<_, anyhow::Error>(()));
    assert!(matches!(late, Finished::Superseded));
    assert_eq!(active.shown(), Some(&pr(A)));
}

#[test]
fn a_failed_build_stays_with_the_reason() {
    let mut active = ActivePr::default();
    shown_after(&mut active, A);
    let OpenDecision::Build(ticket) = active.request(pr(B)) else {
        panic!("builds");
    };

    let finished = active.finished(ticket, Err::<(), _>(anyhow::anyhow!("no clone")));

    assert!(matches!(finished, Finished::Stay { reason } if reason.contains("no clone")));
    assert_eq!(active.shown(), Some(&pr(A)));
}

#[test]
fn state_is_as_it_was_when_switching_back() {
    let repo = shop_with_tours_of_a_and_b();
    let mut window = Window::new(&repo);
    window.open(A);
    window
        .shown
        .as_mut()
        .unwrap()
        .review
        .tick(&"readme".into())
        .unwrap();

    window.open(B);
    assert_eq!(window.title(), format!("Tour of {B}"));
    assert!(!window.is_ticked());

    window.open(A);
    assert_eq!(window.title(), format!("Tour of {A}"));
    assert!(window.is_ticked());
}

#[test]
fn a_failed_build_keeps_the_pr_on_screen_and_gives_the_reason() {
    let repo = shop_with_tours_of_a_and_b();
    let mut window = Window::new(&repo);
    window.open(A);

    let reason = window.open(NO_TOUR).unwrap();

    assert!(reason.contains("no tour for"), "{reason}");
    assert_eq!(window.title(), format!("Tour of {A}"));
}

#[test]
fn a_failed_switch_is_named_as_a_problem_of_the_pr_still_shown() {
    let repo = shop_with_tours_of_a_and_b();
    let mut window = Window::new(&repo);
    window.open(A);
    let reason = window.open(NO_TOUR).unwrap();

    let review = &mut window.shown.as_mut().unwrap().review;
    review.switch_failed(reason.clone());

    assert_eq!(review.problems(), [Problem::SwitchFailed(reason)]);
}

#[test]
fn the_code_source_of_the_pr_left_is_dropped_after_the_switch() {
    let repo = shop_with_tours_of_a_and_b();
    let mut window = Window::new(&repo);
    window.open(A);
    let source = Arc::downgrade(&window.shown.as_ref().unwrap().source);

    window.open(B);

    assert!(source.upgrade().is_none());
}

#[test]
fn the_folder_a_pr_was_opened_from_is_remembered_for_a_later_switch() {
    let repo = shop_with_tours_of_a_and_b();
    open_review(&pr(A), repo.path(), repo.home()).unwrap();

    let file = ReviewStateFile::for_pr(repo.home(), &pr(A));

    assert_eq!(file.opened_from().unwrap().as_deref(), Some(repo.path()));
}
