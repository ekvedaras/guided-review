//! A review subject opened to be read: the one place that tells a PR from a local change set for
//! where the code comes from, how a tour is first made and how it moves on. The CLI's commands, the
//! app's open and its Update ask an `OpenedSubject`; only the adapters inside know GitHub or git.

mod hosts;
mod local;
mod pull_request;

use std::path::{Path, PathBuf};
use std::sync::Arc;

pub use hosts::{EveryHost, adapter_of, host_of};
pub use local::OpenedChangeSet;

use crate::app_folders::AppFolders;
use crate::code_source::CodeSource;
use crate::local_git::LocalGit;
use crate::pieces::{self, ChangedFile};
use crate::pull_request::{ChangeTips, Sha};
use crate::repin::Repin;
use crate::review_subject::ReviewSubject;
use crate::tour::{Tour, TourPin};
use crate::viewer::{
    ChangeTipsPort, CodeHost, HostCapabilities, PickCodeSource, RepinnedTour, Review,
    ReviewBuilder, ReviewStateFile,
};
use pull_request::OpenedPr;

pub struct OpenedSubject(Adapter);

enum Adapter {
    PullRequest(OpenedPr),
    Local(OpenedChangeSet),
}

impl OpenedSubject {
    /// `clones_from` is the folder a PR's clone is searched from; a local change set reads its own
    /// folder. A local change set named by its key alone learns its repo from the clone's `origin`.
    pub fn in_folders(subject: &ReviewSubject, folders: &AppFolders, clones_from: &Path) -> Self {
        Self(match with_repo_of_its_clone(subject) {
            ReviewSubject::PullRequest(pr) => {
                Adapter::PullRequest(OpenedPr::in_folders(&pr, folders, clones_from))
            }
            ReviewSubject::Local(change_set) => {
                Adapter::Local(OpenedChangeSet::new(&change_set, folders))
            }
        })
    }

    pub fn subject(&self) -> ReviewSubject {
        match &self.0 {
            Adapter::PullRequest(opened) => opened.subject(),
            Adapter::Local(opened) => opened.subject(),
        }
    }

    /// Makes the basic tour now, unless a tour is on disk already; true when made. A PR's is made at
    /// the tips GitHub has now, a local change set's from a new snapshot of its folder.
    pub fn make_basic_tour_if_no_tour(&self) -> anyhow::Result<bool> {
        match &self.0 {
            Adapter::PullRequest(opened) => opened.make_basic_tour_if_no_tour(),
            Adapter::Local(opened) => opened.make_basic_tour_if_no_tour(),
        }
    }

    /// What `outline`, `show` and `check` read. A PR is read at the tips GitHub has now; a local change
    /// set at the snapshot its tour is pinned to, since the folder is not what the reviewer reads.
    pub fn code_to_read(&self) -> anyhow::Result<CodeAtTips> {
        match &self.0 {
            Adapter::PullRequest(opened) => opened.at_live_tips(),
            Adapter::Local(opened) => opened.at_pinned_tips(),
        }
    }

    /// The code at the tips a tour's pin names, as `refresh` reads a guided tour.
    pub fn code_at(&self, pin: &TourPin) -> anyhow::Result<CodeAtTips> {
        match &self.0 {
            Adapter::PullRequest(opened) => opened.code_at(pin),
            Adapter::Local(opened) => Ok(opened.code_at(pin)),
        }
    }

    /// `repin`: a PR's guided tour, else its basic one, moves to the PR's head on GitHub; a local
    /// review's shown tour moves to a new snapshot of its folder.
    pub fn repin(&self) -> anyhow::Result<Repin> {
        match &self.0 {
            Adapter::PullRequest(opened) => opened.repin(),
            Adapter::Local(opened) => Ok(opened.repin_to_new_snapshot()?.1),
        }
    }

    /// The app's open: the review of the shown tour at the code its pin names, made first when there is
    /// no tour. Slow, so the app runs it off the UI thread.
    pub fn open_review(&self) -> anyhow::Result<(Review, Box<dyn CodeSource>)> {
        self.make_basic_tour_if_no_tour()?;
        let subject = self.subject();
        let folders = self.folders();
        if let Adapter::PullRequest(opened) = &self.0 {
            opened.before_open();
        }
        let tour = Tour::load_of(&Tour::shown_path_for(folders, &subject), &subject)?;
        let code = match &self.0 {
            Adapter::PullRequest(opened) => opened.code_at(tour.pin())?,
            Adapter::Local(opened) => opened.code_of_shown(tour.pin())?,
        };
        let host = HostCapabilities::of(self.host().as_ref());
        let review =
            ReviewBuilder::new(folders, &subject, host).build(tour, code.source.as_ref())?;
        if let Adapter::PullRequest(opened) = &self.0 {
            opened.after_open()?;
        }
        Ok((review, code.source))
    }

