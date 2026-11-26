use std::sync::Arc;

use crate::pull_request::Sha;

/// Someone the host lists among a PR's reviewers: a person, or a team asked to review.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reviewer {
    name: Arc<str>,
    avatar_url: Option<Arc<str>>,
    kind: ReviewerKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReviewerKind {
    Person,
    Team,
}

impl Reviewer {
    pub fn person(login: &str, avatar_url: Option<&str>) -> Self {
        Self::new(login, avatar_url, ReviewerKind::Person)
    }

    pub fn team(name: &str, avatar_url: Option<&str>) -> Self {
        Self::new(name, avatar_url, ReviewerKind::Team)
    }

    fn new(name: &str, avatar_url: Option<&str>, kind: ReviewerKind) -> Self {
        Self {
            name: name.into(),
            avatar_url: avatar_url.map(Arc::from),
            kind,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn avatar_url(&self) -> Option<&Arc<str>> {
        self.avatar_url.as_ref()
    }

    pub fn kind(&self) -> ReviewerKind {
        self.kind
    }
}

/// Where one reviewer stands on the PR.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stance {
    /// The review that counts on GitHub: the latest approval or change request, else the latest review.
    Reviewed { outcome: ReviewOutcome, commit: Sha },
    /// Asked to review, with no review yet.
    Requested,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Whose {
    Viewers,
    Someone,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Standing {
    pub reviewer: Reviewer,
    pub stance: Stance,
    whose: Whose,
}

impl Standing {
    pub fn of(reviewer: Reviewer, whose: Whose, stance: Stance) -> Self {
        Self {
            reviewer,
            stance,
            whose,
        }
    }

    pub fn is_viewer(&self) -> bool {
        self.whose == Whose::Viewers
    }

    fn is_by(&self, reviewer: &Reviewer) -> bool {
        self.reviewer.name() == reviewer.name()
    }

    fn status(&self) -> ReviewerStatus {
        match &self.stance {
            Stance::Reviewed { outcome, .. } => ReviewerStatus::of(*outcome),
            Stance::Requested => ReviewerStatus::Waiting,
        }
    }

    /// The sidebar's row for this reviewer; a review of a commit before `pr_head` is marked old.
    pub(super) fn line(&self, pr_head: Option<&Sha>) -> ReviewerLine {
        let age = match (&self.stance, pr_head) {
            (Stance::Reviewed { commit, .. }, Some(head)) if head != commit => {
                StanceAge::BeforeNewCommits
            }
            _ => StanceAge::Current,
        };
        ReviewerLine {
            name: if self.is_viewer() {
                "You".into()
            } else {
                Arc::clone(&self.reviewer.name)
            },
            avatar_url: self.reviewer.avatar_url.clone(),
            kind: self.reviewer.kind,
            status: self.status(),
            age,
            is_viewer: self.is_viewer(),
        }
    }

    /// The viewer first, then what blocks the PR, then what lets it through, then the rest.
    fn rank(&self) -> u8 {
        if self.is_viewer() {
            return 0;
        }
        match &self.stance {
            Stance::Reviewed { outcome, .. } => match outcome {
                ReviewOutcome::ChangesRequested => 1,
                ReviewOutcome::Approved => 2,
                ReviewOutcome::Commented => 3,
                ReviewOutcome::Dismissed => 4,
            },
            Stance::Requested => 5,
        }
    }
}

/// One review as the host lists it among a PR's latest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewRecord {
    pub reviewer: Reviewer,
    pub outcome: ReviewOutcome,
    pub commit: Sha,
}

/// What the host says of a PR's reviewers, before it is sorted into standings.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReviewersOnHost {
    pub viewer_login: String,
    /// The PR's author answers threads in reviews of their own; they are not a reviewer.
    pub author_login: Option<String>,
    /// Each reviewer's latest review.
    pub latest: Vec<ReviewRecord>,
    /// Each reviewer's latest approval or change request, which a later comment does not undo.
    pub verdicts: Vec<ReviewRecord>,
    /// Who is asked to review now. One who reviewed already keeps the review that counts.
    pub requested: Vec<Reviewer>,
}

impl ReviewersOnHost {
    pub fn standings(self) -> Vec<Standing> {
        let whose = |reviewer: &Reviewer| {
            if reviewer.name() == self.viewer_login {
                Whose::Viewers
            } else {
                Whose::Someone
            }
        };
        let is_author = |reviewer: &Reviewer| self.author_login.as_deref() == Some(reviewer.name());
        let mut standings: Vec<Standing> = Vec::new();
        let is_listed = |standings: &[Standing], reviewer: &Reviewer| {
            standings.iter().any(|standing| standing.is_by(reviewer))
        };
        for record in self.verdicts.iter().chain(&self.latest) {
            if is_author(&record.reviewer) || is_listed(&standings, &record.reviewer) {
                continue;
            }
            standings.push(Standing::of(
                record.reviewer.clone(),
                whose(&record.reviewer),
                Stance::Reviewed {
                    outcome: record.outcome,
                    commit: record.commit.clone(),
                },
            ));
        }
        for reviewer in &self.requested {
            if !is_author(reviewer) && !is_listed(&standings, reviewer) {
                standings.push(Standing::of(
                    reviewer.clone(),
                    whose(reviewer),
                    Stance::Requested,
                ));
            }
        }
        standings.sort_by_key(Standing::rank);
        standings
    }
}

/// The PR's reviews as one read of the host gives them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PullRequestReviews {
    /// The viewer's latest sent review, whatever it said.
    pub viewer_last: Option<SubmittedReview>,
    pub standings: Vec<Standing>,
}

