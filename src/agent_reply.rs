//! `guided-review agent-reply`: the tour agent answers a hand-off on GitHub, then the hand-off is
//! marked answered, by the running app when it shows the PR, else in the state file.

use std::path::Path;

use anyhow::Context;

use crate::app_link::{Answer, AppFolder, Request, tell_app};
use crate::pull_request::PullRequestUrl;
use crate::viewer::{
    HandOffId, ReplyPlacement, ReviewStateFile, ReviewThreadsPort, ReviewWritesPort,
};

/// Says where the reply went. Nothing is posted for a hand-off the PR does not have.
pub fn agent_reply(
    github: &(impl ReviewThreadsPort + ReviewWritesPort),
    home: &Path,
    pr: &PullRequestUrl,
    id: HandOffId,
    body: &str,
) -> anyhow::Result<String> {
    let state_file = ReviewStateFile::for_pr(home, pr);
    let request = state_file.load()?.hand_offs().agent_reply(pr, id, body)?;
    let placement = request
        .run(github)
        .with_context(|| format!("could not post the reply to {id} on {pr}"))?;
    mark_answered(home, &state_file, pr, id)?;
    Ok(match placement {
        ReplyPlacement::JoinedPendingReview => {
            format!("replied to {id}: in your pending review, which goes out when you send it")
        }
        ReplyPlacement::PostedAtOnce => format!("replied to {id}: posted on {pr}"),
    })
}

/// The app holds the state of the PR it shows and writes it back whole, so it marks that one itself.
fn mark_answered(
    home: &Path,
    state_file: &ReviewStateFile,
    pr: &PullRequestUrl,
    id: HandOffId,
) -> anyhow::Result<()> {
    let told = tell_app(
        &AppFolder::in_home(home),
        &Request::AgentReplied {
            pr: pr.to_string(),
            hand_off: id,
        },
    )
    .unwrap_or_else(|error| {
        eprintln!("could not tell the app: {error:#}");
        None
    });
    if matches!(told, Some(Answer::HandOffAnswered { .. })) {
        return Ok(());
    }
    let mut marked = Ok(());
    state_file.update(|state| marked = state.hand_off_answered(id))?;
    Ok(marked?)
}
