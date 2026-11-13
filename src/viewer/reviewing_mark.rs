use super::finish_review::SubmittedReview;
use super::reactions::{Reaction, ReactionChange};
use crate::pull_request::Sha;

/// What the viewer puts on the PR itself while the reviewer reviews it.
pub const REVIEWING_MARK: Reaction = Reaction::Eyes;

/// The 👀 on the PR, which tells its author the reviewer is on it. It goes on once the PR is known
/// to be someone else's and the reviewer's last review did not see the tour's head, and comes off
/// when a review of the reviewer's is sent, from the app or the web. Not saved.
#[derive(Debug)]
pub(super) struct ReviewingMark {
    tour_head: Sha,
    authorship: Authorship,
    /// The latest sent review as first read after the review opened: a later one is sent since.
    earlier_review: EarlierReview,
    is_finished: bool,
    /// The 👀 was asked for, so it is on GitHub or on its way there.
    is_asked_for: bool,
    to_send: Option<ReactionChange>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Authorship {
    Unknown,
    SomeoneElse,
    Viewer,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum EarlierReview {
    NotReadYet,
    None,
    Sent { at: String, commit: Sha },
}

impl ReviewingMark {
    pub(super) fn new(tour_head: Sha) -> Self {
        Self {
            tour_head,
            authorship: Authorship::Unknown,
            earlier_review: EarlierReview::NotReadYet,
            is_finished: false,
            is_asked_for: false,
            to_send: None,
        }
    }

    pub(super) fn viewer_authorship_is(&mut self, authored: bool) {
        if self.authorship != Authorship::Unknown {
            return;
        }
        self.authorship = if authored {
            Authorship::Viewer
        } else {
            Authorship::SomeoneElse
        };
        self.put_on_when_due();
    }

    pub(super) fn last_review_is(&mut self, last: Option<&SubmittedReview>) {
        let read = match last {
            Some(review) => EarlierReview::Sent {
                at: review.submitted_at.clone(),
                commit: review.commit.clone(),
            },
            None => EarlierReview::None,
        };
        if self.earlier_review == EarlierReview::NotReadYet {
            self.earlier_review = read;
            self.put_on_when_due();
            return;
        }
        let is_sent_since = match (&read, &self.earlier_review) {
            (EarlierReview::Sent { at, .. }, EarlierReview::Sent { at: earlier, .. }) => {
                at != earlier
            }
            (EarlierReview::Sent { .. }, _) => true,
            _ => false,
        };
        if is_sent_since && !self.is_finished {
            self.is_finished = true;
            self.to_send = self.is_asked_for.then_some(ReactionChange::Remove);
        }
    }

    /// What GitHub must be told next; `None` when it shows the mark as it should be.
    pub(super) fn change_to_send(&self) -> Option<ReactionChange> {
        self.to_send
    }

    /// A change that went through. One asked for since it was sent is still to send.
    pub(super) fn sent(&mut self, change: ReactionChange) {
        if self.to_send == Some(change) {
            self.to_send = None;
        }
    }

    fn put_on_when_due(&mut self) {
        let is_reviewed_at_tour_head = matches!(
            &self.earlier_review,
            EarlierReview::Sent { commit, .. } if *commit == self.tour_head
        );
        let is_due = self.authorship == Authorship::SomeoneElse
            && self.earlier_review != EarlierReview::NotReadYet
            && !is_reviewed_at_tour_head
            && !self.is_finished;
        if is_due {
            self.is_asked_for = true;
            self.to_send = Some(ReactionChange::Add);
        }
    }
}
