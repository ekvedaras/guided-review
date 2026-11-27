/// A way a comment or reply box can send what was written. Each box offers some of them, under
/// its own labels (a reply says "Reply now", a local review "Send now").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SendAction {
    AddToReview,
    CommentNow,
    SuggestChange,
    CommentAndAskAgent,
    CommentAndAskAgentNow,
    AskAgent,
}

impl SendAction {
    /// In the order a box lists the actions that are neither primary nor secondary.
    pub const ALL: [Self; 6] = [
        Self::AddToReview,
        Self::CommentNow,
        Self::SuggestChange,
        Self::CommentAndAskAgent,
        Self::CommentAndAskAgentNow,
        Self::AskAgent,
    ];

    pub fn config_text(self) -> &'static str {
        match self {
            Self::AddToReview => "add_to_review",
            Self::CommentNow => "comment_now",
            Self::SuggestChange => "suggest_change",
            Self::CommentAndAskAgent => "comment_and_ask_agent",
            Self::CommentAndAskAgentNow => "comment_and_ask_agent_now",
            Self::AskAgent => "ask_agent_only",
        }
    }

    /// The text the action was written under before it was renamed, still read: `ask_agent` and
    /// `ask_agent_now` meant what Comment and ask agent do now.
    fn former_config_text(self) -> Option<&'static str> {
        match self {
            Self::CommentAndAskAgent => Some("ask_agent"),
            Self::CommentAndAskAgentNow => Some("ask_agent_now"),
            Self::AddToReview | Self::CommentNow | Self::SuggestChange | Self::AskAgent => None,
        }
    }

    pub fn of_config_text(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|action| {
            action.config_text() == text || action.former_config_text() == Some(text)
        })
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::AddToReview => "Add to review",
            Self::CommentNow => "Comment now",
            Self::SuggestChange => "Suggest change",
            Self::CommentAndAskAgent => "Comment and ask agent",
            Self::CommentAndAskAgentNow => "Comment and ask agent now",
            Self::AskAgent => "Ask agent",
        }
    }

    /// What stands in for the action in a box that does not offer it, nearest first. Add to review
    /// is in every box, so it ends every list. A private Ask agent is never stood in for by an action that
    /// posts.
    fn stand_ins(self) -> &'static [Self] {
        match self {
            Self::AddToReview => &[],
            Self::CommentNow | Self::SuggestChange | Self::CommentAndAskAgent | Self::AskAgent => {
                &[Self::AddToReview]
            }
            Self::CommentAndAskAgentNow => &[
                Self::CommentAndAskAgent,
                Self::CommentNow,
                Self::AddToReview,
            ],
        }
    }

    /// The action itself where the box offers it, else its nearest stand-in the box offers.
    pub fn resolve(self, offered: &[Self]) -> Self {
        std::iter::once(self)
            .chain(self.stand_ins().iter().copied())
            .find(|action| offered.contains(action))
            .unwrap_or(Self::AddToReview)
    }
}

/// Whether a send action holds one of the two main keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendRole {
    /// Has the accent colour, comes first and has Cmd+Enter.
    Primary,
    /// Comes second and has Cmd+Shift+Enter.
    Secondary,
    Other,
}

/// The reviewer's two main send actions. They are never the same one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SendChoice {
    primary: SendAction,
    secondary: SendAction,
}

impl Default for SendChoice {
    fn default() -> Self {
        Self {
            primary: SendAction::AddToReview,
            secondary: SendAction::CommentAndAskAgent,
        }
    }
}

impl SendChoice {
    /// When both are the same action the secondary becomes the default one, or Add to review where
    /// that is the primary.
    pub fn new(primary: SendAction, secondary: SendAction) -> Self {
        let default = Self::default();
        let secondary = if secondary != primary {
            secondary
        } else if default.secondary != primary {
            default.secondary
        } else {
            default.primary
        };
        Self { primary, secondary }
    }

    /// Texts that name no action read as the default, so a typo cannot stop the app opening.
    pub fn of_config_text(primary: Option<&str>, secondary: Option<&str>) -> Self {
        let default = Self::default();
        let read = |text: Option<&str>, otherwise| {
            text.and_then(SendAction::of_config_text)
                .unwrap_or(otherwise)
        };
        Self::new(
            read(primary, default.primary),
            read(secondary, default.secondary),
        )
    }

    pub fn primary(self) -> SendAction {
        self.primary
    }

    pub fn secondary(self) -> SendAction {
        self.secondary
    }

    /// Picking the secondary's action as primary swaps the two.
    pub fn with_primary(self, action: SendAction) -> Self {
        if action == self.secondary {
            Self {
                primary: action,
                secondary: self.primary,
            }
        } else {
            Self::new(action, self.secondary)
        }
    }

    /// Picking the primary's action as secondary swaps the two.
    pub fn with_secondary(self, action: SendAction) -> Self {
        if action == self.primary {
            Self {
                primary: self.secondary,
                secondary: action,
            }
        } else {
            Self {
                primary: self.primary,
                secondary: action,
            }
        }
    }

    pub fn role_of(self, action: SendAction) -> SendRole {
        if action == self.primary {
            SendRole::Primary
        } else if action == self.secondary {
            SendRole::Secondary
        } else {
            SendRole::Other
        }
    }
}
