use std::sync::Arc;

use crate::support::FixtureRepo;
use guided_review::open_review::{OpenedReview, open_review};
use guided_review::pull_request::PullRequestUrl;
use guided_review::tour::Tour;
use guided_review::viewer::{
    ActivePr, AgentReplied, Finished, HandOffId, OpenDecision, OpenOrigin, OpenOutcome, Problem,
    ReviewStateFile, TourFact, WriteDecision,
};
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
        let OpenDecision::Build(ticket) = self.active.request(pr(url), OpenOrigin::Reviewer) else {
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
    let OpenDecision::Build(ticket) = active.request(pr(url), OpenOrigin::Reviewer) else {
        panic!("builds");
    };
    active.finished(ticket, Ok::<_, anyhow::Error>(()));
}

#[test]
fn a_request_for_the_pr_on_screen_reloads_it() {
    let mut active = ActivePr::default();
    shown_after(&mut active, A);

    assert!(matches!(
        active.request(pr(A), OpenOrigin::Reviewer),
        OpenDecision::Reload
    ));
    assert_eq!(active.shown(), Some(&pr(A)));
}

#[test]
fn a_request_for_another_pr_builds_it_while_the_first_stays_shown() {
    let mut active = ActivePr::default();
    shown_after(&mut active, A);

    assert!(matches!(
        active.request(pr(B), OpenOrigin::Reviewer),
        OpenDecision::Build(_)
    ));
    assert_eq!(active.shown(), Some(&pr(A)));
}

