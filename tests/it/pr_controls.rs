use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::pr_status::Pull;
use crate::support::FixtureRepo;
use crate::support::fake_ports::blocked_merging;
use guided_review::build::Build;
use guided_review::local_work_tree::LocalWorkTree;
use guided_review::open_review::open_review;
use guided_review::review_subject::ReviewSubject;
use guided_review::viewer::{
    Checks, Command, MergeMethod, OpenPrStatus, PaletteAction, PrStatus, PressStep, Readiness,
    Review,
};

/// A ready PR the viewer may turn into a draft and turn auto-merge on for, so both keys ask.
fn both_changes_offered() -> PrStatus {
    PrStatus::Open(OpenPrStatus {
        readiness: Readiness::Ready,
        checks: Checks::Passing,
        check_runs: Arc::default(),
        viewer_may_change_readiness: true,
        merging: blocked_merging(),
    })
}

fn read(pull: &Pull) -> (Review, Instant) {
    let mut review = pull.open();
    let host = pull.host().with_status(both_changes_offered());
    let now = Instant::now();
    pull.read(&mut review, &host, now);
    (review, now)
}

type Press = fn(&mut Review, Instant) -> PressStep;

const BOTH_KEYS: [(&str, Press); 2] = [
    ("D", |review, now| review.ask_to_change_readiness(now)),
    ("Shift+M", |review, now| review.ask_to_change_merging(now)),
];

fn pr_controls_of(review: &Review) -> Vec<Command> {
    review
        .palette_actions(Build::Local)
        .into_iter()
        .filter_map(PaletteAction::command)
        .filter(|command| [Command::ToggleDraft, Command::ToggleAutoMerge].contains(command))
        .collect()
}

#[test]
fn a_change_goes_ahead_only_on_the_same_key_again_within_ten_seconds_whatever_the_other_key_did() {
    let pull = Pull::new();
    for (key, press) in BOTH_KEYS {
        let (mut review, now) = read(&pull);
        let (_, other) = BOTH_KEYS
            .into_iter()
            .find(|(other, _)| *other != key)
            .unwrap();

        assert!(
            matches!(press(&mut review, now), PressStep::Ask(_)),
            "{key}"
        );
        let late = now + Duration::from_secs(11);
        assert!(
            matches!(press(&mut review, late), PressStep::Ask(_)),
            "{key}"
        );
        assert!(
            matches!(other(&mut review, late), PressStep::Ask(_)),
            "{key}"
        );
        assert_eq!(
            press(&mut review, late + Duration::from_secs(10)),
            PressStep::Sent,
            "{key}"
        );
    }
}

#[test]
fn a_local_review_has_no_pr_controls_whatever_it_is_told() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    repo.write("README.md", "hello\n");
    repo.commit("Start the shop");
    repo.git(&["checkout", "-q", "-b", "feature"]);
    repo.write("README.md", "hello\nworld\n");
    repo.commit("Greet the world");
    let work_tree = LocalWorkTree::containing(repo.path()).unwrap();
    let subject = ReviewSubject::from(work_tree.change_set().unwrap());
    let mut review = open_review(&subject, repo.path(), &repo.folders())
        .unwrap()
        .review;
    let now = Instant::now();

    review.pr_status_is(both_changes_offered());

    assert_eq!(review.pr_status(), None);
    for (key, press) in BOTH_KEYS {
        assert_eq!(press(&mut review, now), PressStep::Nothing, "{key}");
    }
    assert_eq!(pr_controls_of(&review), []);
}

#[test]
fn a_reopened_pr_review_keeps_what_github_said_and_a_first_press() {
    let pull = Pull::new();
    for (key, press) in BOTH_KEYS {
        let (mut replaced, now) = read(&pull);
        assert!(
            matches!(press(&mut replaced, now), PressStep::Ask(_)),
            "{key}"
        );

        let mut reopened = pull.open();
        reopened.carry_on_from(replaced);

        assert_eq!(reopened.pr_status(), Some(&both_changes_offered()), "{key}");
        assert_eq!(
            pr_controls_of(&reopened),
            [Command::ToggleDraft, Command::ToggleAutoMerge],
            "{key}"
        );
        assert_eq!(press(&mut reopened, now), PressStep::Sent, "{key}");
    }
}

#[test]
fn a_reopened_pr_review_merges_with_the_method_the_config_names_now_and_asks_again_for_it() {
    let pull = Pull::new();
    let (mut replaced, now) = read(&pull);
    replaced.prefer_merge_method(MergeMethod::Rebase);
    replaced.ask_to_change_merging(now);

    let mut reopened = pull.open();
    reopened.prefer_merge_method(MergeMethod::Squash);
    reopened.carry_on_from(replaced);

    assert_eq!(reopened.merge_method(), MergeMethod::Squash);
    let step = reopened.ask_to_change_merging(now);
    assert!(
        matches!(&step, PressStep::Ask(note) if note.contains("squash")),
        "{step:?}"
    );
}
