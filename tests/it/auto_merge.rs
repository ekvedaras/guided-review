use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::pr_status::{PR, Pull};
use crate::support::fake_ports::{FakeGitHub, GitHubCall, GitHubFault, blocked_merging};
use crate::support::work::drive_without_agents;
use guided_review::update::Build;
use guided_review::viewer::{
    AutoMerge, Checks, Command, Effect, Intent, MergeChange, MergeMethod, MergeMethods, MergeOffer,
    MergeRefusal, Mergeability, Merging, OpenPrStatus, PaletteAction, PrStatus, PressStep, Problem,
    Readiness, Review, WindowMoment,
};

#[test]
fn the_title_bar_button_says_what_a_click_offers_and_hides_where_the_viewer_cannot_act() {
    let blocked = blocked_merging();
    let label = |merging: Merging| merging.button_label();

    assert_eq!(label(blocked), Some("Auto-merge"));
    assert_eq!(
        label(Merging {
            auto_merge: AutoMerge::On(MergeMethod::Squash),
            viewer_may_disable_auto_merge: true,
            ..blocked
        }),
        Some("Auto-merge on")
    );
    assert_eq!(
        label(Merging {
            mergeability: Mergeability::Now,
            ..blocked
        }),
        Some("Merge now")
    );
    assert_eq!(
        label(Merging {
            viewer_may_enable_auto_merge: false,
            ..blocked
        }),
        None
    );
}

fn read_merging(pull: &Pull, merging: Merging) -> (Review, FakeGitHub, Instant) {
    let mut review = pull.open();
    let github = pull.github().with_status(PrStatus::Open(OpenPrStatus {
        readiness: Readiness::Ready,
        checks: Checks::Running,
        check_runs: Arc::default(),
        viewer_may_change_readiness: true,
        merging,
    }));
    let now = Instant::now();
    pull.read(&mut review, &github, now);
    (review, github, now)
}

fn merging_calls(pull: &Pull) -> Vec<MergeChange> {
    pull.sent
        .github_calls()
        .into_iter()
        .filter_map(|call| match call {
            GitHubCall::ChangeMerging { change, .. } => Some(change),
            _ => None,
        })
        .collect()
}

fn asked(step: PressStep) -> String {
    let PressStep::Ask(note) = step else {
        panic!("not asked: {step:?}");
    };
    note
}

#[test]
fn the_first_press_says_it_turns_on_auto_merge_with_a_merge_commit_and_sends_nothing() {
    let pull = Pull::new();
    let (mut review, github, now) = read_merging(&pull, blocked_merging());

    let note = asked(review.ask_to_change_merging(now));

    assert!(note.contains("turn on auto-merge"), "{note}");
    assert!(note.contains("merge commit"), "{note}");
    assert!(note.contains("again within 10 s"), "{note}");
    drive_without_agents(&mut review, &github, &pull.source(), now);
    assert_eq!(merging_calls(&pull), []);
}

#[test]
fn the_same_press_again_within_ten_seconds_turns_auto_merge_on() {
    let pull = Pull::new();
    let (mut review, github, now) = read_merging(&pull, blocked_merging());
    review.ask_to_change_merging(now);

    let later = now + Duration::from_secs(10);
    assert_eq!(review.ask_to_change_merging(later), PressStep::Sent);
    drive_without_agents(&mut review, &github, &pull.source(), later);

    assert!(
        pull.sent
            .github_calls()
            .contains(&GitHubCall::ChangeMerging {
                pr: PR.parse().unwrap(),
                change: MergeChange::EnableAutoMerge(MergeMethod::Merge),
            })
    );
    assert_eq!(
        merging_calls(&pull),
        [MergeChange::EnableAutoMerge(MergeMethod::Merge)]
    );
    assert_eq!(
        review
            .pr_status()
            .and_then(PrStatus::open)
            .map(|status| status.merging.auto_merge),
        Some(AutoMerge::On(MergeMethod::Merge))
    );
    assert_eq!(review.problems(), []);
}

#[test]
fn a_repo_that_does_not_allow_the_chosen_method_gets_its_first_allowed_one_and_the_toast_says_so() {
    let pull = Pull::new();
    let (mut review, github, now) = read_merging(
        &pull,
        Merging {
            methods: MergeMethods::only(&[MergeMethod::Squash, MergeMethod::Rebase]),
            ..blocked_merging()
        },
    );

    let note = asked(review.ask_to_change_merging(now));
    review.ask_to_change_merging(now);
    drive_without_agents(&mut review, &github, &pull.source(), now);

    assert!(note.contains("squash"), "{note}");
    assert!(note.contains("does not allow merge commit"), "{note}");
    assert_eq!(
        merging_calls(&pull),
        [MergeChange::EnableAutoMerge(MergeMethod::Squash)]
    );
}

