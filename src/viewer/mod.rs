//! The viewer core: everything the app does, with no GPUI in it.
//! Views read its state and send it user actions; tests drive it the same way.

mod active_pr;
mod agent_event;
mod agent_pool;
mod ai_review;
mod ai_review_run;
mod app_state;
mod app_window;
mod ask;
mod ask_context;
mod ask_desk;
mod ask_target;
mod avatars;
mod branch_link;
mod chord;
mod clone_offer;
mod code_link;
mod comment_desk;
mod comments;
mod diff_display;
mod diff_fence;
mod disk_news;
mod display_options;
mod file_block;
mod file_tree;
mod finding_decisions;
mod finding_follow;
mod finding_placement;
mod finding_threads;
mod findings_list;
mod finish_review;
mod fix_requests;
mod focused_change;
mod github_outage;
mod guided_tour;
mod hand_offs;
mod highlight;
mod image_preview;
mod intent;
mod jump;
mod keymap;
mod local_row;
mod local_threads;
mod markdown;
mod merging;
mod more_lines;
mod move_offer;
mod move_to_pr;
mod new_commits;
mod no_github;
mod outdated_lines;
mod overview;
mod page;
mod piece_labels;
mod ports;
mod pr_controls;
mod pr_notes;
mod pr_origin;
mod pr_reviewers;
mod pr_status;
mod pr_switcher;
mod presence;
mod press_again;
mod problems;
mod reactions;
mod refresh;
mod related_issues;
mod relevant_prs;
mod remembered_prs;
mod rendered_view;
mod reopen_after_save;
mod replacing_jobs;
mod review;
mod review_agents;
mod review_builder;
mod review_state;
mod reviewing_mark;
mod row_selection;
mod saved_state;
mod screen_fold;
mod screen_travel;
mod settings;
mod shortcuts;
mod step_page;
mod suggestion;
mod switch_to_guided;
mod thread_actions;
mod threads;
mod toasts;
mod tour_agent;
mod tour_writing;
mod typing_save;
mod viewed_sync;
mod word_diff;
mod work_agenda;

