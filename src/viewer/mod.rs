//! The viewer core: everything the app does, with no GPUI in it.
//! Views read its state and send it user actions; tests drive it the same way.

mod active_pr;
mod agent_event;
mod agent_pool;
mod app_state;
mod app_window;
mod ask;
mod ask_context;
mod ask_target;
mod code_link;
mod comment_desk;
mod comments;
mod diff_display;
mod diff_fence;
mod display_options;
mod file_block;
mod finish_review;
mod hand_offs;
mod highlight;
mod image_preview;
mod jump;
mod markdown;
mod new_commits;
mod outdated_lines;
mod page;
mod ports;
mod pr_notes;
mod pr_switcher;
mod problems;
mod reactions;
mod refresh;
mod relevant_prs;
mod reopen_after_save;
mod review;
mod review_state;
mod row_selection;
mod saved_state;
mod screen_travel;
mod settings;
mod shortcuts;
mod step_page;
mod suggestion;
mod sync_state;
mod thread_actions;
mod threads;
mod toasts;
mod tour_agent;
mod tour_writing;
mod typing_save;
mod viewed_sync;
mod word_diff;

pub use active_pr::{ActivePr, BuildTicket, Finished, OpenDecision, OpenOrigin, WriteDecision};
pub use agent_event::AgentEvent;
pub use agent_pool::{AgentPool, Opening};
pub use app_state::AppStateFile;
pub use app_window::{AfterClose, AppWindow, FailedBuild, ShowReview, ShowSettings};
pub use ask::{Ask, AskId, AskStore, Asks, Author, Message, Place};
pub use ask_target::{AskTarget, Unavailable};
pub(crate) use code_link::percent_encode_path;
pub use code_link::{CodeLink, CodeOpener, Destination, IdeCaveat, NotInIde, Opened, WorkingCopy};
pub use comment_desk::{BoxState, CommentBox, CommentGesture, TextBox, TourFreshness};
pub use comments::{
    CommentDraft, CommentOutcome, CommentPlace, CommentProblem, CommentRefusal, CommentRequest,
    CommentResult, QueuedComment, SendMode,
};
pub use diff_display::{
    DiffHunk, Edge, FilesAtHead, FilesAtHeadRequest, HiddenLines, MoreContext, hidden_lines_after,
    hidden_lines_before,
};
pub use diff_fence::{FenceLine, fence_lines};
pub use display_options::DisplayOptions;
pub use file_block::FileBlock;
pub use finish_review::{
    ApproveWarning, CLAMPED_BODY_LINES, FinishOutcome, FinishRefusal, FinishRequest, FinishResult,
    ReviewOutcome, ReviewVerdict, SubmittedReview,
};
pub use hand_offs::{
    AgentReplied, AgentReplyDone, AgentReplyRefused, AgentReplyRequest, Audience, DeliveryOutcome,
    DeliveryRequest, HandOff, HandOffId, HandOffState, HandOffs, NotAHandOffId, ReplyPlacement,
    SeenMark, SeenMarkOff, SeenMarkOffOutcome, ThreadMessage,
};
pub use highlight::{Capture, HighlightRequest, Highlights, Span, highlight_snippet};
pub use image_preview::{
    ImageCache, ImageMark, ImagePreview, ImagePreviewRequest, ImageSide, LoadedImage,
};
pub use jump::{
    JumpPlace, JumpTarget, PaletteAction, PaletteEntry, PaletteGroup, PaletteGroupKind,
};
pub use markdown::{Block, Markdown, Run};
pub use new_commits::{PickCodeSource, RepinnedTour, TourReloader, TourRepinner, repin_tour};
pub use outdated_lines::{FollowedLines, follow_outdated};
pub use page::Page;
pub use ports::{
    AgentLaunch, AgentLauncher, AgentPort, GitHubError, GitHubPort, NoAccess, PictureUploadPort,
    PullRequestDescriptionPort, PullRequestTipsPort, RefreshWriter, Resolution, ReviewThreadsPort,
    ReviewWritesPort, TurnStopper, Viewed, ViewedFile, ViewedFilesPort, ViewedState,
};
pub use pr_notes::{NoteSubject, NotesButton, ToNotes, ToNotesOutcome, with_diagram_in_notes};
pub use pr_switcher::{NotPickable, Pick, PrSwitcher, SwitcherRow, TourFact};
pub use problems::{Problem, ProblemKind, Problems, Read};
pub use reactions::{Reaction, ReactionChange, ReactionCount};
pub use refresh::{RefreshBrief, StaleStep, TourRefresher};
pub use relevant_prs::{
    RelevantPullRequest, RelevantPullRequests, RelevantPullRequestsPort, WhyListed,
};
pub use reopen_after_save::{InFlight, Reopen, ReopenAfterSave, RestartGuard, RestartStep};
pub use review::{
    NextScreen, OldHead, ReadProgress, ReadingPlace, Review, RouteStop, SafetyNetSize, Screen,
    SidebarEntry, StepId, SummaryStats, TourLead, UnreadStep,
};
pub use review_state::{ReadTick, RepinRecord, ReviewState, ReviewStateFile};
pub use row_selection::{RowSelection, SelectedLines};
pub use saved_state::RepinRunning;
pub use screen_travel::{Arrival, ScreenTravel, Travel};
pub use settings::{CloneRow, CloneRowProblem, SaveOutcome, SettingProblem, Settings};
pub use shortcuts::{Chord, Command, KeyCap, KeyScope, ShortcutGroup, in_group, keystroke};
pub use step_page::{
    DiffLine, LineKind, LineLocation, PieceDiff, RowView, SafetyNetPage, Side, SinceRead, StepPage,
};
pub use sync_state::SyncState;
pub use thread_actions::{
    ThreadAction, ThreadActionOutcome, ThreadActionRequest, ThreadActionResult,
};
pub use threads::{CommentId, ReviewThread, ThreadComment, ThreadId, ThreadView};
pub use toasts::{Toast, ToastId, ToastTone, Toasts};
pub use tour_agent::{
    AgentStatus, AppAgent, LiveTerminal, OwnAgentHandOff, SendOutcome, TerminalId, TourAgentPort,
    TourAgentStart, TourAgentState, TourAgents,
};
pub use tour_writing::{TourWriting, WritingStage};
pub use typing_save::TypingSave;
pub use viewed_sync::{SyncOutcome, SyncRequest};