    /// Picks the code source again for any tips, as the app does when the tour moves: a PR's clone or
    /// GitHub, a local change set's own folder, whose git holds the snapshot.
    pub fn code_source_picker(&self) -> PickCodeSource {
        match &self.0 {
            Adapter::PullRequest(opened) => opened.code_source_picker(),
            Adapter::Local(opened) => opened.code_source_picker(),
        }
    }

    /// The code host the review talks to. A local review has none to ask: its port refuses every call but
    /// the look-up of a PR of its branch, on the host its origin is on.
    pub fn host(&self) -> Arc<dyn CodeHost> {
        match &self.0 {
            Adapter::PullRequest(opened) => opened.host(),
            Adapter::Local(opened) => host_of(&opened.subject()),
        }
    }

    pub fn state_file(&self) -> ReviewStateFile {
        ReviewStateFile::for_subject(self.folders(), &self.subject())
    }

    fn folders(&self) -> &AppFolders {
        match &self.0 {
            Adapter::PullRequest(opened) => opened.folders(),
            Adapter::Local(opened) => opened.folders(),
        }
    }
}

/// The "Update tour" button's work: what the CLI's `repin` does, then a fresh `Review` on the result.
pub struct TourRepinner {
    pub folders: AppFolders,
    pub subject: ReviewSubject,
    pub pick_code_source: PickCodeSource,
    pub host: HostCapabilities,
}

impl TourRepinner {
    /// Blocks on the network and on git; the app runs it off the main thread.
    pub fn run(&self, host: &dyn ChangeTipsPort) -> anyhow::Result<RepinnedTour> {
        let (tour, source) =
            update_shown_tour(&self.subject, &self.folders, host, &self.pick_code_source)?;
        let review = ReviewBuilder::new(&self.folders, &self.subject, self.host)
            .build(tour, source.as_ref())?;
        Ok(RepinnedTour {
            review,
            source: source.into(),
        })
    }
}

/// Update in the app: the shown tour moves to a PR's head on GitHub, or to a new snapshot of a local
/// change set's folder. It takes the app's ports, not an `OpenedSubject`, so tests put fakes behind
/// them; the code source the new tips were read with comes back for the review built on them.
pub fn update_shown_tour(
    subject: &ReviewSubject,
    folders: &AppFolders,
    host: &dyn ChangeTipsPort,
    pick_code_source: &PickCodeSource,
) -> anyhow::Result<(Tour, Box<dyn CodeSource>)> {
    match subject {
        ReviewSubject::PullRequest(pr) => {
            let (tour, _, source) = pull_request::repin_to_live_tips(
                folders,
                pr,
                &Tour::shown_path_for(folders, subject),
                host,
                pick_code_source,
            )?;
            Ok((tour, source))
        }
        ReviewSubject::Local(change_set) => {
            let (tour, _) = OpenedChangeSet::new(change_set, folders).repin_to_new_snapshot()?;
            let source = pick_code_source(&tour.pin().pinned_tips())?;
            Ok((tour, source))
        }
    }
}

/// Where an agent that writes the subject's tour works: a local change set's own folder, a PR's clone
/// (see `pull_request::writer_folder`). Unlike the opened review's agents, it needs no review open.
pub fn writer_folder(subject: &ReviewSubject, folders: &AppFolders) -> PathBuf {
    match subject {
        ReviewSubject::PullRequest(pr) => pull_request::writer_folder(pr, folders),
        ReviewSubject::Local(change_set) => change_set.folder().as_path().to_path_buf(),
    }
}

/// A local key names no repo; its clone's `origin` does, so a PR of the repo finds the review without
/// git. A subject that knows its repo keeps it.
fn with_repo_of_its_clone(subject: &ReviewSubject) -> ReviewSubject {
    match subject {
        ReviewSubject::Local(change_set) if change_set.origin().is_none() => ReviewSubject::from(
            change_set
                .clone()
                .with_origin(LocalGit::origin_of(change_set.folder().as_path())),
        ),
        _ => subject.clone(),
    }
}

pub struct CodeAtTips {
    pub tips: ChangeTips,
    pub source: Box<dyn CodeSource>,
}

impl CodeAtTips {
    pub fn merge_base(&self) -> anyhow::Result<Sha> {
        self.source
            .merge_base(&self.tips.base_branch_tip, &self.tips.head)
    }

    /// The merge base and what changed from it to the head.
    pub fn pieces_since_merge_base(&self) -> anyhow::Result<(Sha, Vec<ChangedFile>)> {
        let base = self.merge_base()?;
        let files = pieces::cut(self.source.as_ref(), &base, &self.tips.head)?;
        Ok((base, files))
    }
}
