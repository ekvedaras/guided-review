//! `guided-review fix-reply`: the tour agent says where it is with one finding of a fix request, then
//! the review takes that on, in the running app when it shows the review, else in the state file. The
//! app holds the review's state and writes it back whole, so it takes the answer itself.

use crate::app_folders::AppFolders;
use crate::app_link::{Answer, AppFolder, Request, tell_app};
use crate::findings::FindingId;
use crate::review_subject::ReviewSubject;
use crate::viewer::{FixReply, ReviewStateFile};

/// Says where the answer went. Nothing is kept for a finding the review has no fix request for.
pub fn fix_reply(
    folders: &AppFolders,
    subject: &ReviewSubject,
    finding: &FindingId,
    reply: &FixReply,
) -> anyhow::Result<String> {
    let told = tell_app(
        &AppFolder::of(folders),
        &Request::FixReplied {
            pr: subject.to_string(),
            finding: finding.clone(),
            reply: reply.clone(),
        },
    )
    .unwrap_or_else(|error| {
        eprintln!("could not tell the app: {error:#}");
        None
    });
    if matches!(told, Some(Answer::FixAnswered { .. })) {
        return Ok(format!("told the app that {finding} {reply}"));
    }
    let state_file = ReviewStateFile::for_subject(folders, subject);
    let mut kept = Ok(());
    state_file.update(|state| kept = state.fix_replied(finding, reply))?;
    kept?;
    Ok(format!("{finding} {reply}: kept for {subject}"))
}
