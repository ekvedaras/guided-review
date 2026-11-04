use std::borrow::Cow;
use std::fmt;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::file_path::FilePath;
use crate::line_number::{InvalidLineNumber, LineNumber};
use crate::pull_request::{PullRequestUrl, Sha};

use super::ports::{GitHubError, PullRequestTipsPort, ReviewWritesPort};
use super::row_selection::SelectedLines;
use super::step_page::{LineLocation, Side};

/// Where a comment sits: on a whole file, or on one line or a range that may cross hunks and sides.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub enum CommentPlace {
    /// GitHub takes this for a picture.
    File(FilePath),
    Lines {
        start: LineLocation,
        end: LineLocation,
    },
}

impl CommentPlace {
    pub fn of(selected: &SelectedLines) -> Self {
        Self::Lines {
            start: selected.first().clone(),
            end: selected.last().clone(),
        }
    }

    pub fn on_file(path: &FilePath) -> Self {
        Self::File(path.clone())
    }

    pub fn is_file_comment(&self) -> bool {
        matches!(self, Self::File(_))
    }

    pub fn path(&self) -> &str {
        match self {
            Self::File(path) => path.as_str(),
            Self::Lines { end, .. } => end.path.as_str(),
        }
    }

    pub fn is_single_line(&self) -> bool {
        matches!(self, Self::Lines { start, end } if start == end)
    }
}

impl<'de> Deserialize<'de> for CommentPlace {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        match StoredPlace::deserialize(deserializer)? {
            StoredPlace::Current(CurrentPlace::File(path)) => Ok(Self::File(path)),
            StoredPlace::Current(CurrentPlace::Lines { start, end }) => {
                Ok(Self::Lines { start, end })
            }
            // Before places had kinds, a file comment was a place on line 0, which no line has.
            StoredPlace::Before { end, .. } if end.line == 0 => Ok(Self::File(end.path)),
            StoredPlace::Before { start, end } => Ok(Self::Lines {
                start: start.into_location().map_err(serde::de::Error::custom)?,
                end: end.into_location().map_err(serde::de::Error::custom)?,
            }),
        }
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum StoredPlace {
    Current(CurrentPlace),
    Before {
        start: BeforeLocation,
        end: BeforeLocation,
    },
}

/// A location as saved before lines were counted from 1.
#[derive(Deserialize)]
struct BeforeLocation {
    path: FilePath,
    side: Side,
    line: u32,
}

impl BeforeLocation {
    fn into_location(self) -> Result<LineLocation, InvalidLineNumber> {
        Ok(LineLocation {
            path: self.path,
            side: self.side,
            line: LineNumber::try_from(self.line)?,
        })
    }
}

#[derive(Deserialize)]
enum CurrentPlace {
    File(FilePath),
    Lines {
        start: LineLocation,
        end: LineLocation,
    },
}

/// Text the reviewer is writing and has not sent. Kept in the state file on every edit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommentDraft {
    pub place: CommentPlace,
    pub body: String,
    /// The tour head whose line numbers `place` uses. `None` in files from before it was kept,
    /// which are taken as current.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "head_or_none"
    )]
    pub written_at: Option<Sha>,
}

/// Files from before `written_at` was optional hold an empty text for "unknown".
fn head_or_none<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Sha>, D::Error> {
    Ok(Option::<Sha>::deserialize(deserializer)?.filter(|sha| !sha.is_empty()))
}

impl CommentDraft {
    pub fn is_placed_at(&self, head: &Sha) -> bool {
        self.written_at.as_ref().is_none_or(|at| at == head)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SendMode {
    /// Goes into the pending review, to be sent with the others.
    AddToReview,
    /// Posts at once, as a review of its own.
    CommentNow,
}

/// A comment GitHub did not take yet. It is sent again when GitHub can be reached.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueuedComment {
    pub draft: CommentDraft,
    pub mode: SendMode,
    pub failed_because: String,
}

/// Why a comment was not written, with nothing lost: the text stays where it was.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CommentRefusal {
    /// The tour is for an older head, so its lines may not be where GitHub has them.
    TourIsOld,
    /// A re-pin moved the tour since the comment was written: the text stays, but its line
    /// numbers belong to the old code.
    CodeMoved,
    NotInsideHunk,
    /// An Ask about a step or the whole tour has no lines a comment could sit on.
    NoLines,
    /// The Ask was answered about an older head; its lines may sit elsewhere now.
    AskIsOnOlderCode,
    /// Someone started a pending review since the last look, so "Comment now" is off.
    PendingReviewExists,
    /// A reply with no text has nothing to send.
    EmptyReply,
    /// An Ask still waiting for its first answer has nothing to put in a comment.
    NoAnswerYet,
    NoAccess(String),
}

