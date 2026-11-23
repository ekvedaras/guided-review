//! Where the review agent works: the tour agent's own folder when it already holds the reviewed head and
//! nothing else, else a worktree the agent makes itself.

use std::path::{Path, PathBuf};

use crate::pull_request::Sha;
use crate::review_subject::ReviewSubject;

/// What the app needs to know about a folder's git checkout to decide where the review agent works.
pub trait CheckoutPort: Send + Sync {
    /// The commit the folder's checkout stands at. Fails when the folder is no git checkout.
    fn head_of(&self, folder: &Path) -> anyhow::Result<Sha>;

    /// Whether the checkout has changes that are not committed, new files included.
    fn has_uncommitted_changes(&self, folder: &Path) -> anyhow::Result<bool>;
}

/// The folder the review agent is started in, and whether it must make a worktree first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReviewPlace {
    /// The subject's tour agent folder, as it is: it stands at the reviewed head with nothing uncommitted.
    TourAgentFolder(PathBuf),
    /// The agent starts in the subject's clone and makes its own review worktree at the reviewed head with
    /// Claude Code's worktree tool, so the project's worktree hooks run.
    OwnWorktree { clone: PathBuf },
}

impl ReviewPlace {
    /// The tour agent's folder when it is a checkout at `head` with nothing uncommitted, else the clone.
    /// Fails when the clone is needed and is no git checkout.
    pub fn choose(
        subject: &ReviewSubject,
        tour_agent_folder: Option<&Path>,
        clone: &Path,
        head: &Sha,
        checkouts: &dyn CheckoutPort,
    ) -> anyhow::Result<Self> {
        if let Some(folder) = tour_agent_folder
            && Self::is_clean_at(folder, head, checkouts)
        {
            return Ok(Self::TourAgentFolder(folder.to_path_buf()));
        }
        anyhow::ensure!(
            checkouts.head_of(clone).is_ok(),
            "There is no clone on this Mac to check the code of {subject} out of: open the PR from a clone of its repository, then run the AI review again"
        );
        Ok(Self::OwnWorktree {
            clone: clone.to_path_buf(),
        })
    }

    /// A folder that is no checkout, or that git cannot read, is not clean: the agent then makes its own.
    fn is_clean_at(folder: &Path, head: &Sha, checkouts: &dyn CheckoutPort) -> bool {
        checkouts
            .head_of(folder)
            .is_ok_and(|standing| standing == *head)
            && checkouts
                .has_uncommitted_changes(folder)
                .is_ok_and(|is_dirty| !is_dirty)
    }

    /// The folder the agent starts in.
    pub fn folder(&self) -> &Path {
        match self {
            Self::TourAgentFolder(folder) => folder,
            Self::OwnWorktree { clone } => clone,
        }
    }
}
