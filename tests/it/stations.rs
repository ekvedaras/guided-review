use crate::support::FixtureRepo;
use guided_review::open_review::open_review;
use guided_review::review_subject::ReviewSubject;
use guided_review::tour::Tour;
use guided_review::viewer::{
    AgentLinkOff, AgentStatus, Problem, Review, Station, StationHealth, StationKind, Stations,
    TerminalId, TourAgentState, waited,
};
use serde_json::json;
use std::time::{Duration, SystemTime};

const PR: &str = "https://github.com/acme/shop/pull/7";

fn pr() -> ReviewSubject {
    PR.parse().unwrap()
}

fn shop_review() -> (FixtureRepo, Review) {
    let repo = FixtureRepo::cloned_from("acme/shop");
    repo.write("README.md", "hello\n");
    let base = repo.commit("base");
    repo.write("README.md", "hello\nworld\n");
    let head = repo.commit("head");
    let tour = json!({
        "pr": PR, "base": base, "head": head, "title": "Readme", "summary": [],
        "steps": [{"id": "readme", "title": "Readme", "why": "w", "pieces": [{"file": "README.md"}]}],
    });
    let path = Tour::path_for(&repo.folders(), &pr());
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, tour.to_string()).unwrap();
    let review = open_review(&pr(), repo.path(), &repo.folders())
        .unwrap()
        .review;
    (repo, review)
}

fn station(stations: &Stations, kind: StationKind) -> (StationHealth, String) {
    let station = stations.get(kind).unwrap();
    (station.health, station.state.clone())
}

fn of(stations: &[(StationKind, StationHealth)]) -> Stations {
    Stations::new(
        stations
            .iter()
            .map(|(kind, health)| Station::new(*kind, *health, "state"))
            .collect(),
    )
}

#[test]
fn the_card_opens_on_the_station_that_failed_before_one_that_waits() {
    let stations = of(&[
        (StationKind::Tour, StationHealth::Done),
        (StationKind::Fixes, StationHealth::Waiting),
        (StationKind::GitHub, StationHealth::Failed),
    ]);

    assert_eq!(stations.open(None), StationKind::GitHub);
}

#[test]
fn the_card_opens_on_a_station_that_waits_before_one_at_work() {
    let stations = of(&[
        (StationKind::AiReview, StationHealth::Working),
        (StationKind::YourReview, StationHealth::Waiting),
    ]);

    assert_eq!(stations.open(None), StationKind::YourReview);
}

#[test]
fn with_nothing_to_look_at_the_card_opens_on_the_ai_review() {
    let stations = of(&[
        (StationKind::Tour, StationHealth::Done),
        (StationKind::AiReview, StationHealth::NotStarted),
        (StationKind::GitHub, StationHealth::Done),
    ]);

    assert_eq!(stations.open(None), StationKind::AiReview);
}

#[test]
fn a_picked_station_opens_whatever_else_needs_the_reviewer() {
    let stations = of(&[
        (StationKind::Tour, StationHealth::Done),
        (StationKind::GitHub, StationHealth::Failed),
    ]);

    assert_eq!(stations.open(Some(StationKind::Tour)), StationKind::Tour);
    assert_eq!(stations.open(Some(StationKind::Fixes)), StationKind::GitHub);
}

#[test]
fn every_other_station_that_needs_the_reviewer_is_named_under_the_card() {
    let stations = of(&[
        (StationKind::Tour, StationHealth::Working),
        (StationKind::Fixes, StationHealth::Waiting),
        (StationKind::YourReview, StationHealth::Done),
        (StationKind::GitHub, StationHealth::Failed),
    ]);

    let named: Vec<StationKind> = stations
        .also_needing_you(StationKind::GitHub)
        .map(|station| station.kind)
        .collect();

    assert_eq!(named, [StationKind::Fixes]);
}

#[test]
fn problems_sit_with_the_station_they_are_about() {
    let stations = of(&[
        (StationKind::Tour, StationHealth::Done),
        (StationKind::GitHub, StationHealth::Done),
    ]);

    assert_eq!(
        stations.home_of(&Problem::ThreadsNotRead("503".to_owned())),
        Some(StationKind::GitHub)
    );
    assert_eq!(
        stations.home_of(&Problem::ShowAgentFailed("gone".to_owned())),
        Some(StationKind::Tour)
    );
    assert_eq!(
        stations.home_of(&Problem::IdeNotStarted("zed: command not found".to_owned())),
        None
    );
}

#[test]
fn a_github_problem_of_a_review_with_no_github_station_sits_above_the_stepper() {
    let stations = of(&[(StationKind::Tour, StationHealth::Done)]);

    assert_eq!(
        stations.home_of(&Problem::ThreadsNotRead("503".to_owned())),
        None
    );
}

