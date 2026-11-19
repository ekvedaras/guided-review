//! The review agent at app level: its agents and worktrees are built from what the app holds, and a
//! findings-done ends it whichever review is in the window.

use std::sync::Arc;

use gpui::AsyncApp;

use super::components::pr_switcher::PrSwitching;
use crate::review_subject::ReviewSubject;
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

/// Ends the subject's review agent and removes its worktree, off the UI thread as it runs `hc` and `git`.
pub fn end_in_background(subject: ReviewSubject, cx: &mut AsyncApp) {
    let agents = cx.update(|cx| agents_of(cx.global::<PrSwitching>()));
    cx.background_executor()
        .spawn(async move {
            if let Err(error) = agents.end(&subject) {
                eprintln!("could not end the review agent of {subject}: {error:#}");
            }
        })
        .detach();
}
