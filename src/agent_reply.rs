//! `guided-review agent-reply`: the tour agent answers a hand-off on GitHub, or says it is working on
//! it, then the hand-off takes that on, in the running app when it shows the PR, else in the state file.

use std::path::Path;

use anyhow::Context;

use crate::app_link::{Answer, AppFolder, Request, tell_app};
use crate::pull_request::PullRequestUrl;
use crate::viewer::{
    AgentReplied, HandOffId, ReplyPlacement, ReviewStateFile, ReviewThreadsPort, ReviewWritesPort,
};

/// What the agent posts on the hand-off's thread.
pub enum AgentReply<'a> {
    Answer(&'a str),
    /// "Working on it…", which the answer replaces later.
    WorkingOnIt,
}

/// Says where the reply went. Nothing is posted for a hand-off the PR does not have.
pub fn agent_reply(
    github: &(impl ReviewThreadsPort + ReviewWritesPort),
    home: &Path,
    pr: &PullRequestUrl,
    id: HandOffId,
    reply: &AgentReply<'_>,
) -> anyhow::Result<String> {
    let state_file = ReviewStateFile::for_pr(home, pr);
    let state = state_file.load()?;
    let request = match reply {
        AgentReply::Answer(body) => state.hand_offs().agent_reply(pr, id, body)?,
        AgentReply::WorkingOnIt => state.hand_offs().agent_working_on_it(pr, id)?,
    };
    let done = request
        .run(github)
        .with_context(|| format!("could not post the reply to {id} on {pr}"))?;
    mark_replied(home, &state_file, pr, id, done.replied())?;
    let what = match reply {
        AgentReply::Answer(_) => "replied to",
        AgentReply::WorkingOnIt => "said you are working on",
    };
    Ok(match done.placement() {
        ReplyPlacement::JoinedPendingReview => {
            format!("{what} {id}: in your pending review, which goes out when you send it")
        }
        ReplyPlacement::PostedAtOnce => format!("{what} {id}: posted on {pr}"),
        ReplyPlacement::ReplacedWorkingOnIt => {
            format!("{what} {id}: in place of the \"Working on it…\" reply")
        }
    })
}

/// The app holds the state of the PR it shows and writes it back whole, so it marks that one itself.
fn mark_replied(
    home: &Path,
    state_file: &ReviewStateFile,
    pr: &PullRequestUrl,
    id: HandOffId,
    replied: &AgentReplied,
) -> anyhow::Result<()> {
    let told = tell_app(
        &AppFolder::in_home(home),
        &Request::AgentReplied {
            pr: pr.to_string(),
            hand_off: id,
            replied: replied.clone(),
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
    state_file.update(|state| marked = state.hand_off_replied(id, replied))?;
    Ok(marked?)
}
