//! The review agent at app level: its agents are built from what the app holds.

use std::sync::Arc;

use super::components::pr_switcher::PrSwitching;
use crate::git_checkout::GitCheckouts;
use crate::viewer::ReviewAgents;

pub fn agents_of(switching: &PrSwitching) -> ReviewAgents {
    ReviewAgents::new(
        Arc::clone(&switching.agents),
        Arc::new(GitCheckouts),
        switching.folders.clone(),
    )
}
