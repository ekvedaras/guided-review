use std::borrow::Cow;
use std::fmt;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::file_path::FilePath;
use crate::findings::{Anchor, Finding};
use crate::line_number::{InvalidLineNumber, LineNumber};
use crate::pull_request::Sha;

use super::line_location::{LineLocation, Side};

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
    pub fn on_file(path: &FilePath) -> Self {
        Self::File(path.clone())
    }

    /// Where a comment made from a finding goes; one about the whole change has no place.
    pub fn of_finding(anchor: &Anchor) -> Option<Self> {
        match anchor {
            Anchor::Change => None,
            Anchor::File { file } => Some(Self::on_file(file)),
            Anchor::Lines { file, side, lines } => {
                let at = |line| LineLocation {
                    path: file.clone(),
                    side: Side::from(*side),
                    line,
                };
                Some(Self::Lines {
                    start: at(lines.start),
                    end: at(lines.end),
                })
            }
        }
    }

    pub fn is_file_comment(&self) -> bool {
        matches!(self, Self::File(_))
    }

    pub fn path(&self) -> &FilePath {
        match self {
            Self::File(path) => path,
            Self::Lines { end, .. } => &end.path,
        }
    }

    /// The comment box's heading.
    pub fn describe(&self) -> String {
        match self {
            Self::File(path) => format!("Comment on {path}"),
            Self::Lines { start, end } if start == end => {
                format!("Comment on {} line {}", end.path, end.line)
            }
            Self::Lines { start, end } => format!(
                "Comment on {} lines {} to {}",
                end.path, start.line, end.line
            ),
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
    /// The findings of the AI review whose text Use or Use as is put in it: they go once it is sent.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub from_findings: Vec<Finding>,
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

/// How a review's boxes send: to GitHub, with its own actions, or as tasks for a local review's agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sending {
    GitHub,
    /// These send actions replace every GitHub one; the first is what Enter does.
    Tasks(&'static [SendMode]),
}

impl Sending {
    pub fn on_github(self) -> bool {
        matches!(self, Self::GitHub)
    }

    pub fn tasks(self) -> &'static [SendMode] {
        match self {
            Self::GitHub => &[],
            Self::Tasks(modes) => modes,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SendMode {
    /// Goes into the pending review, to be sent with the others.
    AddToReview,
    /// Posts at once, as a review of its own.
    CommentNow,
    /// A local review's task: kept as a local thread and handed to the change set's agent at once.
    SendNow,
    /// A local review's task: kept as a local thread and queued, to go to the agent with Send tasks.
    AddToTasks,
}

impl SendMode {
    /// What the comment boxes of a local review send with. Each is a task for the change set's agent.
    pub const LOCAL: [Self; 2] = [Self::AddToTasks, Self::SendNow];

    pub fn is_task(self) -> bool {
        Self::LOCAL.contains(&self)
    }
}

/// A comment GitHub did not take yet. It is sent again when GitHub can be reached.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueuedComment {
    pub draft: CommentDraft,
    pub mode: SendMode,
    #[serde(default)]
    pub audience: Audience,
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
    /// The row is a line of the head the step was read at: GitHub has no such line to comment on.
    OlderHeadLine,
    /// An Ask about a step or the whole tour has no lines a comment could sit on.
    NoLines,
    /// The Ask was answered about an older head; its lines may sit elsewhere now.
    AskIsOnOlderCode,
    /// Someone started a pending review since the last look, so "Comment now" is off.
    PendingReviewExists,
    /// A reply with no text has nothing to send.
    EmptyReply,
    /// An edit that left no text would not be a comment any more.
    EmptyComment,
    /// Only the viewer's own comments can be deleted.
    NotYourComment,
    /// A comment is deleted only after the reviewer said yes to the question.
    DeleteNotConfirmed,
    /// An Ask still waiting for its first answer has nothing to put in a comment.
    NoAnswerYet,
    NoAccess(String),
    /// The send action is not one this review offers: a task in a PR's review, or a GitHub
    /// comment in a local review.
    NotOffered,
    /// A local review's comment is a task for its agent, which cannot be reached now.
    AgentLinkOff,
}

impl CommentRefusal {
    /// Only a refusal that carries a reason has to build its text.
    pub fn message(&self) -> Cow<'static, str> {
        Cow::Borrowed(match self {
            Self::TourIsOld => "Update tour to comment",
            Self::CodeMoved => "Code moved, place the comment again",
            Self::NotInsideHunk => "GitHub takes comments only on lines of the diff",
            Self::OlderHeadLine => {
                "This line is from the code as you last read it: GitHub takes no comment on it, show the full diff to comment"
            }
            Self::NoLines => "Only an Ask about lines can become a comment",
            Self::AskIsOnOlderCode => {
                "This Ask is about older code: select the lines again to comment"
            }
            Self::PendingReviewExists => "There is a pending review now: add the comment to it",
            Self::EmptyReply => "Write the reply first",
            Self::EmptyComment => "A comment needs text",
            Self::NotYourComment => "Only your own comments can be deleted",
            Self::DeleteNotConfirmed => "Confirm the deletion first",
            Self::NoAnswerYet => "This Ask has no answer yet",
            Self::NotOffered => "This review does not send comments that way",
            Self::AgentLinkOff => {
                "The agent cannot be reached now: the comment stays here until it can"
            }
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

/// Who a comment is for. The plain send actions are for the PR's author; only Ask agent also
/// reaches the tour agent.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Audience {
    #[default]
    Author,
    Agent,
}
