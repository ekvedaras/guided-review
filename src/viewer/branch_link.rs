use std::time::Duration;

use crate::app_folders::AppFolders;
use crate::local_reviews_on_disk::local_reviews_on_disk;
use crate::pull_request::PullRequestUrl;
use crate::review_subject::{BranchName, LocalChangeSet, ReviewSubject};

use super::move_offer::{MoveFrom, MoveHereOffer, MoveOffer, StartedMove};
use super::ports::{HostError, PullOfBranch};
use super::review_state::{ReviewState, ReviewStateFile};
use super::saved_state::SavedState;
use super::work_agenda::PULL_OF_BRANCH_RETRY;

/// What a review knows of the other side of Move to PR, and so what it offers. Not saved but the
/// declines, which live in the local review's state file.
pub(super) enum BranchLink {
    /// A local review, and the open PR GitHub last said is opened from its branch.
    OfLocalReview {
        change_set: LocalChangeSet,
        pull: Option<PullOfBranch>,
    },
    /// A PR's review, and the local review on disk of the branch the PR is opened from.
    OfPr {
        pr: PullRequestUrl,
        /// `None` until the review is told; the head branch is looked up only when one is on disk.
        local_reviews: Option<LocalReviewsOfRepo>,
        of_head_branch: Option<LocalChangeSet>,
        /// Each failed look for the head branch in a row waits longer for the next.
        failed_looks: u32,
    },
}

/// What a review offers of its branch's other side; a button beside each move declines it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BranchOffer {
    /// A local review whose branch has no open PR yet: its agent can be asked to open one.
    OpenPr,
    /// A local review whose branch has an open PR: Move to it, or Keep local review.
    MoveToPr(MoveOffer),
    /// A PR's review with a local review of its head branch on disk: Move it here, or keep it separate.
    MoveHere(MoveHereOffer),
}

impl BranchLink {
    pub(super) fn of(subject: &ReviewSubject) -> Self {
        match subject {
            ReviewSubject::PullRequest(pr) => Self::OfPr {
                pr: pr.clone(),
                local_reviews: None,
                of_head_branch: None,
                failed_looks: 0,
            },
            ReviewSubject::Local(change_set) => Self::OfLocalReview {
                change_set: change_set.clone(),
                pull: None,
            },
        }
    }

    pub(super) fn local_reviews_are(&mut self, reviews: LocalReviewsOfRepo) {
        if let Self::OfPr { local_reviews, .. } = self {
            *local_reviews = Some(reviews);
        }
    }

    pub(super) fn pull_of_branch_is(&mut self, found: Option<PullOfBranch>) {
        if let Self::OfLocalReview { pull, .. } = self {
            *pull = found;
        }
    }

    /// When to look again after a failed look; a look for a PR is a convenience, so it is silent.
    pub(super) fn pull_looked_up(
        &mut self,
        found: Result<Option<PullOfBranch>, HostError>,
    ) -> Option<Duration> {
        match found {
            Ok(found) => {
                self.pull_of_branch_is(found);
                None
            }
            Err(_) => Some(PULL_OF_BRANCH_RETRY),
        }
    }

    /// Only a PR's review with local reviews of its repo on disk asks GitHub for its head branch.
    pub(super) fn head_branch_to_look_up(&self) -> Option<&PullRequestUrl> {
        match self {
            Self::OfPr {
                pr,
                local_reviews: Some(reviews),
                ..
            } if !reviews.is_empty() => Some(pr),
            _ => None,
        }
    }

    /// When to look again after a failed look, silent as for a local review's look for its PR.
    pub(super) fn head_branch_looked_up(
        &mut self,
        branch: Result<Option<BranchName>, HostError>,
    ) -> Option<Duration> {
        let Self::OfPr {
            local_reviews,
            of_head_branch,
            failed_looks,
            ..
        } = self
        else {
            return None;
        };
        let Ok(branch) = branch else {
            *failed_looks += 1;
            return Some(head_branch_retry_after(*failed_looks));
        };
        *failed_looks = 0;
        *of_head_branch = branch.and_then(|branch| local_reviews.as_ref()?.of(&branch));
        None
    }

    pub(super) fn can_open_pr(&self) -> bool {
        matches!(self, Self::OfLocalReview { pull: None, .. })
    }

