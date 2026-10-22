//! The viewer core: everything the app does, with no GPUI in it.
//! Views read its state and send it user actions; tests drive it the same way.

mod agent_event;
mod ask;
mod ask_context;
mod comments;
mod diff_display;
mod github_link;
mod highlight;
mod jump;
mod markdown;
mod new_commits;
mod ports;
mod review;
mod review_state;
mod row_selection;
mod step_page;
mod threads;

pub use agent_event::AgentEvent;
pub use ask::{Ask, AskId, AskStore, Asks, Author, Message, Place};
pub use comments::{
    CommentDraft, CommentOutcome, CommentPlace, CommentRefusal, CommentRequest, CommentResult,
    QueuedComment, SendMode,
};
pub use diff_display::{DiffHunk, Edge, FilesAtHead, FilesAtHeadRequest, MoreContext};
pub use github_link::GitHubLink;
pub use highlight::{Capture, HighlightRequest, Highlights, Span, highlight_snippet};
pub use jump::{JumpKind, JumpTarget};
pub use markdown::{Block, Markdown, Run};
pub use new_commits::{PickCodeSource, TourUpdater, UpdatedTour, repin_tour};
pub use ports::{
    AgentLaunch, AgentLauncher, AgentPort, GitHubPort, PendingReviewExists, ThreadNotCreated,
    ViewedFile, ViewedState, ViewedSyncUnavailable,
};
pub use review::{
    OldHead, ReadProgress, RepinRunning, Review, Screen, SidebarEntry, StepId, SyncOutcome,
    SyncRequest,
};
pub use review_state::{ReadTick, RepinRecord, ReviewState, ReviewStateFile};
pub use row_selection::{RowSelection, SelectedLines};
pub use step_page::{
    DiffLine, LineKind, LineLocation, PieceDiff, SafetyNetPage, Side, SinceRead, StepPage,
};
pub use threads::{ReviewThread, ThreadComment, ThreadView};
