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
mod capability;
mod carried;
mod clone_offer;
mod code_link;
mod comment_desk;
mod comment_send;
mod comments;
mod comments_list;
mod diff_display;
mod diff_fence;
mod diff_line;
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
mod guided_tour;
mod hand_offs;
mod hidden_text;
mod highlight;
mod host_outage;
mod image_preview;
mod intent;
mod jump;
mod kept_values;
mod line_location;
mod local_row;
mod local_thread_store;
mod local_threads;
mod markdown;
mod merging;
mod move_offer;
mod move_to_pr;
mod name_card;
mod name_desk;
mod name_highlights;
mod name_hover;
mod new_commits;
mod no_host;
mod outdated_lines;
mod overview;
mod page;
mod piece_diff;
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
mod refresh_brief;
mod related_issues;
mod relevant_prs;
mod remembered_prs;
mod rendered_view;
mod reopen_after_save;
mod replacing_jobs;
mod review;
mod review_agents;
mod review_builder;
mod review_place;
mod review_state;
mod review_thread;
mod reviewing_mark;
mod row_selection;
mod saved_state;
mod screen;
mod screen_fold;
mod screen_travel;
mod settings;
mod settings_page;
mod stations;
mod step_page;
mod suggestion;
mod switch_to_guided;
mod thread_actions;
mod threads;
mod toasts;
mod tour_agent;
mod tour_writing;
mod typing_save;
mod used_findings;
mod viewed_sync;
mod word_diff;
mod work_agenda;
mod written_on;

