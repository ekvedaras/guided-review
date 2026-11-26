//! GitHub's names for the values the viewer reads and sends: the states of checks, reviews,
//! merges and issues, merge methods and reactions. The viewer's own types know none of them.

use crate::merge_method::MergeMethod;
use crate::viewer::{CheckState, Checks, IssueState, Mergeability, Reaction, ReviewOutcome};

/// From the `state` of a commit's `statusCheckRollup`; `None` when the commit has none.
pub fn checks_of_rollup(state: Option<&str>) -> Checks {
    match state {
        None => Checks::NoneRun,
        Some("SUCCESS") => Checks::Passing,
        Some("FAILURE" | "ERROR") => Checks::Failing,
        Some(_) => Checks::Running,
    }
}

/// From a check run's `status` and, once it completed, its `conclusion`. A cancelled or timed
/// out run did not pass, so it fails.
pub fn state_of_check_run(status: &str, conclusion: Option<&str>) -> CheckState {
    if status != "COMPLETED" {
        return CheckState::Running;
    }
    match conclusion {
        Some("SUCCESS") => CheckState::Passing,
        Some("NEUTRAL") => CheckState::Neutral,
        Some("SKIPPED") => CheckState::Skipped,
        _ => CheckState::Failing,
    }
}

/// From the `state` of a commit status, which an external service posts.
pub fn state_of_status_context(state: &str) -> CheckState {
    match state {
        "SUCCESS" => CheckState::Passing,
        "FAILURE" | "ERROR" => CheckState::Failing,
        _ => CheckState::Running,
    }
}

/// From `mergeStateStatus`. `UNSTABLE` (checks that are not required fail) and `HAS_HOOKS` merge
/// like `CLEAN`.
pub fn mergeability_of(merge_state_status: &str) -> Mergeability {
    match merge_state_status {
        "CLEAN" | "UNSTABLE" | "HAS_HOOKS" => Mergeability::Now,
        _ => Mergeability::Blocked,
    }
}

/// From `PullRequestReviewState`. A pending review is not sent, so it has no outcome.
pub fn review_outcome_of(state: &str) -> Option<ReviewOutcome> {
    match state {
        "APPROVED" => Some(ReviewOutcome::Approved),
        "CHANGES_REQUESTED" => Some(ReviewOutcome::ChangesRequested),
        "COMMENTED" => Some(ReviewOutcome::Commented),
        "DISMISSED" => Some(ReviewOutcome::Dismissed),
        _ => None,
    }
}

/// From an issue's `state` and `stateReason`.
pub fn issue_state_of(state: &str, reason: Option<&str>) -> IssueState {
    match (state, reason) {
        ("OPEN", _) => IssueState::Open,
        ("MERGED", _) => IssueState::Merged,
        (_, Some("COMPLETED")) => IssueState::ClosedAsCompleted,
        (_, Some("NOT_PLANNED")) => IssueState::ClosedAsNotPlanned,
        _ => IssueState::Closed,
    }
}

/// `ReactionContent`.
pub fn reaction_name(reaction: Reaction) -> &'static str {
    match reaction {
        Reaction::ThumbsUp => "THUMBS_UP",
        Reaction::ThumbsDown => "THUMBS_DOWN",
        Reaction::Laugh => "LAUGH",
        Reaction::Hooray => "HOORAY",
        Reaction::Confused => "CONFUSED",
        Reaction::Heart => "HEART",
        Reaction::Rocket => "ROCKET",
        Reaction::Eyes => "EYES",
    }
}

pub fn reaction_named(name: &str) -> Option<Reaction> {
    Reaction::ALL
        .into_iter()
        .find(|reaction| reaction_name(*reaction) == name)
}

/// `PullRequestMergeMethod`.
pub fn merge_method_name(method: MergeMethod) -> &'static str {
    match method {
        MergeMethod::Merge => "MERGE",
        MergeMethod::Squash => "SQUASH",
        MergeMethod::Rebase => "REBASE",
    }
}

pub fn merge_method_named(name: &str) -> Option<MergeMethod> {
    MergeMethod::ALL
        .into_iter()
        .find(|method| merge_method_name(*method) == name)
}
