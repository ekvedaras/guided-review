//! The viewer core: everything the app does, with no GPUI in it.
//! Views read its state and send it user actions; tests drive it the same way.

mod active_pr;
mod agent_event;
mod agent_pool;
mod ask;
mod ask_context;
mod ask_target;
mod code_link;
mod comment_desk;
mod comments;
mod diff_display;
mod display_options;
mod file_block;
mod finish_review;
mod highlight;
mod image_preview;
mod jump;
mod markdown;
mod new_commits;
mod ports;
mod problems;
mod reactions;
mod refresh;
mod review;
mod review_state;
mod row_selection;
mod step_page;
mod suggestion;
mod sync_state;
mod thread_actions;
mod threads;
mod tour_agent;
mod typing_save;
mod viewed_sync;

pub use active_pr::{ActivePr, BuildTicket, Finished, OpenDecision};
pub use agent_event::AgentEvent;
pub use agent_pool::{AgentPool, Opening};
pub use ask::{Ask, AskId, AskStore, Asks, Author, Message, Place};
pub use ask_target::{AskTarget, Unavailable};
pub(crate) use code_link::percent_encode_path;
pub use code_link::{CodeLink, CodeOpener, Destination, IdeCaveat, NotInIde, Opened, WorkingCopy};
pub use comment_desk::TourFreshness;
pub use comments::{
    CommentDraft, CommentOutcome, CommentPlace, CommentProblem, CommentRefusal, CommentRequest,
    CommentResult, QueuedComment, SendMode,
};
pub use diff_display::{DiffHunk, Edge, FilesAtHead, FilesAtHeadRequest, MoreContext};
pub use file_block::FileBlock;
pub use finish_review::{
    ApproveWarning, CLAMPED_BODY_LINES, FinishOutcome, FinishRefusal, FinishRequest, FinishResult,
    ReviewOutcome, ReviewVerdict, SubmittedReview,
};
pub use highlight::{Capture, HighlightRequest, Highlights, Span, highlight_snippet};
pub use image_preview::{
    ImageCache, ImageMark, ImagePreview, ImagePreviewRequest, ImageSide, LoadedImage,
};
pub use jump::{JumpPlace, JumpTarget};
pub use markdown::{Block, Markdown, Run};
pub use new_commits::{PickCodeSource, RepinnedTour, TourReloader, TourRepinner, repin_tour};
pub use ports::{
    AgentLaunch, AgentLauncher, AgentPort, GitHubError, GitHubPort, NoAccess,
    PullRequestDescriptionPort, PullRequestTipsPort, RefreshWriter, Resolution, ReviewThreadsPort,
    ReviewWritesPort, TurnStopper, Viewed, ViewedFile, ViewedFilesPort, ViewedState,
};
pub use problems::{Problem, Read};
pub use reactions::{Reaction, ReactionChange, ReactionCount};
pub use refresh::{RefreshBrief, StaleStep, TourRefresher};
pub use review::{
    NextScreen, OldHead, ReadProgress, ReadingPlace, RepinRunning, Review, SafetyNetSize, Screen,
    SidebarEntry, StepId,
};
pub use review_state::{ReadTick, RepinRecord, ReviewState, ReviewStateFile};
pub use row_selection::{RowSelection, SelectedLines};
pub use step_page::{
    DiffLine, LineKind, LineLocation, PieceDiff, SafetyNetPage, Side, SinceRead, StepPage,
};
pub use sync_state::SyncState;
pub use thread_actions::{
    ThreadAction, ThreadActionOutcome, ThreadActionRequest, ThreadActionResult,
};
pub use threads::{CommentId, ReviewThread, ThreadComment, ThreadId, ThreadView};
pub use tour_agent::{AgentStatus, TerminalId, TourAgentPort, TourAgentState, TourAgents};
pub use typing_save::TypingSave;
pub use viewed_sync::{SyncOutcome, SyncRequest};