pub use crate::keys::{Chord, Key, KeyCap, Modifier, Modifiers};
pub use crate::keys::{Command, KeyScope, ShortcutGroup, in_group};
pub use crate::keys::{KeyChanges, KeyProblem, Keymap, PlannedSend, ResolvedKeymap, WrittenKeys};
pub use crate::keys::{SendAction, SendChoice, SendRole};
pub use crate::merge_method::MergeMethod;
pub use crate::more_lines::{MoreLines, MoreLinesEntry};
pub use active_pr::WritingFact;
pub use active_pr::{ActivePr, BuildTicket, Finished, OpenDecision, OpenOutcome, WriteDecision};
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
pub use branch_link::{BranchOffer, local_reviews_of_repo};
pub use capability::{Capability, HostCapabilities};
pub use clone_offer::{CloneJob, CloneOffer, CloneOfferState};
pub use code_link::{
    CodeLink, CodeOnWeb, CodeOpener, Destination, IdeCaveat, NotInIde, Opened, WorkingCopy,
};
pub use comment_desk::{BoxState, CommentBox, CommentGesture, TextBox, land_elsewhere};
pub use comment_send::{CommentOutcome, CommentRequest};
pub use comments::Audience;
pub use comments::{
    CommentDraft, CommentPlace, CommentProblem, CommentRefusal, CommentResult, QueuedComment,
    SendMode, Sending,
};
pub use comments_list::{CommentsList, CommentsListRow, CommentsSection, ScreenGroup, ThreadVisit};
pub use diff_display::{
    ChangeId, DiffHunk, Edge, FilesAtHead, FilesAtHeadRequest, HiddenLines, MoreContext,
    hidden_lines_after, hidden_lines_before,
};
pub use diff_fence::{FenceLine, fence_lines};
pub use diff_line::{DiffLine, LineKind, RowView};
pub use disk_news::{DiskNews, DiskOffer};
pub use display_options::DisplayOptions;
pub use display_options::Focus;
pub use file_block::FileBlock;
pub use file_tree::{FileTree, FolderPath, TreeRow, TreeRowKind};
pub use finding_decisions::{Fate, FindingDecision, FindingDecisions};
pub use finding_threads::{
    FindingThread, FindingThreads, NoQuestion, WhyAuthor, WhyDeliveryOutcome, WhyDeliveryRequest,
    WhyMessage, WhyReply, WhyState,
};
pub use findings_list::{Batch, FindingLine, FindingsList, FindingsListRow};
pub use finish_review::{
    ApproveWarning, FinishAction, FinishOutcome, FinishRefusal, FinishRequest, FinishResult,
    ReviewVerdict,
};
pub use fix_requests::{
    FixNote, FixReply, FixRequest, FixRequests, FixState, FixTiming, NoFixRequest,
};
pub use focused_change::{ChangeInView, PageChange, ReadStep};
pub use guided_tour::{GuidedTourBanner, GuidedTourProgress, GuidedTourRefusal, GuidedTourStage};
pub use hand_offs::{
    AgentReplied, AgentReplyDone, AgentReplyRefused, AgentReplyRequest, HandOff, HandOffButton,
    HandOffId, HandOffState, HandOffs, HandToAgentRefused, NotAHandOffId, ReplyPlacement, Reviewed,
    SeenMark, ThreadMessage,
};
pub use hidden_text::{HiddenPart, HiddenText};
pub use highlight::{
    Capture, HighlightRequest, Highlights, Span, highlight_file_lines, highlight_snippet,
};
pub use host_outage::Read;
pub use host_outage::{Affected, Outage, OutageCause, backoff};
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
pub use line_location::{LineLocation, Side};
pub use local_row::LocalRow;
pub use local_thread_store::{LocalThreadStore, ThreadHome};
pub use local_threads::{LocalAuthor, LocalThreads};
pub use markdown::{Block, ColumnAlignment, Markdown, Run, Table};
pub use merging::{
    AutoMerge, MergeChange, MergeMethods, MergeOffer, MergeRefusal, Mergeability, Merging,
};
pub use move_offer::{MoveFrom, MoveHereOffer, MoveOffer, StartedMove};
pub use move_to_pr::{DropLocalReview, PickPullCodeSource, PrMover, let_go_of_unread_snapshots};
pub use name_card::{
    CardNote, CodeRun, DefinitionCard, DefinitionRow, FoundNames, NameTarget, ServerQuestion,
    Snippet, UsageFile, UsageLine,
};
pub use name_desk::{HoveredName, NameOutcome, NameUnderPointer, NameWaiting, NameWork};
pub use name_highlights::NameHighlights;
pub use name_highlights::USAGES_SHOWN;
pub use name_hover::{HoverTicket, NameHover};
pub use new_commits::{PickCodeSource, catch_up_findings, repin_tour};
pub use no_host::NoHost;
pub use outdated_lines::{FollowedLines, follow_outdated};
pub use overview::{OverviewFile, OverviewPage, OverviewStats};
pub use page::{
    AskAt, BinaryFileRow, BoxEdges, BoxFold, DiagramRow, FindingRow, FixesOffer, FoldControl,
    FullDiffControl, HunkGapRow, Page, PageRow, PieceHeading, PieceLabel, PinnedHeader,
    ReviewerSummaryRow, RouteStopRow, ScreenFooter, SelectedRows, ShownLine, StepHeading,
    StepIntro, StepNotes, SummaryColumns, SummaryStatsRow, ThreadRowPlace, Toolbar, line_count,
    pluralised,
};
pub use piece_diff::PieceDiff;
pub use piece_labels::{LabelSwitch, PieceLabels, TINY_PIECE_LINES};
pub use ports::{
    AgentLaunch, AgentLauncher, AgentPort, ChangeRequestsPort, ChangeTipsPort, ClonePort, CodeHost,
    CommentReactionsPort, HostError, HostedCodePort, IssueDetailsPort, MergingPort, NoAccess,
    PictureUploadPort, PullOfBranch, PullRequestDescriptionPort, PullRequestOriginPort,
    PullRequestReactionsPort, PullRequestStatusPort, PullsOfBranchPort, RefreshWriter,
    RelevantPullRequestsPort, Resolution, ReviewThreadsPort, ReviewWritesPort, TurnStopper, Viewed,
    ViewedFile, ViewedFilesPort, ViewedState,
};
pub use pr_notes::{NoteSubject, NotesButton, ToNotes, ToNotesOutcome, with_diagram_in_notes};
pub use pr_origin::{HeadBranch, MergeRoute, PrAuthor, PullRequestOrigin};
pub use pr_reviewers::{
    CLAMPED_BODY_LINES, PullRequestReviews, ReviewOutcome, ReviewRecord, Reviewer, ReviewerKind,
    ReviewerLine, ReviewerStatus, ReviewersOnHost, Stance, StanceAge, Standing, SubmittedReview,
    Whose, YourReview,
};
pub use pr_status::{
    CHECKS_SHOWN, CheckList, CheckRun, CheckState, Checks, OpenPrStatus, PrStatus, Readiness,
};
pub use pr_switcher::{
    LineAnchor, ListLine, NotPickable, Pick, PrSwitcher, RowKind, SwitcherRow, TourFact,
};
pub use presence::{Closing, Motion, Presence, Shown, Stretch, Ticket};
pub use press_again::{CONFIRM_WITHIN, PressStep};
pub use problems::{Announcement, Problem, ProblemKind, Problems, announcements};
pub use reactions::{Reaction, ReactionChange, ReactionCount};
pub use refresh::TourRefresher;
pub use refresh_brief::{RefreshBrief, StaleStep};
pub use related_issues::{IssueDetails, IssueState, PullRequestIssues, RelatedIssue};
pub use relevant_prs::{RelevantPullRequest, RelevantPullRequests, WhyListed};
pub use remembered_prs::RememberedPullRequestsFile;
pub use rendered_view::{BlockChange, FileView, RenderedBlock, Rendering, ViewSwitch};
pub use reopen_after_save::{
    Departure, InFlight, Reopen, ReopenAfterSave, RestartGuard, RestartStep,
};
pub use replacing_jobs::RepinnedTour;
pub use replacing_jobs::{
    AfterJob, JobOutcome, JobProgress, ReplacingJob, ReplacingJobs, StartedJob,
};
pub use review::{
    DEFAULT_WHY_QUESTION, FindingRefusal, FindingsBatch, OldHead, ReadProgress, ReadUnit,
    ReadingPlace, Review, RouteStop, SafetyNetSize, SidebarEntry, StepId, SummaryStats, TourLead,
    UnreadFile, UnreadStep, WhyAsked,
};
pub use review_agents::ReviewAgents;
pub use review_builder::ReviewBuilder;
pub use review_builder::TourReloader;
pub use review_place::{CheckoutPort, ReviewPlace};
pub use review_state::{ReadTick, RepinRecord, ReviewState, ReviewStateFile};
pub use review_thread::{CommentId, ReviewThread, ThreadComment, ThreadId};
pub use reviewing_mark::ReviewingMarkRemoval;
pub use row_selection::{RowSelection, SelectedLines};
pub use saved_state::RepinRunning;
pub use screen::{NextScreen, ReadSubject, Screen};
pub use screen_fold::ScreenFold;
pub use screen_travel::{Arrival, ScreenTravel, Travel};
pub use settings::{
    CloneRow, CloneRowProblem, LanguageServersRow, SaveOutcome, SettingProblem, Settings,
};
pub use settings_page::SettingsPage;
pub use stations::{Station, StationHealth, StationKind, Stations, waited};
pub use step_page::{FilePage, SafetyNetPage, SinceRead, StepPage};
pub use switch_to_guided::TourSwitcher;
pub use thread_actions::TourFreshness;
pub use thread_actions::{
    ThreadAction, ThreadActionOutcome, ThreadActionRequest, ThreadActionResult,
};
pub use threads::ThreadView;
pub use toasts::{Toast, ToastId, ToastTone, Toasts};
pub use tour_agent::OpenOrigin;
pub use tour_agent::{
    AgentLinkOff, AgentRole, AgentStatus, AppAgent, HerdrRefused, LiveTerminal, OwnAgentHandOff,
    ReviewAgentStart, SendOutcome, TerminalId, TourAgentPort, TourAgentStart, TourAgentState,
    TourAgents, TourKind, WriterHandOver,
};
pub use tour_writing::{TourWriting, WritingPhase, WritingStage};
pub use typing_save::TypingSave;
pub use used_findings::UsedFinding;
pub use viewed_sync::{SyncOutcome, SyncRequest};
pub use work_agenda::{
    AGENT_LINK_RECHECK, AGENT_START_RETRY, AfterLanding, PULL_OF_BRANCH_RETRY, PUSH_DEBOUNCE,
    WORKING_TREE_RECHECK, Work, WorkKind, WorkOutcome, WorkPorts,
};
