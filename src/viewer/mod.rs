//! The viewer core: everything the app does, with no GPUI in it.
//! Views read its state and send it user actions; tests drive it the same way.

mod agent_event;
mod agent_pool;
mod ask;
mod ask_context;
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
mod thread_actions;
mod threads;
mod typing_save;
mod viewed_sync;

pub use agent_event::AgentEvent;
pub use agent_pool::{AgentPool, Opening};
pub use ask::{Ask, AskId, AskStore, Asks, Author, Message, Place};
pub(crate) use code_link::percent_encode_path;
pub use code_link::{CodeLink, CodeOpener, Destination, IdeCaveat, NotInIde};
pub use comments::{
    CommentDraft, CommentOutcome, CommentPlace, CommentProblem, CommentRefusal, CommentRequest,
    CommentResult, QueuedComment, SendMode,
};
pub use diff_display::{DiffHunk, Edge, FilesAtHead, FilesAtHeadRequest, MoreContext};
pub use file_block::FileBlock;
pub use finish_review::{
    ApproveWarning, FinishOutcome, FinishRefusal, FinishRequest, FinishResult, ReviewVerdict,
};
pub use highlight::{Capture, HighlightRequest, Highlights, Span, highlight_snippet};
pub use image_preview::{ImageCache, ImagePreview, ImagePreviewRequest, ImageSide, LoadedImage};
pub use jump::{JumpPlace, JumpTarget};
pub use markdown::{Block, Markdown, Run};
pub use new_commits::{PickCodeSource, RepinnedTour, TourRepinner, repin_tour};
pub use ports::{
    AgentLaunch, AgentLauncher, AgentPort, GitHubPort, PendingReviewExists,
    PullRequestDescriptionPort, PullRequestTipsPort, RefreshWriter, ReviewThreadsPort,
    ReviewWritesPort, ThreadNotCreated, TurnStopper, ViewedFile, ViewedFilesPort, ViewedState,
    ViewedSyncUnavailable,
};
pub use problems::Problem;
pub use reactions::{Reaction, ReactionCount};
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
pub use suggestion::SuggestionDiff;
pub use thread_actions::{
    ThreadAction, ThreadActionOutcome, ThreadActionRequest, ThreadActionResult,
};
pub use threads::{ReviewThread, ThreadComment, ThreadView};
pub use typing_save::TypingSave;
pub use viewed_sync::{SyncOutcome, SyncRequest};
