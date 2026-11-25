use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::app_folders::AppFolders;
use crate::code_source::CodeSource;
use crate::github::GitHub;
use crate::local_git::LocalGit;
use crate::make_basic_tour::make_basic_tour;
use crate::pick_code_source::pick_code_source;
use crate::pull_request::{ChangeTips, PullRequestUrl};
use crate::repin::Repin;
use crate::review_subject::ReviewSubject;
use crate::tour::{Tour, TourPin, ToursOnDisk};
use crate::viewer::{
    ChangeTipsPort, PickCodeSource, ReviewStateFile, let_go_of_unread_snapshots, repin_tour,
};

use super::CodeAtTips;
use super::hosts::code_reader_of;

/// The PR adapter: GitHub holds the PR's tips and title, and its code is read from a clone found from
/// `current_dir`, else from GitHub.
pub(super) struct OpenedPr {
    pr: PullRequestUrl,
    folders: AppFolders,
    current_dir: PathBuf,
    github: GitHub,
}

impl OpenedPr {
    pub(super) fn in_folders(
        pr: &PullRequestUrl,
        folders: &AppFolders,
        current_dir: &Path,
    ) -> Self {
        Self {
            pr: pr.clone(),
            folders: folders.clone(),
            current_dir: current_dir.to_path_buf(),
            github: code_reader_of(pr.host),
        }
    }

    pub(super) fn subject(&self) -> ReviewSubject {
        ReviewSubject::from(self.pr.clone())
    }

    pub(super) const fn github(&self) -> &GitHub {
        &self.github
    }

    pub(super) const fn folders(&self) -> &AppFolders {
        &self.folders
    }

    /// The tour `repin` moves: the guided tour when there is one, else the basic tour. With neither it is
    /// the guided path, which is the file the reviewer is told is missing.
    fn tour_to_repin_path(&self) -> PathBuf {
        match ToursOnDisk::of(&self.folders, &self.subject()) {
            ToursOnDisk::Basic => Tour::basic_path_for(&self.folders, &self.subject()),
            _ => Tour::path_for(&self.folders, &self.subject()),
        }
    }

    /// Makes the PR's basic tour at the PR's tips now, unless a tour is on disk already; true when made.
    pub(super) fn make_basic_tour_if_no_tour(&self) -> anyhow::Result<bool> {
        let subject = self.subject();
        if ToursOnDisk::of(&self.folders, &subject) != ToursOnDisk::None {
            return Ok(false);
        }
        let code = self.at_live_tips()?;
        let title = self.github.pull_request_title(&self.pr)?;
        make_basic_tour(
            &self.folders,
            &subject,
            code.source.as_ref(),
            &code.tips,
            &title,
        )?;
        Ok(true)
    }

    /// The PR as GitHub has it now.
    pub(super) fn at_live_tips(&self) -> anyhow::Result<CodeAtTips> {
        let tips = self.github.pull_request_tips(&self.pr)?;
        self.at(tips)
    }

    /// The tour is read at the commits it is pinned to, even when the PR has moved on since.
    pub(super) fn code_at(&self, pin: &TourPin) -> anyhow::Result<CodeAtTips> {
        self.at(pin.pinned_tips())
    }

    fn at(&self, tips: ChangeTips) -> anyhow::Result<CodeAtTips> {
        let source = pick_code_source(&self.github, &self.pr, &tips, &self.current_dir)?;
        Ok(CodeAtTips { tips, source })
    }

    /// Picks the code source again for any tips, as the app does when the PR moves.
    pub(super) fn code_source_picker(&self) -> PickCodeSource {
        let (github, pr, current_dir) = (
            self.github.clone(),
            self.pr.clone(),
            self.current_dir.clone(),
        );
        Arc::new(move |tips| pick_code_source(&github, &pr, tips, &current_dir))
    }

    /// `repin`: the guided tour, else the basic one, moves to the PR's head on GitHub.
    pub(super) fn repin(&self) -> anyhow::Result<Repin> {
        let tour_path = self.tour_to_repin_path();
        // Fails before any network call when there is no tour yet.
        Tour::load_of(&tour_path, &self.subject())?;
        let (_, repinned, _) = repin_to_live_tips(
            &self.folders,
            &self.pr,
            &tour_path,
            &self.github,
            &self.code_source_picker(),
        )?;
        Ok(repinned)
    }

    /// What an open of the PR's review tidies first: the snapshots of a moved local review that the
    /// PR's review reads no more.
    pub(super) fn before_open(&self) {
        if let Err(error) = let_go_of_unread_snapshots(&self.folders, &self.pr) {
            // A ref kept too long only waits for the next open; the review reads the same either way.
            eprintln!(
                "could not let go of the snapshots {} no longer reads: {error:#}",
                self.pr
            );
        }
    }

    /// The next open of the PR, from the app's own switcher or a restart, looks for a clone from here.
    pub(super) fn after_open(&self) -> anyhow::Result<()> {
        ReviewStateFile::for_subject(&self.folders, &self.subject())
            .remember_folder(&self.current_dir)
    }
}

/// Moves the tour at `tour_path` to the PR's head on GitHub; the code source it read the new head with
/// comes back for the review built on it.
pub(super) fn repin_to_live_tips(
    folders: &AppFolders,
    pr: &PullRequestUrl,
    tour_path: &Path,
    github: &dyn ChangeTipsPort,
    pick_code_source: &PickCodeSource,
) -> anyhow::Result<(Tour, Repin, Box<dyn CodeSource>)> {
    let subject = ReviewSubject::from(pr.clone());
    let tips = github.pull_request_tips(pr)?;
    let source = pick_code_source(&tips)?;
    let base = source.merge_base(&tips.base_branch_tip, &tips.head)?;
    let (tour, repinned) = repin_tour(
        folders,
        &subject,
        tour_path,
        &ReviewStateFile::for_subject(folders, &subject),
        source.as_ref(),
        &base,
        &tips.head,
    )?;
    Ok((tour, repinned, source))
}

/// Where an agent that writes the PR's tour works: the clone the PR was last opened from, else one
/// another PR of the repo was opened from, else the clone `config.toml` names, else the reviewer's
/// home, where the skill reads code through `show`.
pub(super) fn writer_folder(pr: &PullRequestUrl, folders: &AppFolders) -> PathBuf {
    let subject = ReviewSubject::from(pr.clone());
    let own = ReviewStateFile::for_subject(folders, &subject)
        .opened_from()
        .ok()
        .flatten();
    let others = ReviewStateFile::folders_opened_from_in(folders, pr);
    // An unreadable state file or config only costs the clone: the skill still reads code through `show`.
    own.into_iter()
        .chain(others)
        .chain([folders.home().to_path_buf()])
        .find_map(|folder| LocalGit::find(&pr.repo, &folder).ok().flatten())
        .map_or_else(
            || folders.home().to_path_buf(),
            |clone| clone.root().to_path_buf(),
        )
}
