//! `guided-review agent-reply`: the agent answers a hand-off, or says it is working on it, then the
//! hand-off takes that on, in the running app when it shows the review, else in the state file. A PR's
//! answer goes on GitHub; a local review's goes on its local thread in the state file, which only ever
//! changes by a locked read and write of that thread, so the app and this command never write over
//! each other's thread.

use crate::app_folders::AppFolders;
use crate::review_subject::ReviewSubject;

use anyhow::Context;

use crate::app_link::{Answer, AppFolder, Request, tell_app};
use crate::findings::FindingId;
use crate::viewer::{
    AgentReplied, HandOffId, ReplyPlacement, ReviewStateFile, ReviewThreadsPort, ReviewWritesPort,
    ThreadHome, WhyReply,
};

/// What the agent posts on the hand-off's thread.
pub enum AgentReply<'a> {
    Answer(&'a str),
    /// "Working on it…", which the answer replaces later.
    WorkingOnIt,
}

/// What `agent-reply` answers: a question on a finding's thread, when the review has one with that id,
/// else a hand-off.
pub enum ReplyTo {
    Finding(FindingId),
    HandOff(HandOffId),
}

impl ReplyTo {
    /// A finding's id may look like a hand-off's, so the review's findings with a thread come first.
    pub fn of(folders: &AppFolders, subject: &ReviewSubject, id: &str) -> anyhow::Result<Self> {
        let state = ReviewStateFile::for_subject(folders, subject).load()?;
        if let Ok(finding) = FindingId::try_from(id.to_owned())
            && state.finding_threads().thread_of(&finding).is_some()
        {
            return Ok(Self::Finding(finding));
        }
        Ok(Self::HandOff(id.parse()?))
    }
}

/// The review agent's answer to a question on a finding: taken by the app when it shows the review, else
/// kept in the state file under its lock.
pub fn why_reply(
    folders: &AppFolders,
    subject: &ReviewSubject,
    finding: &FindingId,
    reply: &AgentReply<'_>,
) -> anyhow::Result<String> {
    let reply = match reply {
        AgentReply::Answer(body) => WhyReply::Answer((*body).to_owned()),
        AgentReply::WorkingOnIt => WhyReply::Working,
    };
    let told = tell_app(
        &AppFolder::of(folders),
        &Request::WhyReplied {
            pr: subject.to_string(),
            finding: finding.clone(),
            reply: reply.clone(),
        },
    )
    .unwrap_or_else(|error| {
        eprintln!("could not tell the app: {error:#}");
        None
    });
    if matches!(told, Some(Answer::WhyAnswered { .. })) {
        return Ok(format!("told the app that {finding} {reply}"));
    }
    let state_file = ReviewStateFile::for_subject(folders, subject);
    let mut kept = Ok(());
    state_file.update(|state| kept = state.why_replied(finding, &reply))?;
    kept?;
    Ok(format!("{finding} {reply}: on its thread for {subject}"))
}

/// Says where the reply went. Nothing is posted for a hand-off the review does not have.
pub fn agent_reply(
    github: &(impl ReviewThreadsPort + ReviewWritesPort),
    folders: &AppFolders,
    subject: &ReviewSubject,
    id: HandOffId,
    reply: &AgentReply<'_>,
) -> anyhow::Result<String> {
    let state_file = ReviewStateFile::for_subject(folders, subject);
    let state = state_file.load()?;
    let home = ThreadHome::of(subject, &state_file);
    let request = match reply {
        AgentReply::Answer(body) => state.hand_offs().agent_reply(&home, id, body)?,
        AgentReply::WorkingOnIt => state.hand_offs().agent_working_on_it(&home, id)?,
    };
    let done = request
        .run(github)
        .with_context(|| format!("could not post the reply to {id} on {subject}"))?;
    mark_replied(folders, &state_file, subject, id, done.replied())?;
    let what = match reply {
        AgentReply::Answer(_) => "replied to",
        AgentReply::WorkingOnIt => "said you are working on",
    };
    Ok(match (done.placement(), subject) {
        (ReplyPlacement::JoinedPendingReview, _) => {
            format!("{what} {id}: in your pending review, which goes out when you send it")
        }
        (ReplyPlacement::PostedAtOnce, ReviewSubject::PullRequest(pr)) => {
            format!("{what} {id}: posted on {pr}")
        }
        (ReplyPlacement::PostedAtOnce, ReviewSubject::Local(_)) => {
            format!("{what} {id}: on its thread in the local review")
        }
        (ReplyPlacement::ReplacedWorkingOnIt, _) => {
            format!("{what} {id}: in place of the \"Working on it…\" reply")
        }
    })
}

/// The app holds the hand-offs of the review it shows and writes them back whole, so it marks that
/// one itself and reads the threads again.
fn mark_replied(
    folders: &AppFolders,
    state_file: &ReviewStateFile,
    subject: &ReviewSubject,
    id: HandOffId,
    replied: &AgentReplied,
) -> anyhow::Result<()> {
    let told = tell_app(
        &AppFolder::of(folders),
        &Request::AgentReplied {
            pr: subject.to_string(),
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
