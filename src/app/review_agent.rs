//! The review agent at app level: its agents and worktrees are built from what the app holds.

use std::sync::Arc;

use super::components::pr_switcher::PrSwitching;
use crate::review_worktree::GitReviewWorktrees;
use crate::viewer::ReviewAgents;

pub fn agents_of(switching: &PrSwitching) -> ReviewAgents {
    ReviewAgents::new(
        Arc::clone(&switching.agents),
        Arc::new(GitReviewWorktrees::under(
            switching.folders.review_worktrees(),
        )),
        switching.folders.clone(),
    )
}
