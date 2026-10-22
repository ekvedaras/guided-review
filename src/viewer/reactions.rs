use serde::{Deserialize, Serialize};

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
    pub const ALL: [Reaction; 8] = [
        Self::ThumbsUp,
        Self::ThumbsDown,
        Self::Laugh,
        Self::Hooray,
        Self::Confused,
        Self::Heart,
        Self::Rocket,
        Self::Eyes,
    ];

    /// The `ReactionContent` value GitHub's GraphQL API uses.
    pub fn github_name(self) -> &'static str {
        match self {
            Self::ThumbsUp => "THUMBS_UP",
            Self::ThumbsDown => "THUMBS_DOWN",
            Self::Laugh => "LAUGH",
            Self::Hooray => "HOORAY",
            Self::Confused => "CONFUSED",
            Self::Heart => "HEART",
            Self::Rocket => "ROCKET",
            Self::Eyes => "EYES",
        }
    }

    pub fn from_github_name(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|reaction| reaction.github_name() == name)
    }

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
