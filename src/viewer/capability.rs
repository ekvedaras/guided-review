/// Something a code host may lack, handed out by `CodeHost` as its port or `None`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Capability {
    ViewedMarks,
    Merging,
    PullRequestReactions,
    PictureUploads,
    IssueTracker,
}

impl Capability {
    pub const ALL: [Self; 5] = [
        Self::ViewedMarks,
        Self::Merging,
        Self::PullRequestReactions,
        Self::PictureUploads,
        Self::IssueTracker,
    ];

    const fn bit(self) -> u8 {
        1 << self as u8
    }
}

impl std::fmt::Display for Capability {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::ViewedMarks => "viewed marks",
            Self::Merging => "merging",
            Self::PullRequestReactions => "reactions on a PR",
            Self::PictureUploads => "picture uploads",
            Self::IssueTracker => "issue tracker",
        })
    }
}

/// The capabilities a code host offers, read once from its ports (`HostCapabilities::of`). The
/// review's work is planned on the UI thread and run with the ports lent on another, so the review
/// cannot hold a port; it holds this. Each part of a review that shows a capability is built from it,
/// so none shows a feature its host lacks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HostCapabilities {
    offered: u8,
}

impl HostCapabilities {
    pub(super) fn offering(offered: impl IntoIterator<Item = Capability>) -> Self {
        Self {
            offered: offered
                .into_iter()
                .fold(0, |offered, capability| offered | capability.bit()),
        }
    }

    pub fn has(self, capability: Capability) -> bool {
        self.offered & capability.bit() != 0
    }
}
