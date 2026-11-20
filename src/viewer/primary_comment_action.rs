//! Which send action of a comment box the reviewer reaches for most: it gets the accent colour on
//! its button and the main key (Cmd+Enter), and the other gets the secondary key (Cmd+Alt+Enter).

use super::comments::{SendMode, Sending};

/// The `primary_comment_action` in `config.toml`. In a local review Add to review stands for Add
/// to tasks and Comment now for Send now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PrimaryCommentAction {
    #[default]
    AddToReview,
    CommentNow,
}

impl PrimaryCommentAction {
    pub const ALL: [Self; 2] = [Self::AddToReview, Self::CommentNow];

    pub fn config_text(self) -> &'static str {
        match self {
            Self::AddToReview => "add_to_review",
            Self::CommentNow => "comment_now",
        }
    }

    pub fn of_config_text(text: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|action| action.config_text() == text)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::AddToReview => "Add to review",
            Self::CommentNow => "Comment now",
        }
    }

    /// The other one.
    pub fn other(self) -> Self {
        match self {
            Self::AddToReview => Self::CommentNow,
            Self::CommentNow => Self::AddToReview,
        }
    }

    /// Comment now is not always offered (GitHub has a pending review). The box then has one send
    /// action, so that one is primary: its button, and the key that would have gone to the other.
    pub fn effective(self, comment_now_is_offered: bool) -> Self {
        if comment_now_is_offered {
            self
        } else {
            Self::AddToReview
        }
    }

    /// A local review's task send actions with the primary one first.
    pub fn ordered_tasks(self, sending: Sending) -> Vec<SendMode> {
        let mut tasks = sending.tasks().to_vec();
        if self == Self::CommentNow {
            tasks.reverse();
        }
        tasks
    }
}
