use guided_review::review_subject::ReviewSubject;
use guided_review::tour::ToursOnDisk;
use guided_review::viewer::{
    ActivePr, GuidedTourProgress, OpenDecision, OpenOrigin, OpenOutcome, WriteDecision, WritingFact,
};

const A: &str = "https://github.com/acme/shop/pull/7";
const B: &str = "https://github.com/acme/shop/pull/8";

fn pr(url: &str) -> ReviewSubject {
    url.parse().unwrap()
}

fn shown(url: &str) -> ActivePr {
    let mut active = ActivePr::default();
    let OpenDecision::Build(ticket) = active.request(pr(url), OpenOrigin::Reviewer) else {
        panic!("builds");
    };
    active.finished(ticket, Ok::<_, anyhow::Error>(()));
    active
}

#[test]
fn writing_in_the_background_has_no_loading_screen_and_is_refused_while_on() {
    let mut active = shown(A);

    assert_eq!(
        active.write_in_background(pr(A)),
        WriteDecision::StartWriter
    );

    assert!(active.loading().is_none());
    assert_eq!(active.writing_fact_of(&pr(A)), WritingFact::Writing);
    assert_eq!(active.write_in_background(pr(A)), WriteDecision::Watch);
}

#[test]
fn the_writers_open_of_the_basic_pr_on_screen_says_guided_ready_and_changes_nothing() {
    let mut active = shown(A);
    active.write_in_background(pr(A));

    let outcome = active.open(
        pr(A),
        OpenOrigin::AppStartedAgent,
        ToursOnDisk::BasicWithGuided,
    );

    assert!(matches!(
        outcome,
        OpenOutcome::Tour(OpenDecision::GuidedReady)
    ));
    assert!(!active.is_building());
    assert_eq!(active.shown(), Some(&pr(A)));
    assert_eq!(active.switching_to(), None);
    assert_eq!(active.writing_fact_of(&pr(A)), WritingFact::None);
    assert_eq!(
        active.guided_tour_progress_of(&pr(A), ToursOnDisk::BasicWithGuided),
        GuidedTourProgress::Ready
    );
}

#[test]
fn an_open_from_the_reviewers_own_session_of_the_shown_basic_pr_is_guided_ready_too() {
    let mut active = shown(A);

    let outcome = active.open(pr(A), OpenOrigin::Reviewer, ToursOnDisk::BasicWithGuided);

    assert!(matches!(
        outcome,
        OpenOutcome::Tour(OpenDecision::GuidedReady)
    ));
    assert!(!active.is_building());
}

#[test]
fn the_writers_open_of_a_pr_not_on_screen_stays_in_the_background_and_the_switcher_says_ready() {
    let mut active = shown(A);
    active.write_in_background(pr(B));

    let outcome = active.open(
        pr(B),
        OpenOrigin::AppStartedAgent,
        ToursOnDisk::BasicWithGuided,
    );

    assert!(matches!(
        outcome,
        OpenOutcome::Tour(OpenDecision::KeepInBackground)
    ));
    assert_eq!(active.shown(), Some(&pr(A)));
    assert_eq!(active.writing_fact_of(&pr(B)), WritingFact::None);
}

#[test]
fn a_guided_file_on_disk_with_no_writing_starts_ready_but_one_mid_writing_does_not() {
    let mut active = ActivePr::default();

    assert_eq!(
        active.guided_tour_progress_of(&pr(A), ToursOnDisk::BasicWithGuided),
        GuidedTourProgress::Ready
    );
    assert_eq!(
        active.guided_tour_progress_of(&pr(A), ToursOnDisk::Basic),
        GuidedTourProgress::NotAsked
    );

    active.write_in_background(pr(A));

    assert!(matches!(
        active.guided_tour_progress_of(&pr(A), ToursOnDisk::BasicWithGuided),
        GuidedTourProgress::Writing(_)
    ));
}

#[test]
fn a_failed_writing_is_known_as_failed_with_its_reason_and_can_start_over() {
    let mut active = shown(A);
    active.write_in_background(pr(A));
    active.writer_started(&pr(A), Err(anyhow::anyhow!("no herdr")));

    let GuidedTourProgress::Failed(reason) =
        active.guided_tour_progress_of(&pr(A), ToursOnDisk::Basic)
    else {
        panic!("failed");
    };
    assert!(reason.contains("no herdr"));
    assert_eq!(
        active.write_in_background(pr(A)),
        WriteDecision::StartWriter
    );
    assert!(matches!(
        active.guided_tour_progress_of(&pr(A), ToursOnDisk::Basic),
        GuidedTourProgress::Writing(_)
    ));
}
