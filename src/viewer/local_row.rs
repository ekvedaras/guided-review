use crate::local_work_tree::OnDisk;
use crate::pull_request::PullRequestUrl;
use crate::review_subject::LocalChangeSet;

/// A local change set the switcher lists: one branch of one working tree, with what the reviewer
/// needs to tell it from another without opening it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalRow {
    pub change_set: LocalChangeSet,
    /// Commits on the branch that are not on the default branch. `None` when git could not say.
    pub commits_ahead: Option<usize>,
    /// Changed and untracked files that are not committed. `None` when git could not say.
    pub uncommitted_files: Option<usize>,
    /// Where the folder is against the snapshot of the review on disk; `None` when there is no review yet.
    pub on_disk: Option<OnDisk>,
    /// The open PR whose head is the branch, when the host said so.
    pub open_pr: Option<PullRequestUrl>,
}

impl LocalRow {
    /// What the row says of the folder when the review on disk is no longer of it.
    pub const fn disk_label(&self) -> Option<&'static str> {
        match self.on_disk {
            Some(OnDisk::Changed) => Some("changed on disk"),
            Some(OnDisk::OnAnotherBranch(_)) => Some("on another branch"),
            Some(OnDisk::BranchGone) => Some("branch gone"),
            Some(OnDisk::Same) | None => None,
        }
    }

    /// A review of a branch that is gone cannot be updated, only dropped.
    pub const fn offers_drop_review(&self) -> bool {
        matches!(self.on_disk, Some(OnDisk::BranchGone))
    }
}
