use std::collections::HashSet;
use std::path::PathBuf;

use crate::app_folders::AppFolders;
use crate::config::Config;
use crate::local_git::LocalGit;
use crate::local_reviews_on_disk::local_reviews_on_disk;
use crate::local_work_tree::LocalWorkTree;
use crate::pull_request::PullRequestUrl;
use crate::review_subject::{LocalChangeSet, ReviewSubject, WorkTreeRoot};
use crate::tour::Tour;
use crate::viewer::{LocalRow, PullsOfBranchPort, ReviewStateFile, TourAgentPort};

/// The local change sets the switcher lists: the local reviews on disk, and the current branch of each
/// folder a PR was opened from, each clone Settings names and each folder a live Claude terminal works
/// in, when that branch has changes to review. Slow, so the app runs it off the UI thread. A folder
/// git cannot read, or a source that cannot be read, is left out: the list is only a convenience.
pub fn find_local_rows(
    folders: &AppFolders,
    config: &Config,
    agents: &dyn TourAgentPort,
    pulls: &dyn PullsOfBranchPort,
) -> Vec<LocalRow> {
    let reviews: Vec<LocalChangeSet> = local_reviews_on_disk(folders).into_iter().collect();
    let roots = folders_to_look_in(folders, config, agents);
    // Each folder asks git on its own, a big work tree for seconds, so they are all asked at once.
    let (mut rows, of_folders) = std::thread::scope(|scope| {
        let of_folders = scope.spawn(|| in_parallel(&roots, row_of_folder));
        let rows: Vec<LocalRow> = in_parallel(&reviews, |change_set| {
            Some(row_of_review(change_set, folders))
        })
        .into_iter()
        .flatten()
        .collect();
        (rows, of_folders.join().unwrap_or_default())
    });
    let reviewed: HashSet<LocalChangeSet> = rows.iter().map(|row| row.change_set.clone()).collect();
    rows.extend(
        of_folders
            .into_iter()
            .flatten()
            .filter(|row| !reviewed.contains(&row.change_set)),
    );
    let open_prs = in_parallel(&rows, |row| {
        open_pr_of(&row.change_set, pulls).filter(|pr| {
            !ReviewStateFile::for_subject(folders, &ReviewSubject::from(row.change_set.clone()))
                .has_declined_move_to(pr)
        })
    });
    for (row, open_pr) in rows.iter_mut().zip(open_prs) {
        row.open_pr = open_pr;
    }
    rows.sort_by_cached_key(|row| ReviewSubject::from(row.change_set.clone()).to_string());
    rows
}

/// `look` of each item, each on its own thread, in the items' order. A look that panics gives
/// nothing, so the rows it would add are left out like any folder git cannot read.
fn in_parallel<Item: Sync, Found: Send + Default>(
    items: &[Item],
    look: impl Fn(&Item) -> Found + Sync,
) -> Vec<Found> {
    std::thread::scope(|scope| {
        let looks: Vec<_> = items
            .iter()
            .map(|item| scope.spawn(|| look(item)))
            .collect();
        looks
            .into_iter()
            .map(|looking| looking.join().unwrap_or_default())
            .collect()
    })
}

/// The open PR whose head is the branch. A row is only a convenience, so a failed look names none.
fn open_pr_of(
    change_set: &LocalChangeSet,
    pulls: &dyn PullsOfBranchPort,
) -> Option<PullRequestUrl> {
    let repo = change_set.repo()?;
    pulls
        .open_pull_of_branch(repo, change_set.branch())
        .ok()
        .flatten()
        .map(|pull| pull.pr)
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
        open_pr: None,
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
        open_pr: None,
    }
}
