use super::ports::CodeHost;

/// Which optional capabilities a code host offers, read once from its ports. The review's work is
/// planned on the UI thread and run with the ports lent on another, so the review cannot hold a port;
/// it holds this. A review is built with it, so none shows a feature its host lacks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HostCapabilities {
    viewed_marks: bool,
    pull_request_reactions: bool,
    merging: bool,
    picture_uploads: bool,
    issue_tracker: bool,
}

impl HostCapabilities {
    pub fn of(host: &dyn CodeHost) -> Self {
        Self {
            viewed_marks: host.viewed_marks().is_some(),
            pull_request_reactions: host.pull_request_reactions().is_some(),
            merging: host.merging().is_some(),
            picture_uploads: host.picture_uploads().is_some(),
            issue_tracker: host.issue_tracker().is_some(),
        }
    }

    pub(super) fn has_viewed_marks(self) -> bool {
        self.viewed_marks
    }

    pub(super) fn has_pull_request_reactions(self) -> bool {
        self.pull_request_reactions
    }

    pub(super) fn has_merging(self) -> bool {
        self.merging
    }

    pub(super) fn has_picture_uploads(self) -> bool {
        self.picture_uploads
    }

    pub(super) fn has_issue_tracker(self) -> bool {
        self.issue_tracker
    }
}