    /// A local review's declines are in its own state, so a decline made in either review hides both offers.
    pub(super) fn offer(&self, state: &ReviewState) -> Option<BranchOffer> {
        Some(match self {
            Self::OfLocalReview { pull: None, .. } => BranchOffer::OpenPr,
            Self::OfLocalReview {
                pull: Some(pull), ..
            } => {
                if state.has_declined_move_to(&pull.pr) {
                    return None;
                }
                BranchOffer::MoveToPr(MoveOffer::of(pull))
            }
            Self::OfPr {
                pr, of_head_branch, ..
            } => BranchOffer::MoveHere(MoveHereOffer {
                change_set: of_head_branch.clone()?,
                offer: MoveOffer {
                    pr: pr.clone(),
                    title: String::new(),
                },
            }),
        })
    }

    /// The move the button asks for, whichever review it is pressed in.
    pub(super) fn move_to_start(&self, state: &ReviewState) -> Option<StartedMove> {
        match (self, self.offer(state)?) {
            (Self::OfLocalReview { change_set, .. }, BranchOffer::MoveToPr(offer)) => {
                Some(StartedMove {
                    from: MoveFrom::TheLocalReview,
                    change_set: change_set.clone(),
                    offer,
                })
            }
            (_, BranchOffer::MoveHere(here)) => Some(StartedMove {
                from: MoveFrom::ThePr,
                change_set: here.change_set,
                offer: here.offer,
            }),
            _ => None,
        }
    }

    /// Keep local review, on either side's offer: the offer goes for good on both sides. A decline
    /// that could not be written leaves the PR's offer in place; a local review's goes all the same.
    pub(super) fn decline(&mut self, state: &mut SavedState) -> anyhow::Result<()> {
        match self {
            Self::OfLocalReview {
                pull: Some(pull), ..
            } if !state.has_declined_move_to(&pull.pr) => state.decline_move_to(&pull.pr),
            Self::OfPr {
                pr,
                local_reviews: Some(reviews),
                of_head_branch,
                ..
            } => {
                let Some(change_set) = of_head_branch.clone() else {
                    return Ok(());
                };
                reviews.decline(&change_set, pr)?;
                *of_head_branch = None;
                Ok(())
            }
            Self::OfLocalReview { .. } | Self::OfPr { .. } => Ok(()),
        }
    }
}

/// The local reviews on disk of a PR's repo whose move to the PR was not declined. A decline made
/// in the PR's review is written here, into the local review's own state file.
pub struct LocalReviewsOfRepo {
    folders: AppFolders,
    reviews: Vec<LocalChangeSet>,
}

impl LocalReviewsOfRepo {
    /// Reads the tour and state files only: no GitHub and no git.
    pub fn on_disk(folders: &AppFolders, pr: &PullRequestUrl) -> Self {
        Self {
            folders: folders.clone(),
            reviews: local_reviews_of_repo(folders, pr),
        }
    }

    fn is_empty(&self) -> bool {
        self.reviews.is_empty()
    }

    fn of(&self, branch: &BranchName) -> Option<LocalChangeSet> {
        self.reviews
            .iter()
            .find(|review| review.branch() == branch)
            .cloned()
    }

    fn decline(&mut self, change_set: &LocalChangeSet, pr: &PullRequestUrl) -> anyhow::Result<()> {
        let local = ReviewSubject::from(change_set.clone());
        ReviewStateFile::for_subject(&self.folders, &local).decline_move_to(pr)?;
        self.reviews.retain(|review| review != change_set);
        Ok(())
    }
}

/// Each failed look for a PR's head branch doubles the wait for the next, up to a cap, since nothing
/// but the move offer waits on it.
fn head_branch_retry_after(failures: u32) -> Duration {
    const LONGEST_WAIT_DOUBLINGS: u32 = 5;
    PULL_OF_BRANCH_RETRY * 2_u32.pow(failures.saturating_sub(1).min(LONGEST_WAIT_DOUBLINGS))
}

/// The local reviews on disk of the PR's repo whose move to the PR was not declined: one of them may be of
/// the branch the PR is opened from. Reads the tour and state files only: no GitHub and no git.
pub fn local_reviews_of_repo(folders: &AppFolders, pr: &PullRequestUrl) -> Vec<LocalChangeSet> {
    local_reviews_on_disk(folders)
        .into_iter()
        .filter(|review| review.repo() == Some(&pr.repo))
        .filter(|review| {
            let subject = ReviewSubject::from(review.clone());
            !ReviewStateFile::for_subject(folders, &subject).has_declined_move_to(pr)
        })
        .collect()
}
