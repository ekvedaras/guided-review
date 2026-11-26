use std::path::Path;
use std::sync::Arc;

use crate::app_folders::AppFolders;
use crate::code_source::CodeSource;
use crate::findings::FindingsFile;
use crate::pull_request::{ChangeTips, Sha};
use crate::repin::{Repin, repin};
use crate::review_subject::ReviewSubject;
use crate::tour::Tour;

use super::finding_follow::follow_findings;
use super::local_thread_store::LocalThreadStore;
use super::review_state::{RepinRecord, ReviewStateFile};

/// Re-pin as both the CLI and the app run it: the tour moves to the new commits, and the
/// per-PR state learns what became of each piece, so the viewer can show it.
pub fn repin_tour(
    folders: &AppFolders,
    subject: &ReviewSubject,
    tour_path: &Path,
    state_file: &ReviewStateFile,
    source: &dyn CodeSource,
    new_base: &Sha,
    new_head: &Sha,
) -> anyhow::Result<(Tour, Repin)> {
    let mut tour = Tour::load_of(tour_path, subject)?;
    let (old_base, old_head) = (tour.pin().base.clone(), tour.pin().head.clone());
    let result = repin(&mut tour, source, new_base, new_head)?;
    tour.save(tour_path)?;
    let record = RepinRecord::new(&old_base, &old_head, new_base, new_head, &result);
    state_file.update(|state| state.record_repin(record, &result))?;
    if !subject.has_host_threads() {
        LocalThreadStore::in_file(state_file.clone()).follow_to(source, new_head)?;
    }
    follow_findings_file(folders, subject, state_file, source, new_base, new_head)?;
    Ok((tour, result))
}

/// A findings file a review agent wrote after a re-pin, pinned to the commits it read, follows to the
/// commits the review is at now. A file already there is left alone.
pub fn catch_up_findings(
    folders: &AppFolders,
    subject: &ReviewSubject,
    state_file: &ReviewStateFile,
    source: &dyn CodeSource,
    base: &Sha,
    head: &Sha,
) -> anyhow::Result<()> {
    let Some(findings) = FindingsFile::read_of(folders, subject)? else {
        return Ok(());
    };
    let is_behind =
        findings.pin.is_pinned() && (findings.pin.base != *base || findings.pin.head != *head);
    if is_behind {
        follow_findings_file(folders, subject, state_file, source, base, head)?;
    }
    Ok(())
}

fn follow_findings_file(
    folders: &AppFolders,
    subject: &ReviewSubject,
    state_file: &ReviewStateFile,
    source: &dyn CodeSource,
    new_base: &Sha,
    new_head: &Sha,
) -> anyhow::Result<()> {
    let Some(mut findings) = FindingsFile::read_of(folders, subject)? else {
        return Ok(());
    };
    follow_findings(&mut findings, subject, source, new_base, new_head);
    findings.save(&Tour::findings_path_for(folders, subject))?;
    state_file.update(|state| state.follow_findings(&findings.findings))
}

/// Finds the code source that can read the given tips, fetching what is missing.
pub type PickCodeSource =
    Arc<dyn Fn(&ChangeTips) -> anyhow::Result<Box<dyn CodeSource>> + Send + Sync>;