impl CommentRefusal {
    /// Only a refusal that carries a reason has to build its text.
    pub fn message(&self) -> Cow<'static, str> {
        Cow::Borrowed(match self {
            Self::TourIsOld => "Update tour to comment",
            Self::CodeMoved => "Code moved, place the comment again",
            Self::NotInsideHunk => "GitHub takes comments only on lines of the diff",
            Self::NoLines => "Only an Ask about lines can become a comment",
            Self::AskIsOnOlderCode => {
                "This Ask is about older code: select the lines again to comment"
            }
            Self::PendingReviewExists => "There is a pending review now: add the comment to it",
            Self::EmptyReply => "Write the reply first",
            Self::NoAnswerYet => "This Ask has no answer yet",
            Self::NoAccess(reason) => return Cow::Owned(format!("No access to GitHub: {reason}")),
        })
    }
}

impl fmt::Display for CommentRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message())
    }
}

/// What stops a comment box from going out, shown under it. Not the review-wide `Problem`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CommentProblem {
    Refused(CommentRefusal),
    /// GitHub answered but made no thread.
    Rejected(Arc<str>),
    Failed(Arc<str>),
}

impl fmt::Display for CommentProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Refused(refusal) => refusal.fmt(f),
            Self::Rejected(reason) | Self::Failed(reason) => f.write_str(reason),
        }
    }
}

/// What happened to a comment the reviewer sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommentResult {
    Sent,
    /// GitHub could not be reached; the comment waits in the queue.
    Queued(String),
    Refused(CommentRefusal),
    /// GitHub answered but made no thread. Retrying would only leave more empty pending reviews,
    /// so the text stays a draft.
    Rejected(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Origin {
    Draft,
    Queue,
}

/// Everything sending one comment needs, owned, so it can run on a background thread.
pub struct CommentRequest {
    pub(super) origin: Origin,
    pub(super) draft: CommentDraft,
    pub(super) mode: SendMode,
    tour_head: Sha,
    pr: PullRequestUrl,
}

pub struct CommentOutcome {
    pub(super) request: CommentRequest,
    pub(super) how: Outcome,
}

pub(super) enum Outcome {
    Sent,
    Failed(String),
    Rejected(String),
    Refused(CommentRefusal),
}

impl CommentRequest {
    pub(super) fn new(
        origin: Origin,
        draft: CommentDraft,
        mode: SendMode,
        tour_head: Sha,
        pr: PullRequestUrl,
    ) -> Self {
        Self {
            origin,
            draft,
            mode,
            tour_head,
            pr,
        }
    }

    /// Looks at the PR's head again first: the tour may have fallen behind since the last check.
    pub fn run(
        self,
        github: &(impl PullRequestTipsPort + ReviewWritesPort + ?Sized),
    ) -> CommentOutcome {
        let how = self.send(github);
        CommentOutcome { request: self, how }
    }

    fn send(&self, github: &(impl PullRequestTipsPort + ReviewWritesPort + ?Sized)) -> Outcome {
        let pr = &self.pr;
        if !self.draft.is_placed_at(&self.tour_head) {
            return Outcome::Refused(CommentRefusal::CodeMoved);
        }
        match github.pull_request_tips(pr) {
            Ok(tips) if tips.head == self.tour_head => {}
            Ok(_) => return Outcome::Refused(CommentRefusal::TourIsOld),
            Err(error) => return Outcome::Failed(format!("{error:#}")),
        }
        let sent = match self.mode {
            SendMode::AddToReview => {
                github.add_review_thread(pr, &self.draft.place, &self.draft.body)
            }
            SendMode::CommentNow => match github.has_pending_review(pr) {
                Ok(false) => github.comment_now(pr, &self.draft.place, &self.draft.body),
                Ok(true) => return Outcome::Refused(CommentRefusal::PendingReviewExists),
                Err(error) => return classify(&error),
            },
        };
        sent.map_or_else(|error| classify(&error), |()| Outcome::Sent)
    }
}

fn classify(error: &GitHubError) -> Outcome {
    match error {
        GitHubError::ThreadNotCreated(reason) => Outcome::Rejected(reason.clone()),
        GitHubError::PendingReviewExists => Outcome::Refused(CommentRefusal::PendingReviewExists),
        GitHubError::NoAccess(reason) => Outcome::Refused(CommentRefusal::NoAccess(reason.clone())),
        GitHubError::Other(error) => Outcome::Failed(format!("{error:#}")),
    }
}
