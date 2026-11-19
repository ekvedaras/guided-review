//! `guided-review findings-done`: the review agent says its findings file is written and checked.
//! The file on disk is the state, so an app that is not running, or shows another review, reads it
//! when it opens the review.

use crate::app_folders::AppFolders;
use crate::app_link::{AppFolder, Request, tell_app};
use crate::findings::FindingsFile;
use crate::review_subject::ReviewSubject;
use crate::tour::Tour;

/// Says what became of the news.
pub fn findings_done(folders: &AppFolders, subject: &ReviewSubject) -> anyhow::Result<String> {
    let path = Tour::findings_path_for(folders, subject);
    let findings = FindingsFile::load_of(&path, subject)?;
    anyhow::ensure!(
        findings.pin.is_pinned(),
        "{} is not pinned to the change yet: run check-findings, fix its errors, then run findings-done",
        path.display()
    );
    let told = tell_app(
        &AppFolder::of(folders),
        &Request::FindingsDone {
            pr: subject.to_string(),
        },
    )?;
    Ok(match told {
        Some(answer) => answer.to_string(),
        None => format!(
            "no app is running; it reads the {} findings when it opens {subject}",
            findings.findings.len()
        ),
    })
}
