use std::collections::HashSet;
use std::path::PathBuf;

use crate::app_folders::AppFolders;
use crate::config::Config;
use crate::local_git::LocalGit;
use crate::local_work_tree::LocalWorkTree;
use crate::opened_change_set::local_reviews_on_disk;
use crate::review_subject::{LocalChangeSet, ReviewSubject, WorkTreeRoot};
use crate::tour::Tour;
use crate::viewer::{LocalRow, ReviewStateFile, TourAgentPort};

/// The local change sets the switcher lists: the local reviews on disk, and the current branch of each
/// folder a PR was opened from, each clone Settings names and each folder a live Claude terminal works
/// in, when that branch has changes to review. Slow, so the app runs it off the UI thread. A folder
/// git cannot read, or a source that cannot be read, is left out: the list is only a convenience.
pub fn find_local_rows(
    folders: &AppFolders,
    config: &Config,
    agents: &dyn TourAgentPort,
) -> Vec<LocalRow> {
    let mut rows: Vec<LocalRow> = local_reviews_on_disk(folders)
        .into_iter()
        .map(|change_set| row_of_review(&change_set, folders))
        .collect();
    let reviewed: HashSet<LocalChangeSet> = rows.iter().map(|row| row.change_set.clone()).collect();
    rows.extend(
        folders_to_look_in(folders, config, agents)
            .iter()
            .filter_map(row_of_folder)
            .filter(|row| !reviewed.contains(&row.change_set)),
    );
    rows.sort_by_cached_key(|row| ReviewSubject::from(row.change_set.clone()).to_string());
    rows
}

/// The working trees the sources point into, each once.
fn folders_to_look_in(
    folders: &AppFolders,
    config: &Config,
    agents: &dyn TourAgentPort,
) -> Vec<WorkTreeRoot> {
    // Herdr that does not answer is the usual case for a reviewer who does not run it.
    let terminals = agents.live_terminals().unwrap_or_default();
    let paths: Vec<PathBuf> = ReviewStateFile::folders_opened_from_any_repo(folders)
        .into_iter()
        .chain(config.clone_folders())
        .chain(
            terminals
                .into_iter()
                .filter(|terminal| terminal.runs_claude)
                .map(|terminal| terminal.cwd),
        )
        .collect();
    let mut roots: Vec<WorkTreeRoot> = Vec::new();
    for path in paths {
        let Ok(work_tree) = LocalWorkTree::containing(&path) else {
            continue;
        };
        if !roots.contains(work_tree.root()) {
            roots.push(work_tree.root().clone());
        }
    }
    roots
}

/// The branch the folder is on, when it has changes to review.
fn row_of_folder(root: &WorkTreeRoot) -> Option<LocalRow> {
    let work_tree = LocalWorkTree::at(root.clone());
    let change_set = work_tree.change_set().ok()?;
    let glance = work_tree.glance().ok()?;
    (!glance.is_nothing_to_review()).then_some(LocalRow {
        change_set,
        commits_ahead: Some(glance.commits_ahead),
        uncommitted_files: Some(glance.uncommitted_files),
        on_disk: None,
    })
}

/// A review on disk is listed whatever its folder is now; its numbers are the snapshot's, which is what
/// opening it shows, and what the folder is now is told beside them.
fn row_of_review(change_set: &LocalChangeSet, folders: &AppFolders) -> LocalRow {
    let work_tree = LocalWorkTree::at(change_set.folder().clone());
    let change_set = LocalChangeSet::new(
        change_set.folder().clone(),
        change_set.branch().clone(),
        LocalGit::origin_of(change_set.folder().as_path()),
    );
    let subject = ReviewSubject::from(change_set.clone());
    let tips = Tour::load_of(&Tour::shown_path_for(folders, &subject), &subject)
        .ok()
        .map(|tour| tour.pin().pinned_tips());
    let facts = tips
        .as_ref()
        .and_then(|tips| work_tree.facts(&change_set, tips).ok());
    let on_disk = tips
        .as_ref()
        .and_then(|tips| work_tree.on_disk(&change_set, &tips.head).ok());
    LocalRow {
        commits_ahead: facts.as_ref().map(|facts| facts.commit_subjects.len()),
        uncommitted_files: facts.as_ref().map(|facts| facts.uncommitted_files.len()),
        change_set,
        on_disk,
    }
}