#[test]
fn the_merge_method_from_settings_is_used_where_the_repo_allows_it() {
    let pull = Pull::new();
    let (mut review, github, now) = read_merging(&pull, blocked_merging());
    review.prefer_merge_method(MergeMethod::Rebase);

    review.ask_to_change_merging(now);
    review.ask_to_change_merging(now);
    drive_without_agents(&mut review, &github, &pull.source(), now);

    assert_eq!(
        merging_calls(&pull),
        [MergeChange::EnableAutoMerge(MergeMethod::Rebase)]
    );
}

#[test]
fn with_auto_merge_on_the_same_key_turns_it_off() {
    let pull = Pull::new();
    let (mut review, github, now) = read_merging(
        &pull,
        Merging {
            auto_merge: AutoMerge::On(MergeMethod::Squash),
            viewer_may_disable_auto_merge: true,
            ..blocked_merging()
        },
    );

    let note = asked(review.ask_to_change_merging(now));
    review.ask_to_change_merging(now);
    drive_without_agents(&mut review, &github, &pull.source(), now);

    assert!(note.contains("turn off auto-merge"), "{note}");
    assert_eq!(merging_calls(&pull), [MergeChange::DisableAutoMerge]);
    assert_eq!(
        review
            .pr_status()
            .and_then(PrStatus::open)
            .map(|status| status.merging.auto_merge),
        Some(AutoMerge::Off)
    );
}

#[test]
fn when_nothing_blocks_the_pr_the_same_key_merges_it_now_after_the_second_press() {
    let pull = Pull::new();
    let (mut review, github, now) = read_merging(
        &pull,
        Merging {
            mergeability: Mergeability::Now,
            ..blocked_merging()
        },
    );

    let note = asked(review.ask_to_change_merging(now));
    review.ask_to_change_merging(now);
    drive_without_agents(&mut review, &github, &pull.source(), now);

    assert!(note.contains("merge the PR now"), "{note}");
    assert!(note.contains("merge commit"), "{note}");
    assert_eq!(
        merging_calls(&pull),
        [MergeChange::MergeNow(MergeMethod::Merge)]
    );
    assert_eq!(review.pr_status(), Some(&PrStatus::Merged));
}

#[test]
fn a_merged_or_closed_pr_says_so_and_neither_key_nor_the_palette_offers_a_change() {
    let pull = Pull::new();
    for (status, label) in [(PrStatus::Merged, "Merged"), (PrStatus::Closed, "Closed")] {
        let mut review = pull.open();
        let github = pull.github().with_status(status.clone());
        let now = Instant::now();
        pull.read(&mut review, &github, now);

        assert_eq!(review.pr_status(), Some(&status));
        assert_eq!(status.label(), label);
        assert_eq!(
            review.ask_to_change_merging(now),
            PressStep::Nothing,
            "{label}"
        );
        assert_eq!(
            review.ask_to_change_readiness(now),
            PressStep::Nothing,
            "{label}"
        );
        let offered: Vec<PaletteAction> = review
            .palette_actions(Build::Local)
            .into_iter()
            .filter(|action| {
                [Some(Command::ToggleAutoMerge), Some(Command::ToggleDraft)]
                    .contains(&action.command())
            })
            .collect();
        assert_eq!(offered, [], "{label}");
        assert_eq!(review.next_due(), None, "{label}");
    }
}

#[test]
fn a_press_github_would_refuse_says_why_and_one_before_github_is_read_does_nothing() {
    let pull = Pull::new();
    let (mut read_only, _github, now) = read_merging(
        &pull,
        Merging {
            repo_allows_auto_merge: false,
            viewer_may_enable_auto_merge: false,
            ..blocked_merging()
        },
    );

    assert_eq!(
        read_only.ask_to_change_merging(now),
        PressStep::Refused(MergeRefusal::RepoDisallowsAutoMerge.to_string())
    );
    assert_eq!(pull.open().ask_to_change_merging(now), PressStep::Nothing);
}

#[test]
fn a_change_github_does_not_take_is_a_problem_and_leaves_the_status_as_it_was() {
    let pull = Pull::new();
    let (mut review, github, now) = read_merging(&pull, blocked_merging());
    github.inject(GitHubFault::ConnectionLost);
    review.ask_to_change_merging(now);
    review.ask_to_change_merging(now);

    drive_without_agents(&mut review, &github, &pull.source(), now);

    assert_eq!(
        review
            .pr_status()
            .and_then(PrStatus::open)
            .map(|status| status.merging.auto_merge),
        Some(AutoMerge::Off)
    );
    assert_eq!(
        review.problems(),
        [Problem::MergingNotChanged(
            MergeChange::EnableAutoMerge(MergeMethod::Merge),
            "network is down".to_owned()
        )]
    );
}

