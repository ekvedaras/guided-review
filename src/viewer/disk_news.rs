use crate::local_work_tree::OnDisk;
use crate::review_subject::BranchName;

/// What a local review says of its folder when it is no longer the snapshot the review reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiskNews {
    pub line: String,
    pub offer: Option<DiskOffer>,
}

/// What the reviewer can do about the news.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiskOffer {
    /// Re-pin the tour to a new snapshot.
    Update,
    /// Delete the review's tours, state file and snapshot ref.
    DropReview,
}

impl DiskNews {
    /// `None` while the folder is the snapshot: there is nothing to say.
    pub fn of(on_disk: &OnDisk, branch: &BranchName) -> Option<Self> {
        let branch = branch.as_str();
        match on_disk {
            OnDisk::Same => None,
            OnDisk::Changed => Some(Self {
                line: "Changed on disk".to_owned(),
                offer: Some(DiskOffer::Update),
            }),
            OnDisk::OnAnotherBranch(other) => Some(Self {
                line: match other {
                    Some(other) => format!(
                        "The folder is on {} now; this review is of {branch}",
                        other.as_str()
                    ),
                    None => format!("The folder is on no branch now; this review is of {branch}"),
                },
                offer: None,
            }),
            OnDisk::BranchGone => Some(Self {
                line: format!("The branch {branch} is gone"),
                offer: Some(DiskOffer::DropReview),
            }),
        }
    }
}
