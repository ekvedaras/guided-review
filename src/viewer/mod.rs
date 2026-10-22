//! The viewer core: everything the app does, with no GPUI in it.
//! Views read its state and send it user actions; tests drive it the same way.

mod agent_event;
mod ask;
mod ask_context;
mod diff_display;
mod github_link;
mod highlight;
mod markdown;
mod new_commits;
mod ports;
mod review;
mod review_state;
mod step_page;
mod threads;

pub use agent_event::AgentEvent;
pub use ask::{Ask, AskId, AskStore, Asks, Author, Message, Place};
pub use diff_display::{DiffHunk, Edge, FilesAtHead, FilesAtHeadRequest, MoreContext};
pub use github_link::GitHubLink;
pub use highlight::{Capture, HighlightRequest, Highlights, Span, highlight_snippet};
pub use markdown::{Block, Markdown, Run};
pub use new_commits::repin_tour;
pub use ports::{
    AgentLaunch, AgentLauncher, AgentPort, GitHubPort, ViewedFile, ViewedState,
    ViewedSyncUnavailable,
};
pub use review::{
    OldHead, ReadProgress, Review, Screen, SidebarEntry, StepId, SyncOutcome, SyncRequest,
};
pub use review_state::{ReadTick, RepinRecord, ReviewState, ReviewStateFile};
pub use step_page::{
    DiffLine, LineKind, LineLocation, PieceDiff, SafetyNetPage, Side, SinceRead, StepPage,
};
pub use threads::{ReviewThread, ThreadComment, ThreadView};