pub use active_pr::{
    ActivePr, BuildTicket, Finished, OpenDecision, OpenOrigin, OpenOutcome, WriteDecision,
};
pub use agent_event::AgentEvent;
pub use agent_pool::{AgentPool, Opening};
pub use ai_review::AiReview;
pub use ai_review_run::{
    AiReviewAction, AiReviewRefusal, AiReviewRun, AiReviewStage, RunningReview,
};
pub use app_state::AppStateFile;
pub use app_window::{
    AfterClose, AppWindow, FailedBuild, Relaunched, ShowBuilding, ShowReview, ShowSettings,
    ShowSwitcher, ShowWriting,
};
pub use ask::{Ask, AskId, AskStore, Asks, Author, Message, Place, ShownOn};
pub use ask_desk::{
    AskDesk, AskNotice, AskProcessEvent, AskProcesses, NotDelivered, QUESTION_NOT_SENT, Question,
};
pub use ask_target::{AskTarget, Unavailable};
pub use avatars::{AvatarOutcome, AvatarPort, AvatarRequest, Avatars};
pub use branch_link::BranchOffer;
pub use chord::{Chord, Key, KeyCap, Modifier, Modifiers};
pub use clone_offer::{CloneJob, CloneOffer, CloneOfferState, RepoCloner};
pub(crate) use code_link::percent_encode_path;
pub use code_link::{
    CodeHost, CodeLink, CodeOpener, Destination, IdeCaveat, NotInIde, Opened, WorkingCopy,
};
pub use comment_desk::{BoxState, CommentBox, CommentGesture, TextBox, TourFreshness};
pub use comments::{
    CommentDraft, CommentOutcome, CommentPlace, CommentProblem, CommentRefusal, CommentRequest,
    CommentResult, QueuedComment, SendMode, Sending,
};
pub use diff_display::{
    ChangeId, DiffHunk, Edge, FilesAtHead, FilesAtHeadRequest, HiddenLines, MoreContext,
    hidden_lines_after, hidden_lines_before,
};
pub use diff_fence::{FenceLine, fence_lines};
pub use disk_news::{DiskNews, DiskOffer};
pub use display_options::DisplayOptions;
pub use file_block::FileBlock;
pub use file_tree::{FileTree, FolderPath, TreeRow, TreeRowKind};
pub use finding_decisions::{Fate, FindingDecision, FindingDecisions};
pub use finding_threads::{
    FindingThread, FindingThreads, NoQuestion, WhyAuthor, WhyDeliveryOutcome, WhyDeliveryRequest,
    WhyMessage, WhyReply, WhyState,
};
pub use findings_list::{Batch, FindingLine, FindingsList, FindingsListRow};
pub use finish_review::{
    ApproveWarning, CLAMPED_BODY_LINES, FinishAction, FinishOutcome, FinishRefusal, FinishRequest,
    FinishResult, ReviewOutcome, ReviewVerdict, SubmittedReview,
};
pub use fix_requests::{FixReply, FixRequest, FixRequests, FixState, NoFixRequest};
pub use focused_change::{ChangeInView, Focus, PageChange, ReadStep};
pub use github_outage::{Affected, Outage, OutageCause, backoff};
pub use guided_tour::{GuidedTourBanner, GuidedTourProgress, GuidedTourRefusal, GuidedTourStage};
pub use hand_offs::{
    AgentReplied, AgentReplyDone, AgentReplyRefused, AgentReplyRequest, Audience, HandOff,
    HandOffButton, HandOffId, HandOffState, HandOffs, NotAHandOffId, ReplyPlacement, Reviewed,
    SeenMark, ThreadMessage,
};
pub use highlight::{Capture, HighlightRequest, Highlights, Span, highlight_snippet};
pub use image_preview::{
    ImageCache, ImageMark, ImagePreview, ImagePreviewRequest, ImageSide, LoadedImage,
};
pub use intent::{
    Effect, FileToggle, Intent, Overlay, Refusal, Reopening, RestartArgs, ReviewChange,
    SettingsSheet, WindowMoment,
};
pub use jump::{
    JumpPlace, JumpTarget, PaletteAction, PaletteEntry, PaletteGroup, PaletteGroupKind,
};
pub use keymap::{KeyChanges, KeyProblem, Keymap, ResolvedKeymap, WrittenKeys};
pub use local_row::LocalRow;
pub use local_threads::{LocalAuthor, LocalThreadStore, LocalThreads, ThreadHome};
pub use markdown::{Block, ColumnAlignment, Markdown, Run, Table};
pub use merging::{
    AutoMerge, MergeChange, MergeMethod, MergeMethods, MergeOffer, MergeRefusal, Mergeability,
    Merging,
};
pub use more_lines::{MoreLines, MoreLinesEntry};
pub use move_offer::{MoveFrom, MoveHereOffer, MoveOffer, StartedMove};
pub use move_to_pr::{PickPullCodeSource, PrMover, let_go_of_unread_snapshots};
pub use new_commits::{
    PickCodeSource, RepinnedTour, TourReloader, TourRepinner, catch_up_findings, repin_tour,
};
pub use no_github::NoGitHub;
pub use outdated_lines::{FollowedLines, follow_outdated};
pub use overview::{OverviewFile, OverviewPage, OverviewStats};
pub use page::{
    AskAt, BinaryFileRow, BoxEdges, BoxFold, DiagramRow, FindingRow, FoldControl, FullDiffControl,
    HunkGapRow, Page, PageRow, PieceHeading, PieceLabel, PinnedHeader, ReviewerSummaryRow,
    RouteStopRow, ScreenFooter, SelectedRows, ShownLine, StepHeading, StepIntro, StepNotes,
    SummaryColumns, SummaryStatsRow, ThreadRowPlace, Toolbar, line_count, pluralised,
};
pub use piece_labels::{LabelSwitch, PieceLabels, TINY_PIECE_LINES};
pub use ports::{
    AgentLaunch, AgentLauncher, AgentPort, ChangeTipsPort, GitHubError, GitHubPort,
    IssueDetailsPort, NoAccess, PictureUploadPort, PullOfBranch, PullRequestDescriptionPort,
    PullRequestOriginPort, PullRequestReactionsPort, PullRequestStatusPort, PullsOfBranchPort,
    RefreshWriter, Resolution, ReviewThreadsPort, ReviewWritesPort, TurnStopper, Viewed,
    ViewedFile, ViewedFilesPort, ViewedState,
};
pub use pr_notes::{NoteSubject, NotesButton, ToNotes, ToNotesOutcome, with_diagram_in_notes};
pub use pr_origin::{HeadBranch, MergeRoute, PrAuthor, PullRequestOrigin};
pub use pr_reviewers::{
    PullRequestReviews, ReviewRecord, Reviewer, ReviewerKind, ReviewerLine, ReviewerStatus,
    ReviewersOnGitHub, Stance, StanceAge, Standing, Whose, YourReview,
};
pub use pr_status::{
    CHECKS_SHOWN, CheckList, CheckRun, CheckState, Checks, OpenPrStatus, PrStatus, Readiness,
};
pub use pr_switcher::{
    LineAnchor, ListLine, NotPickable, Pick, PrSwitcher, RowKind, SwitcherRow, TourFact,
    WritingFact,
};
pub use presence::{Closing, Motion, Presence, Shown, Stretch, Ticket};
pub use press_again::{CONFIRM_WITHIN, PressStep};
pub use problems::{Announcement, Problem, ProblemKind, Problems, Read, announcements};
pub use reactions::{Reaction, ReactionChange, ReactionCount};
pub use refresh::{RefreshBrief, StaleStep, TourRefresher};
pub use related_issues::{IssueDetails, IssueState, PullRequestIssues, RelatedIssue};
pub use relevant_prs::{
    RelevantPullRequest, RelevantPullRequests, RelevantPullRequestsPort, WhyListed,
};
pub use remembered_prs::RememberedPullRequestsFile;
pub use rendered_view::{BlockChange, FileView, RenderedBlock, Rendering, ViewSwitch};
pub use reopen_after_save::{
    Departure, InFlight, Reopen, ReopenAfterSave, RestartGuard, RestartStep,
};
pub use replacing_jobs::{
    AfterJob, JobOutcome, JobProgress, ReplacingJob, ReplacingJobs, StartedJob,
};
pub use review::{
    DEFAULT_WHY_QUESTION, FindingRefusal, FindingsBatch, NextScreen, OldHead, ReadProgress,
    ReadSubject, ReadUnit, ReadingPlace, Review, RouteStop, SafetyNetSize, Screen, SidebarEntry,
    StepId, SummaryStats, TourLead, UnreadFile, UnreadStep, WhyAsked,
};
pub use review_agents::ReviewAgents;
pub use review_builder::ReviewBuilder;
pub use review_state::{ReadTick, RepinRecord, ReviewState, ReviewStateFile};
pub use reviewing_mark::ReviewingMarkRemoval;
pub use row_selection::{RowSelection, SelectedLines};
pub use saved_state::RepinRunning;
pub use screen_fold::ScreenFold;
pub use screen_travel::{Arrival, ScreenTravel, Travel};
pub use settings::{CloneRow, CloneRowProblem, SaveOutcome, SettingProblem, Settings};
pub use shortcuts::{Command, KeyScope, ShortcutGroup, in_group};
pub use step_page::{
    DiffLine, FilePage, LineKind, LineLocation, PieceDiff, RowView, SafetyNetPage, Side, SinceRead,
    StepPage,
};
pub use switch_to_guided::TourSwitcher;
pub use thread_actions::{
    ThreadAction, ThreadActionOutcome, ThreadActionRequest, ThreadActionResult,
};
pub use threads::{CommentId, ReviewThread, ThreadComment, ThreadId, ThreadView};
pub use toasts::{Toast, ToastId, ToastTone, Toasts};
pub use tour_agent::{
    AgentLinkOff, AgentRole, AgentStatus, AppAgent, HerdrRefused, LiveTerminal, OwnAgentHandOff,
    ReviewAgentStart, SendOutcome, TerminalId, TourAgentPort, TourAgentStart, TourAgentState,
    TourAgents, TourKind, WriterHandOver,
};
pub use tour_writing::{TourWriting, WritingStage};
pub use typing_save::TypingSave;
pub use viewed_sync::{SyncOutcome, SyncRequest};
pub use work_agenda::{
    AGENT_LINK_RECHECK, AGENT_START_RETRY, AfterLanding, PULL_OF_BRANCH_RETRY, PUSH_DEBOUNCE,
    WORKING_TREE_RECHECK, Work, WorkKind, WorkOutcome, WorkPorts,
};