#[test]
fn a_fresh_review_of_a_pr_has_every_station_and_nothing_started() {
    let (_repo, review) = shop_review();

    let stations = review.stations(&TourAgentState::None, SystemTime::now());

    let kinds: Vec<StationKind> = stations.all().iter().map(|station| station.kind).collect();
    assert_eq!(
        kinds,
        [
            StationKind::Tour,
            StationKind::AiReview,
            StationKind::Fixes,
            StationKind::YourReview,
            StationKind::GitHub,
        ]
    );
    assert_eq!(
        station(&stations, StationKind::Tour),
        (StationHealth::Done, "written".to_owned())
    );
    assert_eq!(
        station(&stations, StationKind::AiReview),
        (StationHealth::NotStarted, "not run".to_owned())
    );
    assert_eq!(
        station(&stations, StationKind::Fixes),
        (StationHealth::NotStarted, "none asked".to_owned())
    );
    assert_eq!(
        station(&stations, StationKind::GitHub),
        (StationHealth::Done, "viewed marks in sync".to_owned())
    );
}

#[test]
fn a_tour_agent_that_waits_for_the_reviewer_makes_the_tour_wait() {
    let (_repo, review) = shop_review();
    let agent = TourAgentState::Agent {
        terminal: TerminalId::new("w1:p2"),
        status: AgentStatus::WaitingForYou,
    };

    let stations = review.stations(&agent, SystemTime::now());

    assert_eq!(
        station(&stations, StationKind::Tour),
        (StationHealth::Waiting, "agent waits for you".to_owned())
    );
}

#[test]
fn an_agent_link_that_is_off_fails_the_tour() {
    let (_repo, review) = shop_review();
    let off = TourAgentState::LinkOff(AgentLinkOff::HerdrFailed(
        "registry writer stopped".to_owned(),
    ));

    let stations = review.stations(&off, SystemTime::now());

    assert_eq!(
        station(&stations, StationKind::Tour),
        (StationHealth::Failed, "agent link off".to_owned())
    );
}

#[test]
fn reading_everything_with_no_review_sent_makes_your_review_wait() {
    let (_repo, mut review) = shop_review();
    review.viewer_authorship_is(false);
    let start = SystemTime::now();
    assert_eq!(
        station(
            &review.stations(&TourAgentState::None, start),
            StationKind::YourReview
        )
        .0,
        StationHealth::NotStarted
    );

    review.tick(&"readme".into()).unwrap();

    assert_eq!(
        station(
            &review.stations(&TourAgentState::None, start),
            StationKind::YourReview
        ),
        (StationHealth::Waiting, "ready to send".to_owned())
    );
}

#[test]
fn on_your_own_pr_with_nothing_pending_your_review_has_nothing_to_send() {
    let (_repo, mut review) = shop_review();
    review.viewer_authorship_is(true);

    review.tick(&"readme".into()).unwrap();

    assert_eq!(
        station(
            &review.stations(&TourAgentState::None, SystemTime::now()),
            StationKind::YourReview
        ),
        (StationHealth::NotStarted, "nothing to send".to_owned())
    );
}

#[test]
fn viewed_marks_waiting_for_github_say_how_many_and_for_how_long() {
    let (_repo, mut review) = shop_review();
    let start = SystemTime::now();
    review.tick(&"readme".into()).unwrap();
    review.time_viewed_wait(start).unwrap();

    review
        .time_viewed_wait(start + Duration::from_mins(5))
        .unwrap();
    let stations = review.stations(
        &TourAgentState::None,
        start + Duration::from_secs(4 * 60 + 10),
    );

    assert_eq!(
        station(&stations, StationKind::GitHub),
        (
            StationHealth::Waiting,
            "1 viewed mark waiting · 4 min".to_owned()
        )
    );
}

#[test]
fn how_long_marks_waited_is_said_in_minutes_then_hours() {
    assert_eq!(waited(Duration::from_secs(20)), "under a minute");
    assert_eq!(waited(Duration::from_secs(4 * 60 + 59)), "4 min");
    assert_eq!(waited(Duration::from_mins(122)), "2 h");
}

#[test]
fn how_long_marks_waited_outlives_a_restart() {
    let (repo, mut review) = shop_review();
    let start = SystemTime::now();
    review.tick(&"readme".into()).unwrap();
    review.time_viewed_wait(start).unwrap();
    drop(review);

    let mut reopened = open_review(&pr(), repo.path(), &repo.folders())
        .unwrap()
        .review;
    let later = start + Duration::from_hours(2);
    reopened.time_viewed_wait(later).unwrap();

    assert_eq!(
        station(
            &reopened.stations(&TourAgentState::None, later),
            StationKind::GitHub
        )
        .1,
        "1 viewed mark waiting · 2 h"
    );
}