/// Whether a reviewer's stance is about the PR as it is now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StanceAge {
    Current,
    /// The review saw a commit the PR has moved past.
    BeforeNewCommits,
}

/// What the sidebar says of one reviewer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReviewerStatus {
    Approved,
    ChangesRequested,
    Commented,
    Dismissed,
    Waiting,
}

/// Where the viewer stands, for the sidebar's lead of the reviews.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct YourReview {
    pub status: ReviewerStatus,
    /// Whether the PR has commits the viewer's last sent review did not see.
    pub age: StanceAge,
    /// Done at the tour's head with no new review sent.
    pub is_marked_done: bool,
    pub avatar_url: Option<Arc<str>>,
    last_sent: Option<SubmittedReview>,
}

impl YourReview {
    pub(super) fn of(
        standing: Option<&Standing>,
        last_sent: Option<&SubmittedReview>,
        age: StanceAge,
        is_marked_done: bool,
    ) -> Option<Self> {
        let status = match (standing.map(Standing::status), last_sent) {
            (Some(ReviewerStatus::Waiting) | None, Some(last)) => ReviewerStatus::of(last.outcome),
            (Some(status), _) => status,
            (None, None) => return None,
        };
        Some(Self {
            status,
            age: if status == ReviewerStatus::Waiting {
                StanceAge::Current
            } else {
                age
            },
            is_marked_done,
            avatar_url: standing.and_then(|standing| standing.reviewer.avatar_url.clone()),
            last_sent: last_sent.cloned(),
        })
    }

    pub fn title(&self) -> &'static str {
        match self.status {
            ReviewerStatus::Approved => "You approved",
            ReviewerStatus::ChangesRequested => "You requested changes",
            ReviewerStatus::Commented => "You commented",
            ReviewerStatus::Dismissed => "Your review was dismissed",
            ReviewerStatus::Waiting => "Your review is requested",
        }
    }

    /// When the last review was sent, and that its new commits were checked. New commits not checked
    /// are the head of the your-review station.
    pub fn since(&self) -> String {
        let Some(last) = &self.last_sent else {
            return String::new();
        };
        let day = last.submitted_at.split('T').next().unwrap_or_default();
        let after = if self.is_marked_done {
            " · new commits checked"
        } else {
            ""
        };
        format!("Last review {day}{after}")
    }

    /// What the viewer wrote with their last review; empty when nothing.
    pub fn body(&self) -> &str {
        self.last_sent
            .as_ref()
            .map_or("", |last| last.body.as_str())
    }

    pub fn body_is_long(&self) -> bool {
        self.last_sent
            .as_ref()
            .is_some_and(SubmittedReview::body_is_long)
    }
}

impl ReviewerStatus {
    fn of(outcome: ReviewOutcome) -> Self {
        match outcome {
            ReviewOutcome::Approved => Self::Approved,
            ReviewOutcome::ChangesRequested => Self::ChangesRequested,
            ReviewOutcome::Commented => Self::Commented,
            ReviewOutcome::Dismissed => Self::Dismissed,
        }
    }
}

/// One row of the sidebar's reviews: the viewer is named You.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ReviewerLine {
    pub name: Arc<str>,
    pub avatar_url: Option<Arc<str>>,
    pub kind: ReviewerKind,
    pub status: ReviewerStatus,
    pub age: StanceAge,
    pub is_viewer: bool,
}

/// How a review that is on the host ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewOutcome {
    Approved,
    ChangesRequested,
    Commented,
    /// Someone took the review back; it no longer counts for or against the PR.
    Dismissed,
}

impl SubmittedReview {
    /// Too tall for the sidebar as it is, so it is shown clamped to `CLAMPED_BODY_LINES`.
    pub fn body_is_long(&self) -> bool {
        self.body.lines().count() > CLAMPED_BODY_LINES
            || self.body.chars().count() > LONG_BODY_CHARS
    }
}

pub const CLAMPED_BODY_LINES: usize = 4;

/// About what a sidebar line holds times the clamped lines; a longer text wraps past them.
const LONG_BODY_CHARS: usize = 200;

/// The viewer's latest sent review of a PR, as the host holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubmittedReview {
    pub outcome: ReviewOutcome,
    /// What the reviewer wrote with it; empty when nothing.
    pub body: String,
    /// GitHub's UTC time, like `2026-11-05T13:35:33Z`.
    pub submitted_at: String,
    /// The PR's head when the review was sent.
    pub commit: Sha,
}
