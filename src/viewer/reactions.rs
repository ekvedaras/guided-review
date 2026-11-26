use serde::{Deserialize, Serialize};

/// Adds the viewer's reaction to a comment, or takes it back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReactionChange {
    Add,
    Remove,
}

/// The eight reactions GitHub offers on a comment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Reaction {
    ThumbsUp,
    ThumbsDown,
    Laugh,
    Hooray,
    Confused,
    Heart,
    Rocket,
    Eyes,
}

impl Reaction {
    pub const ALL: [Self; 8] = [
        Self::ThumbsUp,
        Self::ThumbsDown,
        Self::Laugh,
        Self::Hooray,
        Self::Confused,
        Self::Heart,
        Self::Rocket,
        Self::Eyes,
    ];

    pub fn emoji(self) -> &'static str {
        match self {
            Self::ThumbsUp => "👍",
            Self::ThumbsDown => "👎",
            Self::Laugh => "😄",
            Self::Hooray => "🎉",
            Self::Confused => "😕",
            Self::Heart => "❤️",
            Self::Rocket => "🚀",
            Self::Eyes => "👀",
        }
    }
}

/// How many people reacted one way, and whether the viewer is one of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReactionCount {
    pub reaction: Reaction,
    pub count: usize,
    pub viewer_has_reacted: bool,
}