#[test]
fn the_palette_offers_the_one_change_that_applies_and_nothing_where_github_would_refuse() {
    let offered = |review: &Review| -> Vec<PaletteAction> {
        review
            .palette_actions(Build::Local)
            .into_iter()
            .filter(|action| action.command() == Some(Command::ToggleAutoMerge))
            .collect()
    };
    let pull = Pull::new();
    let blocked = blocked_merging();

    let (off, ..) = read_merging(&pull, blocked);
    let (on, ..) = read_merging(
        &pull,
        Merging {
            auto_merge: AutoMerge::On(MergeMethod::Merge),
            viewer_may_disable_auto_merge: true,
            ..blocked
        },
    );
    let (clean, ..) = read_merging(
        &pull,
        Merging {
            mergeability: Mergeability::Now,
            ..blocked
        },
    );
    let (refused, ..) = read_merging(
        &pull,
        Merging {
            viewer_may_enable_auto_merge: false,
            ..blocked
        },
    );

    assert_eq!(offered(&off), [PaletteAction::EnableAutoMerge]);
    assert_eq!(offered(&on), [PaletteAction::DisableAutoMerge]);
    assert_eq!(offered(&clean), [PaletteAction::MergeNow]);
    assert_eq!(offered(&refused), []);
    assert_eq!(offered(&pull.open()), []);
}

#[test]
fn the_key_the_button_and_the_palette_ask_for_the_same_thing() {
    let pull = Pull::new();
    let review = pull.open();

    assert_eq!(
        review.decide(Intent::ToggleAutoMerge, WindowMoment::default()),
        Effect::ChangeMerging
    );
    for action in [
        PaletteAction::EnableAutoMerge,
        PaletteAction::DisableAutoMerge,
        PaletteAction::MergeNow,
    ] {
        assert_eq!(Intent::from(action), Intent::ToggleAutoMerge);
    }
    assert_eq!(
        Command::ToggleAutoMerge.default_chords()[0].keystroke(),
        "shift-m"
    );
}

#[test]
fn a_blocked_pr_offers_auto_merge_and_one_with_auto_merge_on_offers_to_turn_it_off() {
    let blocked = blocked_merging();
    let on = Merging {
        auto_merge: AutoMerge::On(MergeMethod::Squash),
        viewer_may_disable_auto_merge: true,
        ..blocked
    };

    assert_eq!(blocked.offer(), Ok(MergeOffer::EnableAutoMerge));
    assert_eq!(on.offer(), Ok(MergeOffer::DisableAutoMerge));
}

#[test]
fn a_pr_nothing_blocks_offers_merge_now_instead_of_auto_merge() {
    let clean = Merging {
        mergeability: Mergeability::Now,
        ..blocked_merging()
    };

    assert_eq!(clean.offer(), Ok(MergeOffer::MergeNow));
}

#[test]
fn nothing_is_offered_where_github_would_refuse_and_the_refusal_says_why() {
    let blocked = blocked_merging();
    let cases = [
        (
            Merging {
                auto_merge: AutoMerge::On(MergeMethod::Merge),
                viewer_may_disable_auto_merge: false,
                ..blocked
            },
            MergeRefusal::MayNotDisableAutoMerge,
        ),
        (
            Merging {
                mergeability: Mergeability::Now,
                viewer_may_merge: false,
                ..blocked
            },
            MergeRefusal::MayNotMerge,
        ),
        (
            Merging {
                repo_allows_auto_merge: false,
                viewer_may_enable_auto_merge: false,
                ..blocked
            },
            MergeRefusal::RepoDisallowsAutoMerge,
        ),
        (
            Merging {
                viewer_may_enable_auto_merge: false,
                ..blocked
            },
            MergeRefusal::MayNotEnableAutoMerge,
        ),
    ];

    for (merging, refusal) in cases {
        assert_eq!(merging.offer(), Err(refusal), "{merging:?}");
    }
}

#[test]
fn the_preferred_method_is_used_where_the_repo_allows_it_else_its_first_allowed_one() {
    let squash_or_rebase = MergeMethods::only(&[MergeMethod::Rebase, MergeMethod::Squash]);

    assert_eq!(
        squash_or_rebase.pick(MergeMethod::Rebase),
        MergeMethod::Rebase
    );
    assert_eq!(
        squash_or_rebase.pick(MergeMethod::Merge),
        MergeMethod::Squash
    );
    let only_squash = Merging {
        methods: MergeMethods::only(&[MergeMethod::Squash]),
        ..blocked_merging()
    };
    assert_eq!(
        only_squash.change(MergeMethod::Merge),
        Ok(MergeChange::EnableAutoMerge(MergeMethod::Squash))
    );
}

#[test]
fn github_merge_states_are_merge_now_or_blocked() {
    for status in ["CLEAN", "UNSTABLE", "HAS_HOOKS"] {
        assert_eq!(
            Mergeability::from_github(status),
            Mergeability::Now,
            "{status}"
        );
    }
    for status in ["BLOCKED", "BEHIND", "DIRTY", "DRAFT", "UNKNOWN"] {
        assert_eq!(
            Mergeability::from_github(status),
            Mergeability::Blocked,
            "{status}"
        );
    }
}

#[test]
fn merge_methods_read_and_write_as_github_and_the_config_spell_them() {
    for method in MergeMethod::ALL {
        assert_eq!(
            MergeMethod::of_github_name(method.github_name()),
            Some(method)
        );
        assert_eq!(
            MergeMethod::of_config_text(method.config_text()),
            Some(method)
        );
    }
    assert_eq!(MergeMethod::default(), MergeMethod::Merge);
}
