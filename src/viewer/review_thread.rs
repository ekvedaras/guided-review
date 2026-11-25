//! A review thread as GitHub has it: its comments, place and state.

use std::fmt;
use std::sync::Arc;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::file_path::FilePath;
use crate::line_number::LineNumber;

use super::line_location::Side;
use super::reactions::{Reaction, ReactionCount};

/// GitHub's node id of a review thread.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ThreadId(Arc<str>);

/// GitHub's node id of one comment in a review thread, which reactions are added to.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CommentId(Arc<str>);

macro_rules! host_id {
    ($id:ident) => {
        impl $id {
            pub fn new(id: &str) -> Self {
                Self(id.into())
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $id {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl Serialize for $id {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(&self.0)
            }
        }

        impl<'de> Deserialize<'de> for $id {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                Ok(Self::new(&String::deserialize(deserializer)?))
            }
        }
    };
}

host_id!(ThreadId);
host_id!(CommentId);

/// A review thread as GitHub holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewThread {
    pub id: ThreadId,
    pub is_resolved: bool,
    pub is_outdated: bool,
    pub path: FilePath,
    /// The last line the thread covers at the PR's head. Unknown for outdated threads and file comments.
    pub line: Option<LineNumber>,
    pub start_line: Option<LineNumber>,
    pub side: Side,
    /// A comment on the whole file, not on a line.
    pub is_file_comment: bool,
    pub original_line: Option<LineNumber>,
    pub original_start_line: Option<LineNumber>,
    pub original_commit: Option<String>,
    pub comments: Vec<ThreadComment>,
}

/// Marks what a tour agent wrote on GitHub, where it shares the reviewer's account.
pub const AGENT_MARK: &str = "🤖";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadComment {
    pub id: CommentId,
    pub author: String,
    /// GitHub's address of the author's picture; `None` for a deleted account.
    pub avatar_url: Option<String>,
    pub is_bot: bool,
    /// Whether the change set's agent wrote it, as a local thread knows it of its own authors; `None` on
    /// GitHub, where an agent's comment is told by the robot in front of it instead.
    pub by_agent: Option<bool>,
    /// The viewer wrote it, so the viewer may edit it.
    pub by_viewer: bool,
    pub body: String,
    /// The comment on the PR conversation page.
    pub url: String,
    /// Only the reactions someone made; `reaction_summary` fills in the rest.
    pub reactions: Vec<ReactionCount>,
}

impl ThreadComment {
    /// Written by the author's tour agent, which posts under their account with the robot in front.
    pub fn is_by_agent(&self) -> bool {
        self.by_agent
            .unwrap_or_else(|| self.body.starts_with(AGENT_MARK))
    }

    /// `<login>'s agent` for an agent's comment on GitHub; a local thread names its authors itself.
    pub fn shown_author(&self) -> String {
        if self.by_agent.is_none() && self.is_by_agent() {
            format!("{}'s agent", self.author)
        } else {
            self.author.clone()
        }
    }

    /// An agent's comment without the robot, which the page shows in its own way.
    pub fn shown_body(&self) -> &str {
        if self.by_agent.is_some() {
            return &self.body;
        }
        self.body
            .strip_prefix(AGENT_MARK)
            .map_or(self.body.as_str(), str::trim_start)
    }

    /// All eight reactions in GitHub's order, with zero for the ones nobody used.
    pub fn reaction_summary(&self) -> Vec<ReactionCount> {
        Reaction::ALL
            .into_iter()
            .map(|reaction| {
                self.reactions
                    .iter()
                    .find(|count| count.reaction == reaction)
                    .copied()
                    .unwrap_or(ReactionCount {
                        reaction,
                        count: 0,
                        viewer_has_reacted: false,
                    })
            })
            .collect()
    }
}

impl ReviewThread {
    pub(super) fn is_folded_by_default(&self) -> bool {
        self.is_resolved || self.comments.first().is_some_and(|comment| comment.is_bot)
    }
}
