use crate::support::fake_ports::blocked_merging;
use guided_review::viewer::{
    AutoMerge, MergeChange, MergeMethod, MergeMethods, MergeOffer, MergeRefusal, Mergeability,
    Merging,
};

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
                mergeability: Mergeability::NotOpen,
                ..blocked
            },
            MergeRefusal::NotOpen,
        ),
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
fn github_merge_states_are_merge_now_blocked_or_not_open() {
    for status in ["CLEAN", "UNSTABLE", "HAS_HOOKS"] {
        assert_eq!(
            Mergeability::from_github("OPEN", status),
            Mergeability::Now,
            "{status}"
        );
    }
    for status in ["BLOCKED", "BEHIND", "DIRTY", "DRAFT", "UNKNOWN"] {
        assert_eq!(
            Mergeability::from_github("OPEN", status),
            Mergeability::Blocked,
            "{status}"
        );
    }
    assert_eq!(
        Mergeability::from_github("MERGED", "UNKNOWN"),
        Mergeability::NotOpen
    );
    assert_eq!(
        Mergeability::from_github("CLOSED", "CLEAN"),
        Mergeability::NotOpen
    );
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