#[test]
fn a_newer_request_wins_over_a_build_still_running() {
    let mut active = ActivePr::default();
    let OpenDecision::Build(first) = active.request(pr(A), OpenOrigin::Reviewer) else {
        panic!("builds");
    };
    let OpenDecision::Build(second) = active.request(pr(B), OpenOrigin::Reviewer) else {
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
    let OpenDecision::Build(slow) = active.request(pr(B), OpenOrigin::Reviewer) else {
        panic!("builds");
    };

    assert!(matches!(
        active.request(pr(A), OpenOrigin::Reviewer),
        OpenDecision::Reload
    ));

    let late = active.finished(slow, Ok::<_, anyhow::Error>(()));
    assert!(matches!(late, Finished::Superseded));
    assert_eq!(active.shown(), Some(&pr(A)));
}

#[test]
fn a_failed_build_stays_with_the_reason() {
    let mut active = ActivePr::default();
    shown_after(&mut active, A);
    let OpenDecision::Build(ticket) = active.request(pr(B), OpenOrigin::Reviewer) else {
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

#[test]
fn a_picked_pr_is_being_switched_to_until_it_is_shown() {
    let mut active = ActivePr::default();
    shown_after(&mut active, A);

    active.picked(pr(B));
    assert_eq!(active.switching_to(), Some(&pr(B)));
    assert_eq!(active.shown(), Some(&pr(A)));

    let OpenDecision::Build(ticket) = active.request(pr(B), OpenOrigin::Reviewer) else {
        panic!("builds");
    };
    assert_eq!(active.switching_to(), Some(&pr(B)));
    active.finished(ticket, Ok::<_, anyhow::Error>(()));

    assert_eq!(active.switching_to(), None);
    assert_eq!(active.shown(), Some(&pr(B)));
}

#[test]
fn a_switch_that_failed_is_no_longer_being_switched_to() {
    let mut active = ActivePr::default();
    shown_after(&mut active, A);
    active.picked(pr(B));
    let OpenDecision::Build(ticket) = active.request(pr(B), OpenOrigin::Reviewer) else {
        panic!("builds");
    };

    active.finished(ticket, Err::<(), _>(anyhow::anyhow!("no clone")));

    assert_eq!(active.switching_to(), None);
}

#[test]
fn picking_the_pr_on_screen_is_not_a_switch() {
    let mut active = ActivePr::default();
    shown_after(&mut active, A);

    active.picked(pr(A));

    assert_eq!(active.switching_to(), None);
}

#[test]
fn a_build_started_by_a_request_is_a_switch_too() {
    let mut active = ActivePr::default();
    shown_after(&mut active, A);

    active.request(pr(B), OpenOrigin::Reviewer);

    assert_eq!(active.switching_to(), Some(&pr(B)));
}

#[test]
fn a_reopen_builds_the_pr_on_screen_again_behind_the_opening_veil() {
    let mut active = ActivePr::default();
    shown_after(&mut active, A);

    let (ticket, reopened) = active.reopen().unwrap();
    assert_eq!(reopened, pr(A));
    assert_eq!(active.switching_to(), Some(&pr(A)));

    let finished = active.finished(ticket, Ok::<_, anyhow::Error>("again"));

    assert!(matches!(finished, Finished::Switch("again")));
    assert_eq!(active.shown(), Some(&pr(A)));
    assert_eq!(active.switching_to(), None);
}

#[test]
fn there_is_nothing_to_reopen_with_no_pr_on_screen() {
    let mut active = ActivePr::default();

    assert_eq!(active.reopen(), None);
    assert_eq!(active.switching_to(), None);
}

#[test]
fn a_reopen_leaves_a_switch_to_another_pr_to_build_as_it_reads_the_config_anyway() {
    let mut active = ActivePr::default();
    shown_after(&mut active, A);
    let OpenDecision::Build(switch) = active.request(pr(B), OpenOrigin::Reviewer) else {
        panic!("builds");
    };

    assert_eq!(active.reopen(), None);

    assert!(matches!(
        active.finished(switch, Ok::<_, anyhow::Error>("b")),
        Finished::Switch("b")
    ));
    assert_eq!(active.shown(), Some(&pr(B)));
}

#[test]
fn a_switch_to_another_pr_wins_over_a_reopen_still_building() {
    let mut active = ActivePr::default();
    shown_after(&mut active, A);
    let (reopen, _) = active.reopen().unwrap();
    let OpenDecision::Build(switch) = active.request(pr(B), OpenOrigin::Reviewer) else {
        panic!("builds");
    };

    let late = active.finished(reopen, Ok::<_, anyhow::Error>("a"));
    let current = active.finished(switch, Ok::<_, anyhow::Error>("b"));

    assert!(matches!(late, Finished::Superseded));
    assert!(matches!(current, Finished::Switch("b")));
    assert_eq!(active.shown(), Some(&pr(B)));
}

#[test]
fn a_second_reopen_wins_over_the_first_so_the_newest_config_applies() {
    let mut active = ActivePr::default();
    shown_after(&mut active, A);
    let (first, _) = active.reopen().unwrap();
    let (second, _) = active.reopen().unwrap();

    assert!(matches!(
        active.finished(first, Ok::<_, anyhow::Error>("first")),
        Finished::Superseded
    ));
    assert!(matches!(
        active.finished(second, Ok::<_, anyhow::Error>("second")),
        Finished::Switch("second")
    ));
}

#[test]
fn an_open_of_the_pr_on_screen_while_it_reopens_builds_it_again_rather_than_cancel_the_reopen() {
    let mut active = ActivePr::default();
    shown_after(&mut active, A);
    let (reopen, _) = active.reopen().unwrap();

    let OpenDecision::Build(again) = active.request(pr(A), OpenOrigin::AppStartedAgent) else {
        panic!("builds");
    };

    assert!(matches!(
        active.finished(reopen, Ok::<_, anyhow::Error>("stale")),
        Finished::Superseded
    ));
    assert!(matches!(
        active.finished(again, Ok::<_, anyhow::Error>("fresh")),
        Finished::Switch("fresh")
    ));
    assert_eq!(active.switching_to(), None);
}

fn hand_off(id: &str) -> HandOffId {
    id.parse().unwrap()
}

#[test]
fn an_agent_reply_while_the_pr_reopens_is_kept_for_the_window_it_opens_in() {
    let mut active = ActivePr::default();
    shown_after(&mut active, A);
    let (ticket, _) = active.reopen().unwrap();

    active.hold_reply_for_reopen(&pr(A), hand_off("h1"), &AgentReplied::default());
    active.finished(ticket, Ok::<_, anyhow::Error>(()));

    assert_eq!(
        active.take_replies_for_reopened(),
        [(hand_off("h1"), AgentReplied::default())]
    );
    assert_eq!(active.take_replies_for_reopened(), []);
}

#[test]
fn an_agent_reply_is_kept_only_while_its_pr_reopens() {
    let mut active = ActivePr::default();
    shown_after(&mut active, A);
    active.hold_reply_for_reopen(&pr(A), hand_off("h1"), &AgentReplied::default());
    let (ticket, _) = active.reopen().unwrap();

    active.hold_reply_for_reopen(&pr(B), hand_off("h2"), &AgentReplied::default());
    active.finished(ticket, Ok::<_, anyhow::Error>(()));

    assert_eq!(active.take_replies_for_reopened(), []);
}

#[test]
fn replies_kept_for_a_reopen_go_with_it_when_it_fails() {
    let mut active = ActivePr::default();
    shown_after(&mut active, A);
    let (failing, _) = active.reopen().unwrap();
    active.hold_reply_for_reopen(&pr(A), hand_off("h1"), &AgentReplied::default());
    active.finished(failing, Err::<(), _>(anyhow::anyhow!("clone is gone")));

    let (ticket, _) = active.reopen().unwrap();
    active.finished(ticket, Ok::<_, anyhow::Error>(()));

    assert_eq!(active.take_replies_for_reopened(), []);
}

#[test]
fn replies_kept_for_a_reopen_go_with_it_when_another_pr_wins() {
    let mut active = ActivePr::default();
    shown_after(&mut active, A);
    active.reopen().unwrap();
    active.hold_reply_for_reopen(&pr(A), hand_off("h1"), &AgentReplied::default());

    shown_after(&mut active, B);

    assert_eq!(active.take_replies_for_reopened(), []);
}

#[test]
fn replies_kept_for_a_reopen_stay_kept_when_the_pr_is_built_again() {
    let mut active = ActivePr::default();
    shown_after(&mut active, A);
    active.reopen().unwrap();
    active.hold_reply_for_reopen(&pr(A), hand_off("h1"), &AgentReplied::default());

    let (ticket, _) = active.reopen().unwrap();
    active.finished(ticket, Ok::<_, anyhow::Error>(()));

    assert_eq!(
        active.take_replies_for_reopened(),
        [(hand_off("h1"), AgentReplied::default())]
    );
}

#[test]
fn a_failed_reopen_keeps_the_pr_on_screen_with_the_reason() {
    let mut active = ActivePr::default();
    shown_after(&mut active, A);
    let (ticket, _) = active.reopen().unwrap();

    let finished = active.finished(ticket, Err::<(), _>(anyhow::anyhow!("clone is gone")));

    assert!(matches!(finished, Finished::Stay { reason } if reason.contains("clone is gone")));
    assert_eq!(active.shown(), Some(&pr(A)));
    assert_eq!(active.switching_to(), None);
}

#[test]
fn an_open_of_a_pr_without_a_tour_shows_its_tour_being_written() {
    let mut active = ActivePr::default();
    let pr: PullRequestUrl = A.parse().unwrap();

    let outcome = active.open(pr.clone(), OpenOrigin::Reviewer, TourFact::Missing);

    assert!(matches!(
        outcome,
        OpenOutcome::Writing(WriteDecision::StartWriter)
    ));
    assert_eq!(active.loading().map(|(loading, _)| loading), Some(&pr));
    assert!(!active.is_building());
}

#[test]
fn an_open_of_a_pr_whose_tour_is_being_written_watches_that_writing() {
    let mut active = ActivePr::default();
    let pr: PullRequestUrl = A.parse().unwrap();
    active.open(pr.clone(), OpenOrigin::Reviewer, TourFact::Missing);

    let outcome = active.open(pr, OpenOrigin::Reviewer, TourFact::Writing);

    assert!(matches!(
        outcome,
        OpenOutcome::Writing(WriteDecision::Watch)
    ));
}

#[test]
fn an_open_of_a_pr_with_a_tour_builds_it() {
    let mut active = ActivePr::default();

    let outcome = active.open(A.parse().unwrap(), OpenOrigin::Reviewer, TourFact::Ready);

    assert!(matches!(outcome, OpenOutcome::Tour(OpenDecision::Build(_))));
}

#[test]
fn the_writers_open_once_the_tour_is_there_builds_it_for_the_loading_screen_waiting_on_it() {
    let mut active = ActivePr::default();
    let pr: PullRequestUrl = A.parse().unwrap();
    active.open(pr.clone(), OpenOrigin::Reviewer, TourFact::Missing);

    let outcome = active.open(pr, OpenOrigin::AppStartedAgent, TourFact::Ready);

    assert!(matches!(outcome, OpenOutcome::Tour(OpenDecision::Build(_))));
    assert!(active.loading().is_none());
}

#[test]
fn a_written_tour_that_fails_to_build_shows_the_failure_on_the_loading_screen_to_try_again() {
    let mut active = ActivePr::default();
    let pr: PullRequestUrl = A.parse().unwrap();
    active.open(pr.clone(), OpenOrigin::Reviewer, TourFact::Missing);
    let OpenOutcome::Tour(OpenDecision::Build(ticket)) =
        active.open(pr.clone(), OpenOrigin::AppStartedAgent, TourFact::Ready)
    else {
        panic!("the writer's open builds the tour");
    };

    let finished = active.finished::<()>(ticket, Err(anyhow::anyhow!("the tour is broken")));

    assert!(matches!(finished, Finished::Stay { .. }));
    let (loading, writing) = active.loading().expect("the loading screen stays");
    assert_eq!(loading, &pr);
    assert!(writing.has_failed());
    assert_eq!(active.tour_fact_of(&pr), Some(TourFact::WritingFailed));
    active.try_again(&pr);
    assert!(!active.writing_of(&pr).unwrap().has_failed());
}

#[test]
fn a_failed_build_of_a_tour_nobody_was_writing_leaves_no_loading_screen() {
    let mut active = ActivePr::default();
    let pr: PullRequestUrl = A.parse().unwrap();
    let OpenDecision::Build(ticket) = active.request(pr, OpenOrigin::Reviewer) else {
        panic!("builds");
    };

    active.finished::<()>(ticket, Err(anyhow::anyhow!("no clone")));

    assert!(active.loading().is_none());
}
