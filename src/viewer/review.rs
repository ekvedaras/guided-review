mod guided_tour;
mod jobs;
mod kept;
mod pr_controls;
mod work;

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;
use std::sync::Arc;
use std::time::Instant;

use crate::code_source::CodeSource;
use crate::coverage::{self, PiecePart};
use crate::issue::IssueLink;
use crate::local_facts::{LocalChange, LocalFacts};
use crate::local_work_tree::OnDisk;
use crate::pieces::{self, ChangedFile, PieceId};
use crate::pull_request::Sha;
use crate::review_subject::ReviewSubject;
use crate::tour::{BasicTour, GuidedTour, Step, Tour};

use std::path::{Path, PathBuf};

use crate::config::AskSettings;

use super::ask::{Ask, Asks, Place, ShownOn};
use super::ask_context;
use super::ask_desk::{AskDesk, AskProcesses};
use super::ask_target::{AskTarget, Unavailable};
use super::branch_link::{BranchLink, BranchOffer, LocalReviewsOfRepo};
use super::code_link::{CodeHost, CodeLinks, CodeOpener, WorkingCopy};
use super::comment_desk::{CommentBox, CommentDesk, CommentGesture, TextBox, TourFreshness};
use super::comments::{
    CommentOutcome, CommentPlace, CommentProblem, CommentRefusal, CommentRequest, CommentResult,
    QueuedComment, SendMode, Sending,
};
use super::diff_display::{DiffDisplay, FilesAtHead, FilesAtHeadRequest};
use super::disk_news::DiskNews;
use super::display_options::DisplayOptions;
use super::file_block::FileBlock;
use super::file_tree::{FileTree, FolderPath, TreeRow};
use super::finish_review::{
    ApproveWarning, FinishAction, FinishOutcome, FinishRefusal, FinishRequest, FinishResult,
    ReviewVerdict, SubmittedReview,
};
use super::github_outage::{Affected, OutageCause};
use super::guided_tour::GuidedTourProgress;
use super::hand_offs::{
    AgentReplied, Audience, HandOff, HandOffId, HandOffState, HandOffs, SentFrom,
};
use super::highlight::{HighlightRequest, Highlights};
use super::image_preview::ImagePreviewRequest;
use super::jump::{self, JumpTarget, PaletteAction, PaletteGroup};
use super::local_threads::ThreadHome;
use super::markdown::Markdown;
use super::outdated_lines::FollowedLines;
use super::overview::{OverviewFile, OverviewPage, OverviewStats};
use super::page::Page;
use super::ports::{AgentLaunch, GitHubError, PullOfBranch};
use super::pr_controls::PrControls;
use super::pr_notes::{Note, NoteSubject, NotesButton, PrNotes, ToNotes, ToNotesOutcome};
use super::problems::{Problem, ProblemKind, Problems, Read};
use super::reactions::ReactionChange;
use super::refresh::RefreshBrief;
use super::related_issues::{IssueDetails, RelatedIssue, RelatedIssues};
use super::replacing_jobs::{ReplacingJob, ReplacingJobs};
use super::review_state::{RepinRecord, ReviewState, ReviewStateFile};
use super::reviewing_mark::ReviewingMark;
use super::row_selection::{RowSelection, SelectedLines};
use super::saved_state::SavedState;
use super::step_page::{
    DiffLine, FilePage, LineLocation, PieceDiff, SafetyNetPage, SinceRead, StepPage,
};
use super::switch_to_guided::ReadCarrier;
use super::thread_actions::{
    ThreadAction, ThreadActionOutcome, ThreadActionRequest, ThreadActionResult,
};
use super::threads::{self, CommentId, ReviewThread, ThreadId, ThreadPlacement, ThreadWork};
use super::typing_save::TypingSave;
use super::viewed_sync::{Coverage, SyncOutcome, SyncRequest, ViewedSync};
use super::work_agenda::{PUSH_DEBOUNCE, WorkAgenda, WorkKind};
use crate::file_path::FilePath;
use crate::ide::{Ide, NoIde};
use crate::repin::RepinStatus;
use crate::update::Build;

pub use crate::tour::StepId;

/// What a read tick says was read: a guided tour's step, or a basic tour's whole file.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ReadSubject {
    Step(StepId),
    File(FilePath),
}
pub use kept::Review;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Screen {
    Summary,
    /// A basic tour's first screen, in place of the summary.
    Overview,
    /// One changed file of a basic tour, in place of a step.
    File(FilePath),
    Step(StepId),
    /// After the last step: every change no step shows.
    SafetyNet,
}

impl Screen {
    /// What an Ask about lines of this screen records as the screen it was shown on; the
    /// summary and the overview show no lines.
    pub fn shown_on(&self) -> Option<ShownOn> {
        match self {
            Self::Step(step) => Some(ShownOn::Step { step: step.clone() }),
            Self::SafetyNet => Some(ShownOn::SafetyNet),
            Self::File(path) => Some(ShownOn::File { path: path.clone() }),
            Self::Summary | Self::Overview => None,
        }
    }

    fn sent_from(&self) -> SentFrom {
        match self {
            Self::Step(step) => SentFrom::Step(step.clone()),
            Self::File(path) => SentFrom::File(path.clone()),
            Self::Summary | Self::Overview | Self::SafetyNet => SentFrom::Elsewhere,
        }
    }
}

/// One row of the step tree. Rows come in reading order, so indenting by `depth` draws the tree.
pub struct SidebarEntry<'a> {
    pub id: StepId,
    pub title: &'a str,
    pub depth: usize,
    pub is_current: bool,
    pub is_ticked: bool,
    pub thread_count: usize,
    /// Shown greyed: every piece of the step is gone since the last version.
    pub is_gone: bool,
    /// The step carries a check hint.
    pub has_check: bool,
    pub is_skim: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReadProgress {
    pub read: usize,
    pub total: usize,
}

impl ReadProgress {
    /// How much of the tour is read, from 0 to 1: the length of the trail.
    pub fn share(self) -> f32 {
        if self.total == 0 {
            return 0.;
        }
        self.read.min(self.total) as f32 / self.total as f32
    }

    /// Every step read; a tour without steps has nothing to complete.
    pub fn is_complete(self) -> bool {
        self.total > 0 && self.read >= self.total
    }
}

/// What a tour's read ticks count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadUnit {
    Steps,
    Files,
}

impl ReadUnit {
    pub fn plural(self) -> &'static str {
        match self {
            Self::Steps => "steps",
            Self::Files => "files",
        }
    }
}

/// A step the reader has not read yet, named for a button that leads there.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct UnreadStep {
    pub id: StepId,
    /// Its title is markdown.
    pub title: String,
}

/// What the summary leads the reader to: the start of the tour, the first step not read yet, or
/// once every step is read, finishing the review.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TourLead {
    /// A tour with no steps leads nowhere.
    Nowhere,
    Start(UnreadStep),
    Continue(UnreadStep),
    /// A basic tour leads by files.
    StartFile(UnreadFile),
    ContinueFile(UnreadFile),
    FinishFiles {
        files: usize,
        finish: Option<FinishAction>,
    },
    Finish {
        steps: usize,
        /// The PR takes a review from the viewer; an own PR with nothing to send does not.
        finish: Option<FinishAction>,
    },
}

/// A file the reader has not read yet, named for a button that leads there.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct UnreadFile {
    pub path: FilePath,
}

/// One top-level step of the tour as the summary lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteStop {
    pub id: StepId,
    /// Markdown.
    pub title: String,
    /// Markdown.
    pub why: String,
    /// How many steps sit under it.
    pub details: usize,
    pub is_read: bool,
    pub has_check: bool,
}

/// What the summary counts about the PR, all of it known without GitHub but the threads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SummaryStats {
    pub steps: usize,
    pub files: usize,
    pub added_lines: usize,
    pub removed_lines: usize,
    pub open_threads: usize,
}

/// Where the open screen sits in the reading order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadingPlace {
    /// Counted among the steps from 1; `None` on the summary and the safety net.
    pub step_number: Option<usize>,
    pub step_count: usize,
    /// Counted among a basic tour's files from 1; `None` on the overview and in a guided tour.
    pub file_number: Option<usize>,
    pub file_count: usize,
    pub has_previous: bool,
    pub next: Option<NextScreen>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum NextScreen {
    /// Its title is markdown.
    Step {
        title: String,
    },
    /// A basic tour's next file.
    File {
        path: FilePath,
    },
    SafetyNet,
}

/// What the sidebar counts for the safety net, cheap enough to ask on every change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SafetyNetSize {
    pub pieces: usize,
    pub threads: usize,
}

/// The tour was written for an older head than the PR has now, so it may be stale.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OldHead {
    pub tour_head: Sha,
    pub pr_head: Sha,
}

/// What the reviewer reads and does on one PR. Reached through `Review`, which keeps the page built from it.
pub struct ReviewCore {
    tour: Tour,
    /// The basic sidebar's tree and the reading order of file screens; empty for a guided tour.
    file_tree: FileTree,
    /// Folders the reviewer folded. Not saved: a reopened app shows the tree unfolded.
    folded_folders: BTreeSet<FolderPath>,
    subject: ReviewSubject,
    /// Where code the IDE cannot open goes: the PR's repo on GitHub, or nowhere for a local review.
    code_host: CodeHost,
    /// What a local review's overview shows; set by the app from the clone, `None` for a PR.
    local_facts: Option<LocalFacts>,
    /// What a local review is called in its window and sidebar; made when its facts arrive, not per frame.
    local_heading: Option<Arc<str>>,
    branch_link: BranchLink,
    related_issues: RelatedIssues,
    base: Sha,
    head: Sha,
    /// Unknown until GitHub answers.
    pr_head: Option<Sha>,
    /// A local review's folder against the snapshot it reads; unknown until the folder is looked at.
    on_disk: Option<OnDisk>,
    /// What GitHub last said of the viewer's sent review; kept as it was while a read fails.
    last_review: Option<SubmittedReview>,
    /// `None` for a local review, which has no PR to change.
    pr_controls: Option<PrControls>,
    files: Vec<ChangedFile>,
    /// Guided: summary, then parent, its children, the next parent (from the big picture to the
    /// details), then the safety net when it holds anything. Basic: overview, then the files in
    /// tree order.
    steps_in_reading_order: Vec<Screen>,
    has_uncovered_changes: bool,
    pulled_threads: Vec<ReviewThread>,
    followed_lines: BTreeMap<ThreadId, FollowedLines>,
    threads: ThreadPlacement,
    /// Threads fold by default when resolved, outdated or started by a bot; these are the ones the reviewer flipped.
    thread_fold_flipped: BTreeSet<ThreadId>,
    screen: Screen,
    state: SavedState,
    /// Skim and ticked steps start folded; these are the steps whose fold the reviewer flipped.
    /// Not saved: a reopened app shows steps folded again.
    fold_flipped: BTreeSet<StepId>,
    /// File boxes of a step opened or folded against the step's fold, by step and path.
    box_fold_flipped: BTreeSet<(Screen, FilePath)>,
    /// The tour's own folder, whose pictures its summary shows and whose files its links open.
    tour_files: PathBuf,
    viewed_sync: ViewedSync,
    problems: Problems,
    /// The diff from each older head some step was read at to the tour's head.
    /// A head that cannot be read (gone after a force push) is left out: those steps show the full diff.
    changes_since: BTreeMap<Sha, Vec<ChangedFile>>,
    /// Steps read before whose full diff the reviewer asked for. Not saved.
    full_diff_shown: BTreeSet<ReadSubject>,
    /// Until told, code opens in the IDE with a note that its lines may differ.
    working_copy: WorkingCopy,
    comment_desk: CommentDesk,
    display_options: DisplayOptions,
    pr_notes: PrNotes,
    ask_desk: AskDesk,
    work: WorkAgenda,
    jobs: ReplacingJobs,
    reviewing_mark: ReviewingMark,
    /// Agent replies taken while a re-pin ran, which the re-pinned review applies and saves.
    replies_after_repin: Vec<(HandOffId, AgentReplied)>,
    /// The last news of the tour agent said tour agents cannot be reached. Not saved.
    agent_link_is_off: bool,
    /// What is known of the guided tour of a basic review's PR. Not saved: the app tells it at a build.
    guided_tour: GuidedTourProgress,
    /// After a switch from a basic tour: how many steps started read. Not saved.
    steps_started_read: Option<usize>,
    /// After a switch from a basic tour, until the hand-over: what carries the files read on the
    /// basic review into the steps when this review takes over its whole state. Not saved.
    read_carrier: Option<ReadCarrier>,
    /// The screen whose end the window last had on show; the forward key ticks a step only then.
    /// Not saved, and no input of the page.
    seen_end_of: Option<Screen>,
}

impl Review {
    /// Code is read at the tour's own base and head, not the PR's current ones.
    pub fn open(
        tour: GuidedTour,
        source: &dyn CodeSource,
        state_file: ReviewStateFile,
    ) -> anyhow::Result<Self> {
        ReviewCore::open(Tour::Guided(tour), source, state_file).map(Self::keeping_page_of)
    }

    /// A review with no steps: it opens on the overview and reads one screen per changed file.
    pub fn open_basic(
        tour: BasicTour,
        source: &dyn CodeSource,
        state_file: ReviewStateFile,
    ) -> anyhow::Result<Self> {
        ReviewCore::open(Tour::Basic(tour), source, state_file).map(Self::keeping_page_of)
    }

    /// The pictures of the screen that are not loaded yet; the app loads them off the main thread
    /// only while their step is shown.
    pub fn image_preview_requests(&self) -> Vec<ImagePreviewRequest> {
        self.page()
            .images_to_load()
            .iter()
            .filter_map(|path| self.image_preview_request(path))
            .collect()
    }

    /// Opens the comment box the gesture asks for, which then takes the keys, or says why it
    /// cannot open. `Ok(None)` when a drag ended on no rows.
    pub fn open_comment_box(
        &mut self,
        gesture: CommentGesture,
    ) -> Result<Option<CommentPlace>, CommentRefusal> {
        match gesture {
            CommentGesture::DragEnded => {
                let page = Arc::clone(self.page());
                self.finish_selection(&page)
            }
            CommentGesture::OnFile(path) => self.begin_file_comment(path).map(Some),
            CommentGesture::AskAnswer(ask) => self.use_as_comment(ask).map(Some),
        }
    }

    /// What the comment box's Ask is about: its lines, on this screen. `None` for a comment on a
    /// file, or lines not on this screen.
    pub fn ask_place_of(&self, comment: &CommentPlace) -> Option<Place> {
        let page = self.page();
        let selected = page.selected(&RowSelection::of(comment)?)?;
        Some(page.place_of(&selected))
    }

    /// The place an Ask about selected rows is about: the rows and the pieces holding them.
    pub fn place_of_lines(&self, selected: &SelectedLines) -> Place {
        self.page().place_of(selected)
    }

    /// The rows being dragged over, for drawing them.
    pub fn selected_lines(&self) -> Option<SelectedLines> {
        self.page().selected(self.row_selection()?)
    }

    /// Adds a suggestion block of the commented lines to the draft, after what is typed already.
    /// `None` when the lines cannot be suggested on, are not on the screen, or there is no draft.
    pub fn suggest_change(&mut self, place: &CommentPlace) -> anyhow::Result<Option<String>> {
        let Some(rows) = RowSelection::of(place).and_then(|lines| self.page().selected(&lines))
        else {
            return Ok(None);
        };
        CommentDesk::suggest_change(&mut self.state, place, rows.lines())
    }

    /// A review reopened after a re-pin goes on with what the one it replaces was doing: what its
    /// comment boxes were doing (which takes the keys, which are sending and why a send did not
    /// go), its work, whose outcomes still land here, its branch link and PR controls, which those
    /// outcomes filled, and which screen's end the window saw. What the config and the disk say is
    /// `ReviewBuilder`'s, which built this review.
    pub fn carry_on_from(&mut self, replaced: Self) {
        let replaced = replaced.into_core();
        let switched_to_guided =
            matches!(replaced.tour, Tour::Basic(_)) && matches!(self.tour, Tour::Guided(_));
        self.comment_desk.keep_boxes_of(replaced.comment_desk);
        self.work = replaced.work;
        self.jobs = replaced.jobs;
        self.branch_link = replaced.branch_link;
        self.related_issues.carry_on_from(replaced.related_issues);
        if let (Some(controls), Some(replaced)) = (&mut self.pr_controls, replaced.pr_controls) {
            controls.carry_on_from(replaced);
        }
        if !switched_to_guided {
            self.guided_tour = replaced.guided_tour;
        }
        self.seen_end_of = replaced.seen_end_of;
        self.ask_desk = replaced.ask_desk;
        if switched_to_guided {
            self.take_over_from_basic(&replaced.state);
            self.follow_file_asks_into_the_guided_tour();
        }
        self.check_asks_for_code_changes();
        for (id, replied) in replaced.replies_after_repin {
            if let Err(error) = self.hand_off_replied(id, &replied) {
                eprintln!("could not carry the reply to {id} into the re-pinned review: {error:#}");
            }
        }
    }

    /// Opens the Ask panel on a place, with notes on the answers whose code changed since.
    pub fn open_ask(
        &mut self,
        place: Place,
        label: String,
        settings: &AskSettings,
        now: Instant,
        processes: &mut dyn AskProcesses,
    ) {
        let clone = self.ask_desk.clone_folder().cloned();
        let target = self.ask_target(place, label, clone, settings);
        let head = self.head().clone();
        self.check_asks_for_code_changes();
        self.change_asks(|desk| desk.open(target, &head, now, processes));
    }

    pub(super) fn takes_over_from_basic_with(&mut self, carrier: ReadCarrier) {
        self.read_carrier = Some(carrier);
    }

    /// The switch carried the state saved when it started; the basic review stayed on screen and
    /// may have read files, queued comments, handed off and typed since, and saved its own state
    /// over the carried one. This review takes the basic review's whole state as of now, carries
    /// its reads again, and saves over what it wrote.
    fn take_over_from_basic(&mut self, basic: &ReviewState) {
        let Some(carrier) = self.read_carrier.take() else {
            return;
        };
        let carried = carrier.carry(&basic.file_read_heads());
        let saved = self
            .state
            .record(|state| state.take_over_from_basic(basic, &carried));
        if let Err(error) = saved {
            eprintln!("could not save the review state handed over to the guided tour: {error:#}");
        }
        self.steps_started_read = Some(self.progress().read);
    }

    /// An Ask about lines of a file screen finds its rows in the first step that shows all its
    /// pieces, else in the safety net; when neither shows them its place is gone, as after a re-pin.
    fn follow_file_asks_into_the_guided_tour(&mut self) {
        let Some(guided) = self.tour.guided() else {
            return;
        };
        let steps: Vec<(StepId, Vec<PieceId>)> = guided
            .steps()
            .into_iter()
            .map(|step| {
                let shown = super::step_page::pieces_of(step, guided, &self.files);
                (
                    step.id.clone(),
                    shown.iter().map(|piece| piece.id.clone()).collect(),
                )
            })
            .collect();
        let in_safety_net: Vec<PieceId> = self
            .safety_net()
            .map_or_default(|page| page.pieces.iter().map(|piece| piece.id.clone()).collect());
        let shows_all = |shown: &[PieceId], pieces: &[PieceId]| {
            !pieces.is_empty() && pieces.iter().all(|piece| shown.contains(piece))
        };
        let moved = self.ask_desk.move_lines_asks(|pieces| {
            steps
                .iter()
                .find(|(_, shown)| shows_all(shown, pieces))
                .map(|(step, _)| ShownOn::Step { step: step.clone() })
                .or_else(|| shows_all(&in_safety_net, pieces).then_some(ShownOn::SafetyNet))
        });
        if let Err(error) = moved {
            eprintln!("could not save the Asks moved into the guided tour: {error:#}");
        }
    }

    fn check_asks_for_code_changes(&mut self) {
        let changed = self
            .ask_desk
            .all()
            .iter()
            .filter(|ask| self.code_changed_since(ask))
            .map(|ask| ask.id)
            .collect();
        self.change_asks(|desk| desk.code_changed_is(changed));
    }
}

impl ReviewCore {
    fn open(
        tour: Tour,
        source: &dyn CodeSource,
        state_file: ReviewStateFile,
    ) -> anyhow::Result<Self> {
        let (mut state, saved_review_kept_as) = state_file.load_or_set_aside()?;
        let asks = Asks::loaded(state.take_asks(), Arc::new(state_file.clone()));
        let tour_files = state_file.tour_files().to_owned();
        let subject = tour.pin().subject()?;
        let code_host = subject
            .github_repo()
            .map_or(CodeHost::LocalClone, |repo| CodeHost::GitHub(repo.clone()));
        let viewed_sync = if subject.syncs_viewed_marks() {
            ViewedSync::default()
        } else {
            ViewedSync::without_github_marks()
        };
        let (base, head) = (tour.pin().base.clone(), tour.pin().head.clone());
        let reviewing_mark = ReviewingMark::new(head.clone());
        let files = pieces::cut(source, &base, &head)?;
        let (file_tree, steps_in_reading_order, screen) = match &tour {
            Tour::Guided(guided) => {
                let steps = guided
                    .steps()
                    .into_iter()
                    .map(|step| Screen::Step(step.id.clone()));
                (
                    FileTree::default(),
                    std::iter::once(Screen::Summary).chain(steps).collect(),
                    Screen::Summary,
                )
            }
            Tour::Basic(_) => {
                let file_tree = FileTree::of(files.iter().map(|file| &file.path));
                let screens = file_tree.reading_order().into_iter().map(Screen::File);
                (
                    file_tree,
                    std::iter::once(Screen::Overview).chain(screens).collect(),
                    Screen::Overview,
                )
            }
        };
        let has_uncovered_changes = tour
            .guided()
            .is_some_and(|guided| threads::uncovered(guided, &files));
        let read_heads: BTreeSet<&Sha> = tour
            .guided()
            .into_iter()
            .flat_map(GuidedTour::steps)
            .filter_map(|step| read_head_of(&state, &step.id.clone()))
            .chain(
                files
                    .iter()
                    .filter_map(|file| read_head_of_file(&state, &file.path)),
            )
            .filter(|read_head| **read_head != head)
            .collect();
        let changes_since = read_heads
            .into_iter()
            .filter_map(|read_head| {
                let files = pieces::cut(source, read_head, &head).ok()?;
                Some((read_head.clone(), files))
            })
            .collect();
        let mut display_options = DisplayOptions::for_code(base.clone(), head.clone());
        display_options.cut_markdown(&files);
        let local_heading = Self::heading_of(&subject, None);
        let branch_link = BranchLink::of(&subject);
        let related_issues = RelatedIssues::of(&subject, &tour, state.pr_description());
        let pr_controls = PrControls::of(&subject);
        Ok(Self {
            tour,
            file_tree,
            folded_folders: BTreeSet::new(),
            subject,
            code_host,
            local_heading,
            local_facts: None,
            branch_link,
            related_issues,
            base,
            head,
            pr_head: None,
            on_disk: None,
            last_review: None,
            pr_controls,
            files,
            steps_in_reading_order,
            has_uncovered_changes,
            pulled_threads: Vec::new(),
            followed_lines: BTreeMap::new(),
            threads: ThreadPlacement::default(),
            thread_fold_flipped: BTreeSet::new(),
            screen,
            state: SavedState::new(state, state_file),
            fold_flipped: BTreeSet::new(),
            box_fold_flipped: BTreeSet::new(),
            viewed_sync,
            problems: {
                let mut problems = Problems::default();
                if let Some(reason) = source.fallback_reason() {
                    problems.raise(Problem::CodeOnGitHub(reason.to_owned()));
                }
                if let Some(backup) = saved_review_kept_as {
                    problems.raise(Problem::SavedReviewNotRead(backup));
                }
                problems
            },
            changes_since,
            full_diff_shown: BTreeSet::new(),
            working_copy: WorkingCopy::Elsewhere,
            comment_desk: CommentDesk::default(),
            display_options,
            pr_notes: PrNotes::default(),
            ask_desk: AskDesk::new(asks, source.local_clone().map(Path::to_path_buf)),
            work: WorkAgenda::default(),
            jobs: ReplacingJobs::default(),
            reviewing_mark,
            replies_after_repin: Vec::new(),
            agent_link_is_off: false,
            guided_tour: GuidedTourProgress::NotAsked,
            steps_started_read: None,
            read_carrier: None,
            seen_end_of: None,
            tour_files,
        })
    }

    /// Colouring reads whole files, so the app runs this off the main thread and hands the result to `set_highlights`.
    pub fn highlight_request(&self) -> HighlightRequest {
        HighlightRequest::for_files(&self.files, &self.base, &self.head)
    }

    /// Context lines come from the files at head, read off the main thread like the colours.
    pub fn files_at_head_request(&self) -> FilesAtHeadRequest {
        FilesAtHeadRequest::for_files(&self.files, &self.head)
    }

    /// Files read at another head, e.g. before a re-pin, are dropped. Says whether they were kept.
    pub fn set_files_at_head(&mut self, files_at_head: FilesAtHead) -> bool {
        let kept = self.display_options.set_files_at_head(files_at_head);
        if kept {
            self.display_options.cut_markdown(&self.files);
            self.place_threads();
        }
        kept
    }

    fn image_preview_request(&self, path: &FilePath) -> Option<ImagePreviewRequest> {
        let file = self.files.iter().find(|file| file.path == *path)?;
        Some(ImagePreviewRequest::for_file(file, &self.base, &self.head))
    }

    /// Every Ask of the PR, the place the Ask panel is open on and the processes answering.
    pub fn asks(&self) -> &AskDesk {
        &self.ask_desk
    }

    pub fn display_options(&self) -> &DisplayOptions {
        &self.display_options
    }

    /// Nothing of it is saved, so it changes freely, even while a re-pin runs.
    pub fn display_options_mut(&mut self) -> &mut DisplayOptions {
        &mut self.display_options
    }

    pub fn screen(&self) -> &Screen {
        &self.screen
    }

    pub fn subject(&self) -> &ReviewSubject {
        &self.subject
    }

    pub(super) fn thread_home(&self) -> ThreadHome {
        ThreadHome::of(&self.subject, self.state.file())
    }

    /// The send actions a comment box of this review offers; Ask agent is one more for a PR.
    pub fn send_modes(&self) -> &'static [SendMode] {
        if self.subject.comments_are_agent_tasks() {
            &SendMode::LOCAL
        } else {
            &[SendMode::AddToReview, SendMode::CommentNow]
        }
    }

    /// A local review's boxes send tasks for its agent, a PR's send to GitHub.
    pub fn sending(&self) -> Sending {
        if self.subject.comments_are_agent_tasks() {
            Sending::Tasks(&SendMode::LOCAL)
        } else {
            Sending::GitHub
        }
    }

    /// A local review's comments are tasks for its agent: while the link to the agent is off one
    /// sent now stays a draft, and says why; one added to tasks waits for Send tasks all the same.
    fn send_refusal(&self, mode: SendMode) -> Option<CommentRefusal> {
        if !self.send_modes().contains(&mode) {
            Some(CommentRefusal::NotOffered)
        } else if mode == SendMode::SendNow && self.agent_link_is_off {
            Some(CommentRefusal::AgentLinkOff)
        } else {
            None
        }
    }

    pub(super) fn has_seen_end_of_screen(&self) -> bool {
        self.seen_end_of.as_ref() == Some(&self.screen)
    }

    /// For a failure the reviewer must know about; it stays until its kind is cleared.
    pub fn raise_problem(&mut self, problem: Problem) {
        self.problems.raise(problem);
    }

    pub fn clear_problem(&mut self, kind: ProblemKind) {
        self.problems.clear(kind);
    }

    /// The outcome of a Show agent: a terminal that did not come up is a problem until one does.
    pub fn show_agent_result(&mut self, failure: Option<&anyhow::Error>) {
        match failure {
            Some(error) => {
                self.raise_problem(Problem::ShowAgentFailed(format!("{error:#}")));
            }
            None => self.clear_problem(ProblemKind::ShowAgentFailed),
        }
    }

    pub fn use_agent_result(&mut self, failure: Option<&anyhow::Error>) {
        match failure {
            Some(error) => self.raise_problem(Problem::UseAgentFailed(format!("{error:#}"))),
            None => self.clear_problem(ProblemKind::UseAgentFailed),
        }
    }

    pub fn show_agent_failed(&self) -> bool {
        self.problems
            .list()
            .any(|problem| problem.kind() == ProblemKind::ShowAgentFailed)
    }

    pub fn could_not_read(&mut self, read: Read, error: &GitHubError) {
        let affected = Affected::from(read);
        if let GitHubError::NotAnswering(cause) = error {
            self.clear_problem(ProblemKind::NotRead(read));
            self.github_not_answering(*cause, affected);
        } else {
            self.github_answered(affected);
            self.raise_problem(Problem::NotRead(read, error.short_reason()));
        }
    }

    /// GitHub does not answer: one problem for all the reads that fail, and the read is tried
    /// again by itself, further off each time it fails.
    fn github_not_answering(&mut self, cause: OutageCause, affected: Affected) {
        let retry_in = self.problems.github_not_answering(cause, affected);
        self.work.want_in(affected.retried_by(), retry_in);
    }

    fn github_answered(&mut self, affected: Affected) {
        self.problems.github_answered(affected);
    }

    pub fn ide_not_started(&mut self, error: &impl fmt::Display) {
        self.raise_problem(Problem::IdeNotStarted(format!("{error:#}")));
    }

    pub fn ide_started(&mut self) {
        self.clear_problem(ProblemKind::IdeNotStarted);
    }

    pub fn pr_head_is(&mut self, head: Sha) {
        self.clear_problem(ProblemKind::NotRead(Read::PrHead));
        self.github_answered(Affected::NewCommits);
        self.pr_head = Some(head);
    }

    pub fn old_head(&self) -> Option<OldHead> {
        self.pr_head
            .as_ref()
            .filter(|pr_head| **pr_head != self.head)
            .map(|pr_head| OldHead {
                tour_head: self.head.clone(),
                pr_head: pr_head.clone(),
            })
    }

    /// What GitHub said of the PR opened from a local review's branch.
    pub fn pull_of_branch_is(&mut self, found: Option<PullOfBranch>) {
        self.branch_link.pull_of_branch_is(found);
    }

    /// The local reviews on disk that are of this PR's repo, so the review looks for one of the PR's branch.
    /// A PR's head branch never changes, so it is looked up once here, never again on focus.
    pub(super) fn local_reviews_of_repo_are(&mut self, reviews: LocalReviewsOfRepo) {
        self.branch_link.local_reviews_are(reviews);
        self.work.want(WorkKind::LookUpHeadBranch);
    }

    /// The lasting line of the sidebar, or Open PR, from what is known of the other side of the branch.
    pub fn branch_offer(&self) -> Option<BranchOffer> {
        self.branch_link.offer(&self.state)
    }

    /// Not now, on either side's offer, unless the move runs. A decline that could not be written to
    /// the state file is a problem, as the offer then comes back at the next open.
    pub fn decline_the_move(&mut self) {
        if self.jobs.is_running(ReplacingJob::MoveToPr) {
            return;
        }
        let declined = self.branch_link.decline(&mut self.state);
        match declined {
            Ok(()) => self.clear_problem(ProblemKind::MoveNotDeclined),
            Err(error) => self.raise_problem(Problem::MoveNotDeclined(format!("{error:#}"))),
        }
    }

    /// What a local review's folder is against its snapshot, once the folder was looked at.
    pub fn working_tree_is(&mut self, on_disk: OnDisk) {
        self.working_copy = WorkingCopy::of(on_disk == OnDisk::Same);
        self.on_disk = Some(on_disk);
    }

    /// The line the sidebar shows when a local review's folder is not its snapshot any more.
    pub fn disk_news(&self) -> Option<DiskNews> {
        let change_set = self.subject.local()?;
        DiskNews::of(self.on_disk.as_ref()?, change_set.branch())
    }

    /// The overview's facts: the PR's title and description, the counts and the files.
    pub fn overview_page(&self) -> OverviewPage {
        let stats = self.summary_stats();
        let files = self
            .file_tree
            .reading_order()
            .into_iter()
            .filter_map(|path| {
                let file = self.files.iter().find(|file| file.path == path)?;
                Some(OverviewFile {
                    added_lines: file.added,
                    removed_lines: file.removed,
                    is_read: self.is_file_ticked(&path),
                    change: self.local_change_of(&path),
                    path,
                })
            })
            .collect();
        OverviewPage {
            title: self.title().to_owned(),
            description: self.pr_description(),
            stats: OverviewStats {
                files: stats.files,
                added_lines: stats.added_lines,
                removed_lines: stats.removed_lines,
                open_threads: stats.open_threads,
            },
            files,
            write_guided_tour_refusal: self.write_guided_tour_refusal(),
            local: self.local_facts.clone(),
        }
    }

    fn local_change_of(&self, path: &FilePath) -> Option<LocalChange> {
        self.local_facts.as_ref()?.change_of(path)
    }

    /// What the app read of the clone for a local review's overview. Not saved: git tells it again.
    pub fn local_facts_are(&mut self, facts: LocalFacts) {
        self.local_heading = Self::heading_of(&self.subject, Some(&facts));
        self.local_facts = Some(facts);
    }

    /// `None` for a PR, and for a local review until the app has read its clone.
    pub fn local_facts(&self) -> Option<&LocalFacts> {
        self.local_facts.as_ref()
    }

    /// The open file of a basic tour with every piece of it.
    pub fn file_page(&self) -> Option<FilePage> {
        let Screen::File(path) = &self.screen else {
            return None;
        };
        let file = self.files.iter().find(|file| file.path == *path)?;
        let links = self.links();
        let display = self.display(&links);
        let mut page = FilePage::of(file, &self.files, &display);
        page.change = self.local_change_of(path);
        page.threads = threads::views(
            self.threads.in_file(path),
            &page.pieces,
            &self.thread_fold_flipped,
            self.display_options.files_at_head(),
            &self.thread_work(),
            self.state.hand_offs(),
        );
        self.mark_repin_status(&mut page.pieces);
        if let Some((read_head, since_files)) = self.changes_since_file_read(path) {
            let showing_full_diff = self
                .full_diff_shown
                .contains(&ReadSubject::File(path.clone()));
            page.since_read = Some(SinceRead {
                read_at: read_head.clone(),
                showing_full_diff,
            });
            if !showing_full_diff {
                let links_from_read_head = CodeLinks {
                    host: &self.code_host,
                    base: &read_head,
                    head: &self.head,
                };
                let since_display = self.display(&links_from_read_head);
                page.pieces = file
                    .pieces
                    .iter()
                    .map(|piece| {
                        PieceDiff::since_read(
                            &PiecePart::whole(piece),
                            since_files,
                            Some(file),
                            &since_display,
                        )
                    })
                    .collect();
                self.mark_repin_status(&mut page.pieces);
            }
        }
        Some(page)
    }

    /// What the window shows where a guided tour shows its title and summary.
    pub fn basic_mode_label(&self) -> Option<&'static str> {
        match &self.tour {
            Tour::Basic(_) => Some("Basic mode"),
            Tour::Guided(_) => None,
        }
    }

    /// A basic tour of the same pin and title, for a snapshot scene to show the PR in Basic mode.
    #[cfg(feature = "snapshots")]
    pub fn basic_tour_of_same_pin(&self) -> BasicTour {
        BasicTour::new(self.tour.pin().clone(), self.title().to_owned())
    }

    /// A basic tour has the PR's own title.
    pub fn title(&self) -> &str {
        match &self.tour {
            Tour::Basic(basic) => &basic.pr_title,
            Tour::Guided(guided) => &guided.title,
        }
    }

    /// The window's title.
    pub fn window_title(&self) -> String {
        match &self.local_heading {
            Some(heading) => heading.to_string(),
            None => format!("{}: {}", self.subject, self.title()),
        }
    }

    /// The line a local review's sidebar leads with, naming what is reviewed; `None` for a PR.
    pub fn local_heading(&self) -> Option<Arc<str>> {
        self.local_heading.clone()
    }

    fn heading_of(subject: &ReviewSubject, facts: Option<&LocalFacts>) -> Option<Arc<str>> {
        let change_set = subject.local()?;
        let branch = change_set.branch().as_str();
        Some(
            match facts.and_then(|facts| facts.repo.as_ref()) {
                Some(repo) => format!("Local changes · {repo} · {branch}"),
                None => format!("Local changes · {branch}"),
            }
            .into(),
        )
    }

    /// What GitHub last said of the issue; `None` for a ticket and until it answers.
    pub fn issue_details(&self, link: &IssueLink) -> Option<Arc<IssueDetails>> {
        self.related_issues.details_of(link)
    }

    /// The issues the review is about, best source first; empty when none names one.
    pub fn related_issues(&self) -> Arc<[RelatedIssue]> {
        self.related_issues.shown()
    }

    /// The issues a snapshot scene shows, as if GitHub linked them to the PR.
    #[cfg(feature = "snapshots")]
    pub fn stage_related_issues(&mut self, issues: Vec<(RelatedIssue, Option<IssueDetails>)>) {
        self.related_issues.staged(issues);
    }

    #[cfg(feature = "snapshots")]
    pub fn unstage_related_issues(&mut self) {
        self.related_issues.unstaged();
    }

    /// Whether the review has a PR page to open.
    pub fn can_open_on_github(&self) -> bool {
        self.subject.has_github_page()
    }

    /// A local review whose branch has no open PR yet, so its agent can be asked to open one.
    pub fn can_open_pr(&self) -> bool {
        self.branch_link.can_open_pr()
    }

    pub fn summary(&self) -> &[String] {
        self.tour.guided().map_or(&[], |guided| &guided.summary)
    }

    /// Under the summary's bullets and above the PR description. `None` when the tour has none.
    pub fn reviewer_summary(&self) -> Option<Markdown> {
        let body = self.reviewer_summary_text();
        let markdown = Markdown::parse_with_files(body, &self.tour_files);
        (!body.trim().is_empty()).then(|| markdown.nested_one_level())
    }

    fn reviewer_summary_text(&self) -> &str {
        self.tour
            .guided()
            .map_or("", |guided| &guided.reviewer_summary)
    }

    /// The PR's body, under the summary: the one GitHub gave last, until it answers again.
    /// `None` for a PR with no description.
    pub fn pr_description(&self) -> Option<Markdown> {
        let body = self.state.pr_description();
        (!body.trim().is_empty()).then(|| Markdown::parse(body).nested_one_level())
    }

    /// `None` when the tour has no such diagram, picture or summary, or the review has no PR.
    pub fn notes_button(&self, subject: &NoteSubject) -> Option<NotesButton> {
        self.subject.pull_request()?;
        let note = self.note(subject)?;
        Some(
            self.pr_notes
                .button(subject, self.state.pr_description(), &note),
        )
    }

    /// `None` while the subject is being added, or when the description holds it.
    pub fn start_adding_to_notes(&mut self, subject: &NoteSubject) -> Option<ToNotes> {
        let note = self.note(subject)?;
        if note.is_in(self.state.pr_description()) {
            return None;
        }
        self.pr_notes
            .start(self.subject.pull_request()?, subject, note)
    }

    pub fn apply_to_notes(&mut self, outcome: ToNotesOutcome) -> anyhow::Result<()> {
        match self.pr_notes.finish(outcome) {
            Some(description) => self.set_pr_description(&description),
            None => Ok(()),
        }
    }

    fn note(&self, subject: &NoteSubject) -> Option<Note> {
        match subject {
            NoteSubject::StepDiagram(step) => {
                let found = self.step(step)?;
                Some(Note::diagram(&found.title, found.diagram.as_deref()?))
            }
            NoteSubject::SummaryPicture(path) => {
                let summary = self.reviewer_summary()?;
                let (_, alt) = summary
                    .pictures()
                    .into_iter()
                    .find(|(picture, _)| picture == path)?;
                Note::picture(path, alt)
            }
            NoteSubject::ReviewerSummary => {
                let summary = self.reviewer_summary_text();
                if summary.trim().is_empty() {
                    return None;
                }
                Note::reviewer_summary(summary, &self.tour_files)
            }
        }
    }

    /// Kept in the state file only when it changed, since every window focus pulls it again.
    pub fn set_pr_description(&mut self, body: &str) -> anyhow::Result<()> {
        self.clear_problem(ProblemKind::NotRead(Read::PrDescription));
        self.github_answered(Affected::Description);
        if self.state.pr_description() != body {
            self.change_related_issues(|related| related.description_is(body));
            self.state.record(|state| state.saw_pr_description(body))?;
        }
        Ok(())
    }

    fn thread_work(&self) -> ThreadWork<'_> {
        ThreadWork {
            reply_drafts: self.state.reply_drafts(),
            comment_edits: self.comment_desk.comment_edits(),
            delete_asked: self.comment_desk.delete_asked(),
        }
    }

    pub fn current_step(&self) -> Option<StepPage<'_>> {
        let Screen::Step(id) = &self.screen else {
            return None;
        };
        let guided = self.tour.guided()?;
        let step = self.step(id)?;
        let links = self.links();
        let display = self.display(&links);
        let mut page = StepPage::of(step, guided, &self.files, &display, self.is_folded(id));
        page.is_ticked = self.is_ticked(id);
        page.threads = threads::views(
            self.threads.in_step(id),
            &page.pieces,
            &self.thread_fold_flipped,
            self.display_options.files_at_head(),
            &self.thread_work(),
            self.state.hand_offs(),
        );
        if let Some(record) = self.repin_record() {
            self.mark_repin_status(&mut page.pieces);
            page.gone_pieces = record.gone_count(id);
            page.text_may_be_old = page
                .pieces
                .iter()
                .any(|piece| piece.repin_status == Some(RepinStatus::Changed));
        }
        page.is_gone = self.is_gone(step);
        if let Some((read_head, since_files)) = self.changes_since_read(id) {
            let showing_full_diff = self
                .full_diff_shown
                .contains(&ReadSubject::Step(id.clone()));
            page.since_read = Some(SinceRead {
                read_at: read_head.clone(),
                showing_full_diff,
            });
            if !showing_full_diff {
                let links_from_read_head = CodeLinks {
                    host: &self.code_host,
                    base: &read_head,
                    head: &self.head,
                };
                let since_display = self.display(&links_from_read_head);
                page.pieces = step
                    .pieces
                    .iter()
                    .flat_map(|pointer| coverage::shown_parts(pointer, guided, &self.files))
                    .map(|part| {
                        let pr_file = self
                            .files
                            .iter()
                            .find(|file| file.path == *part.piece.id.path());
                        PieceDiff::since_read(&part, since_files, pr_file, &since_display)
                    })
                    .collect();
                self.mark_repin_status(&mut page.pieces);
            }
        }
        Some(page)
    }

    /// A basic tour has no steps to go stale, so its brief is empty.
    pub fn refresh_brief(&self) -> RefreshBrief {
        self.tour
            .guided()
            .map_or_default(|guided| RefreshBrief::of(guided, &self.files, self.state.last_repin()))
    }

    fn changes_since_read(&self, id: &StepId) -> Option<(Sha, &[ChangedFile])> {
        let read_head = read_head_of(&self.state, id)?;
        let since_files = self.changes_since.get(read_head)?;
        Some((read_head.clone(), since_files))
    }

    fn changes_since_file_read(&self, path: &FilePath) -> Option<(Sha, &[ChangedFile])> {
        let read_head = read_head_of_file(&self.state, path)?;
        let since_files = self.changes_since.get(read_head)?;
        Some((read_head.clone(), since_files))
    }

    pub fn toggle_full_diff(&mut self, id: &StepId) {
        self.toggle_full_diff_of(&ReadSubject::Step(id.clone()));
    }

    /// Flips a step or file read before between the change since reading and the full diff, for the session.
    pub fn toggle_full_diff_of(&mut self, subject: &ReadSubject) {
        if !self.full_diff_shown.remove(subject) {
            self.full_diff_shown.insert(subject.clone());
        }
    }

    fn step(&self, id: &StepId) -> Option<&Step> {
        self.tour.guided()?.find_step(id)
    }

    /// A record of a re-pin to another head says nothing about this tour any more.
    fn repin_record(&self) -> Option<&RepinRecord> {
        self.state
            .last_repin()
            .filter(|record| record.to_head == self.head)
    }

    fn mark_repin_status(&self, pieces: &mut [PieceDiff]) {
        if let Some(record) = self.repin_record() {
            for piece in pieces {
                piece.repin_status = record.status_of(&piece.id);
            }
        }
    }

    fn is_gone(&self, step: &Step) -> bool {
        let Some(guided) = self.tour.guided() else {
            return false;
        };
        let id = step.id.clone();
        step.diagram.is_none()
            && self
                .repin_record()
                .is_some_and(|record| record.gone_count(&id) > 0)
            && step
                .pieces
                .iter()
                .all(|pointer| coverage::shown_parts(pointer, guided, &self.files).is_empty())
    }

    pub fn head(&self) -> &Sha {
        &self.head
    }

    /// What the AI is told about a place: pointers, the shown diff lines, step title and why.
    pub fn ask_context(&self, place: &Place) -> Option<String> {
        ask_context::context_for(&self.tour, &self.files, &self.links(), place)
    }

    /// What the reviewer's own tour agent is typed for an Ask. In a basic tour it is told so, and which
    /// file the Ask is on, since there is no tour text to find the place in.
    pub fn ask_hand_off_text(&self, ask: &Ask, place: &str) -> String {
        let subject = &self.subject;
        let Tour::Basic(_) = &self.tour else {
            return ask.hand_off_text(subject, place);
        };
        let file = match &ask.place {
            Place::File { path }
            | Place::Lines {
                shown_on: ShownOn::File { path },
                ..
            } => Some(path),
            Place::Tour | Place::Step { .. } | Place::Piece { .. } | Place::Lines { .. } => None,
        };
        let named_file = file.map_or_else(String::new, |file| format!("\nFile: {file}"));
        let reviewed = subject.owner_of_changed_files();
        let basic_place = format!(
            "{place}{named_file}\nThe tour is a basic tour: {reviewed} changed files, with no tour file. \
             Read the changed code with `guided-review outline {subject}` and \
             `guided-review show {subject} <piece ids> --around`."
        );
        ask.hand_off_text(subject, &basic_place)
    }

    /// A re-pin since the Ask changed, added or took away a piece it is about: its answer may be old.
    pub fn code_changed_since(&self, ask: &Ask) -> bool {
        if ask.head == self.head {
            return false;
        }
        let Some(record) = self.repin_record() else {
            return false;
        };
        let moved = |piece: &PieceId| record.status_of(piece) != Some(RepinStatus::Same);
        match &ask.place {
            Place::Tour => false,
            Place::Step { step } => {
                let Some(guided) = self.tour.guided() else {
                    return false;
                };
                record.gone_count(step) > 0
                    || self.step(step).is_some_and(|step| {
                        step.pieces
                            .iter()
                            .flat_map(|pointer| coverage::shown_parts(pointer, guided, &self.files))
                            .any(|part| moved(&part.piece.id))
                    })
            }
            Place::File { path } => self
                .files
                .iter()
                .filter(|file| file.path == *path)
                .flat_map(|file| &file.pieces)
                .any(|piece| moved(&piece.id)),
            Place::Piece { piece, .. } => moved(piece),
            Place::Lines { pieces, .. } => pieces.iter().any(moved),
        }
    }

    /// Puts the Ask's latest answer into the comment box on its lines, after any text already
    /// there. The reviewer edits and sends it; nothing goes to GitHub here.
    fn use_as_comment(&mut self, ask: &Ask) -> Result<CommentPlace, CommentRefusal> {
        if matches!(
            ask.place,
            Place::Tour | Place::Step { .. } | Place::File { .. }
        ) {
            return Err(CommentRefusal::NoLines);
        }
        if ask.head != self.head {
            return Err(CommentRefusal::AskIsOnOlderCode);
        }
        if self.tour_freshness() == TourFreshness::Behind {
            return Err(CommentRefusal::TourIsOld);
        }
        let selected = self.lines_of(&ask.place).ok_or(CommentRefusal::NoLines)?;
        if !selected.take_comment() {
            return Err(CommentRefusal::NotInsideHunk);
        }
        let answer = ask.latest_answer().ok_or(CommentRefusal::NoAnswerYet)?;
        self.comment_desk
            .add_to_draft(&mut self.state, &selected, &self.head, answer)
    }

    /// The rows an Ask about lines or a piece is on, in its step at the tour head.
    fn lines_of(&self, place: &Place) -> Option<SelectedLines> {
        let (shown_on, only_piece) = match place {
            Place::Tour | Place::Step { .. } | Place::File { .. } => return None,
            Place::Piece { step, piece } => (ShownOn::Step { step: step.clone() }, Some(piece)),
            Place::Lines { shown_on, .. } => (shown_on.clone(), None),
        };
        let pieces = match shown_on {
            ShownOn::Step { step } => {
                let links = self.links();
                let display = self.display(&links);
                let guided = self.tour.guided()?;
                StepPage::of(self.step(&step)?, guided, &self.files, &display, false).pieces
            }
            ShownOn::SafetyNet => self.safety_net()?.pieces,
            ShownOn::File { .. } if self.tour.guided().is_some() => return None,
            ShownOn::File { path } => {
                let links = self.links();
                let display = self.display(&links);
                let file = self.files.iter().find(|file| file.path == path)?;
                FilePage::of(file, &self.files, &display).pieces
            }
        };
        let blocks = FileBlock::group(pieces);
        let shown: Vec<&DiffLine> = match only_piece {
            Some(id) => blocks
                .iter()
                .flat_map(|block| &block.pieces)
                .filter(|shown| shown.id == *id)
                .flat_map(PieceDiff::lines)
                .collect(),
            None => blocks.iter().flat_map(FileBlock::lines).collect(),
        };
        let (start, end) = match place {
            Place::Lines { start, end, .. } => (start.clone(), end.clone()),
            _ => (
                shown.first()?.location.clone(),
                shown.last()?.location.clone(),
            ),
        };
        let mut selection = RowSelection::start_at(start);
        selection.extend_to(end);
        selection.resolve(shown.into_iter())
    }

    /// The place the Ask panel opens on, ready only when an AI has code to read and the place is still in the tour.
    pub fn ask_target(
        &self,
        place: Place,
        label: String,
        clone: Option<PathBuf>,
        settings: &AskSettings,
    ) -> AskTarget {
        let Some(clone) = clone else {
            return AskTarget::Unavailable {
                place,
                label,
                why: Unavailable::NoClone,
            };
        };
        let Some(context) = self.ask_context(&place) else {
            return AskTarget::Unavailable {
                place,
                label,
                why: Unavailable::PlaceGone,
            };
        };
        AskTarget::Ready {
            launch: self.agent_launch(clone, settings, None),
            place,
            label,
            context,
        }
    }

    pub fn agent_launch(
        &self,
        cwd: PathBuf,
        settings: &AskSettings,
        resume: Option<String>,
    ) -> AgentLaunch {
        AgentLaunch {
            cwd,
            model: settings.model.clone(),
            effort: settings.effort.clone(),
            system_prompt: ask_context::system_prompt(self.tour.pin().subject_key()),
            resume,
        }
    }

    pub fn sidebar(&self) -> Vec<SidebarEntry<'_>> {
        fn walk<'a>(
            steps: &'a [Step],
            depth: usize,
            current: Option<&StepId>,
            review: &ReviewCore,
            into: &mut Vec<SidebarEntry<'a>>,
        ) {
            for step in steps {
                let id = step.id.clone();
                into.push(SidebarEntry {
                    is_current: current == Some(&id),
                    is_ticked: review.is_ticked(&id),
                    thread_count: review.threads.in_step(&id).len(),
                    is_gone: review.is_gone(step),
                    has_check: step.check.is_some(),
                    is_skim: step.skim,
                    id,
                    title: &step.title,
                    depth,
                });
                walk(&step.children, depth + 1, current, review, into);
            }
        }
        let current = match &self.screen {
            Screen::Step(id) => Some(id),
            Screen::Summary | Screen::Overview | Screen::File(_) | Screen::SafetyNet => None,
        };
        let mut entries = Vec::new();
        if let Some(guided) = self.tour.guided() {
            walk(&guided.steps, 0, current, self, &mut entries);
        }
        entries
    }

    /// The rows of a basic tour's file tree; none for a guided tour, whose sidebar lists steps.
    pub fn basic_sidebar(&self) -> Vec<TreeRow> {
        let current = match &self.screen {
            Screen::File(path) => Some(path),
            _ => None,
        };
        self.file_tree.rows(
            &self.folded_folders,
            current,
            &|path| self.is_file_ticked(path),
            &|path| {
                self.repin_record()
                    .and_then(|record| record.status_of_file(path))
            },
        )
    }

    /// Folds an open folder of the tree and opens a folded one. Not saved.
    pub fn toggle_folder(&mut self, folder: &FolderPath) {
        if !self.folded_folders.remove(folder) {
            self.folded_folders.insert(folder.clone());
        }
    }

    /// `None` when every change is in a step, and always for a basic tour.
    pub fn safety_net(&self) -> Option<SafetyNetPage> {
        let guided = self.tour.guided()?;
        self.reading_order().contains(&Screen::SafetyNet).then(|| {
            let links = self.links();
            let mut page = SafetyNetPage::of(guided, &self.files, &self.display(&links));
            self.mark_repin_status(&mut page.pieces);
            page.threads = threads::views(
                self.threads.in_safety_net(),
                &page.pieces,
                &self.thread_fold_flipped,
                self.display_options.files_at_head(),
                &self.thread_work(),
                self.state.hand_offs(),
            );
            page
        })
    }

    /// `None` when every change is in a step. Does not build the page.
    pub fn safety_net_size(&self) -> Option<SafetyNetSize> {
        let guided = self.tour.guided()?;
        self.reading_order()
            .contains(&Screen::SafetyNet)
            .then(|| SafetyNetSize {
                pieces: coverage::safety_net(guided, &self.files).len(),
                threads: self.threads.in_safety_net().len(),
            })
    }

    fn display<'a>(&'a self, links: &'a CodeLinks<'a>) -> DiffDisplay<'a> {
        self.display_options.display(links)
    }

    /// Where a screen sits in the reading order, without building the order; the safety net is last.
    pub fn reading_position(&self, screen: &Screen) -> Option<usize> {
        match screen {
            Screen::SafetyNet => Some(self.steps_in_reading_order.len()),
            Screen::Summary | Screen::Overview | Screen::File(_) | Screen::Step(_) => self
                .steps_in_reading_order
                .iter()
                .position(|shown| shown == screen),
        }
    }

    fn reading_order(&self) -> Vec<Screen> {
        let has_safety_net = self.has_uncovered_changes || !self.threads.in_safety_net().is_empty();
        self.steps_in_reading_order
            .iter()
            .cloned()
            .chain(has_safety_net.then_some(Screen::SafetyNet))
            .collect()
    }

    pub fn set_threads(&mut self, threads: &[ReviewThread]) {
        self.clear_problem(ProblemKind::ThreadsNotRead);
        self.github_answered(Affected::Comments);
        self.pulled_threads = threads.to_vec();
        self.place_threads();
    }

    /// Outdated threads hang under the lines they were followed to; the others stay after the code.
    pub fn set_followed_lines(&mut self, followed: BTreeMap<ThreadId, FollowedLines>) {
        self.followed_lines = followed;
        self.place_threads();
    }

    /// The threads shown stay as they were, named as possibly old until a read goes through.
    pub fn threads_not_read(&mut self, error: &GitHubError) {
        if let GitHubError::NotAnswering(cause) = error {
            self.clear_problem(ProblemKind::ThreadsNotRead);
            self.github_not_answering(*cause, Affected::Comments);
        } else {
            self.github_answered(Affected::Comments);
            self.raise_problem(Problem::ThreadsNotRead(error.short_reason()));
        }
    }

    /// Placed by the rows a step shows, which depend on the files read at head, so again when those arrive.
    fn place_threads(&mut self) {
        let links = self.links();
        let display = DiffDisplay {
            highlights: &Highlights::default(),
            files_at_head: self.display_options.files_at_head(),
            more_context: &HashMap::new(),
            ignores_whitespace: false,
            links: &links,
        };
        self.threads = ThreadPlacement::of(
            &self.tour,
            &self.files,
            &self.pulled_threads,
            &self.followed_lines,
            &display,
        );
    }

    pub fn toggle_thread_fold(&mut self, thread_id: &ThreadId) {
        if !self.thread_fold_flipped.remove(thread_id) {
            self.thread_fold_flipped.insert(thread_id.clone());
        }
    }

    pub fn pending_review_is(&mut self, exists: bool) {
        self.clear_problem(ProblemKind::NotRead(Read::PendingReview));
        self.github_answered(Affected::PendingReview);
        self.comment_desk.pending_review_is(exists);
    }

    pub fn last_review_is(&mut self, last: Option<SubmittedReview>) {
        self.clear_problem(ProblemKind::NotRead(Read::LastReview));
        self.github_answered(Affected::LastReview);
        self.reviewing_mark.last_review_is(last.as_ref());
        self.last_review = last;
        self.want_reviewing_mark_sent();
    }

    fn want_reviewing_mark_sent(&mut self) {
        if self.reviewing_mark.change_to_send().is_some() {
            self.work.want(WorkKind::ChangeReviewingMark);
        }
    }

    fn reviewing_mark_landed(&mut self, change: ReactionChange, result: Result<(), GitHubError>) {
        match result {
            Ok(()) => {
                self.clear_problem(ProblemKind::ReviewingMarkNotChanged);
                self.reviewing_mark.sent(change);
            }
            Err(error) => {
                self.raise_problem(Problem::ReviewingMarkNotChanged(error.short_reason()));
            }
        }
    }

    /// The viewer's latest sent review, from this app or the GitHub web page.
    pub fn last_review(&self) -> Option<&SubmittedReview> {
        self.last_review.as_ref()
    }

    /// The PR has commits the last review did not see. Not so while the PR's head is unknown.
    pub fn last_review_is_old(&self) -> bool {
        match (&self.last_review, &self.pr_head) {
            (Some(review), Some(pr_head)) => review.commit != *pr_head,
            _ => false,
        }
    }

    /// "Comment now" shows only when there is surely no pending review.
    fn tour_freshness(&self) -> TourFreshness {
        if self.old_head().is_some() {
            TourFreshness::Behind
        } else {
            TourFreshness::Current
        }
    }

    pub fn can_comment_now(&self) -> bool {
        self.comment_desk.can_comment_now()
    }

    /// A reply in a local thread is a follow-up task, so it always reaches the agent too.
    pub fn thread_action_request(&self, action: ThreadAction) -> ThreadActionRequest {
        let (action, reply_body, send_refusal) = match action {
            ThreadAction::Reply {
                thread_id,
                mode,
                audience,
            } => {
                let reply_body = self.state.reply_draft(&thread_id).map(str::to_owned);
                let audience = if mode.is_task() {
                    Audience::Agent
                } else {
                    audience
                };
                let action = ThreadAction::Reply {
                    thread_id,
                    mode,
                    audience,
                };
                (action, reply_body, self.send_refusal(mode))
            }
            other => (other, None, None),
        };
        let mut request = self.comment_desk.thread_action_request(
            (self.thread_home(), &self.head),
            &self.pulled_threads,
            action,
            reply_body,
            self.tour_freshness(),
        );
        if send_refusal.is_some() {
            request.refusal = send_refusal;
        }
        request
    }

    /// Asks the reviewer to confirm deleting one of their own comments, which a deletion needs.
    pub fn ask_to_delete(&mut self, comment_id: &CommentId) {
        self.comment_desk
            .ask_to_delete(&self.pulled_threads, comment_id);
    }

    /// The reviewer said no, or looked away.
    pub fn withdraw_delete(&mut self) {
        self.comment_desk.withdraw_delete();
    }

    /// The text in the box as typed so far; `None` while it is not open.
    pub fn text_of(&self, text_box: &TextBox) -> Option<&str> {
        self.comment_desk.text_of(&self.state, text_box)
    }

    /// Opens a reply box, or the edit of one of the viewer's own comments on its current text;
    /// nothing opens for a comment someone else wrote.
    pub fn open_box(&mut self, text_box: &TextBox) -> anyhow::Result<()> {
        self.comment_desk
            .open(&mut self.state, &self.pulled_threads, text_box)
    }

    /// For every key: changes the text in memory only. `typing_save` takes it to disk, off the UI thread.
    pub fn type_in(&mut self, text_box: &TextBox, text: &str) -> anyhow::Result<()> {
        self.comment_desk.type_in(&mut self.state, text_box, text)
    }

    /// The reviewer leaves the box (Escape): a box with nothing written goes, a written one stays.
    pub fn leave(&mut self, text_box: &TextBox) -> anyhow::Result<()> {
        self.comment_desk.leave(&mut self.state, text_box)
    }

    pub fn discard(&mut self, text_box: &TextBox) -> anyhow::Result<()> {
        self.comment_desk.discard(&mut self.state, text_box)
    }

    /// Whatever came of it, the next hand-off may go now.
    pub fn apply_thread_action(&mut self, outcome: ThreadActionOutcome) -> ThreadActionResult {
        self.work.want(WorkKind::DeliverHandOff);
        self.comment_desk
            .apply_thread_action(&mut self.state, &outcome, self.screen.sent_from());
        self.show_pulled_threads(outcome.threads.as_ref());
        if outcome.result == ThreadActionResult::Done {
            match &outcome.action {
                ThreadAction::Resolve { thread_id } => self.fold_thread(thread_id, true),
                ThreadAction::Unresolve { thread_id } => self.fold_thread(thread_id, false),
                _ => {}
            }
        }
        outcome.result
    }

    fn fold_thread(&mut self, thread_id: &ThreadId, folded: bool) {
        let Some(thread) = self
            .pulled_threads
            .iter()
            .find(|thread| &thread.id == thread_id)
        else {
            return;
        };
        if thread.is_folded_by_default() == folded {
            self.thread_fold_flipped.remove(thread_id);
        } else {
            self.thread_fold_flipped.insert(thread_id.clone());
        }
    }

    fn show_pulled_threads(&mut self, threads: Option<&Result<Vec<ReviewThread>, String>>) {
        match threads {
            Some(Ok(threads)) => self.set_threads(threads),
            Some(Err(reason)) => self.raise_problem(Problem::ThreadsNotRead(reason.clone())),
            None => {}
        }
    }

    pub fn viewer_authorship_is(&mut self, authored: bool) {
        self.clear_problem(ProblemKind::NotRead(Read::Authorship));
        self.github_answered(Affected::Authorship);
        self.comment_desk.viewer_authorship_is(authored);
        self.reviewing_mark.viewer_authorship_is(authored);
        self.want_reviewing_mark_sent();
    }

    /// GitHub refuses both on the viewer's own PR, so the buttons show only when the PR is surely someone else's.
    /// A local review gives no verdict.
    pub fn can_approve_or_request_changes(&self) -> bool {
        self.finishes_with_send_tasks().is_none()
            && self.comment_desk.can_approve_or_request_changes()
    }

    /// On the viewer's own PR there is no verdict to give, so finishing only makes sense with comments
    /// to send; a local review's finish sends its queued tasks, so it needs some.
    pub fn can_finish_review(&self) -> bool {
        match self.finishes_with_send_tasks() {
            Some(queued) => queued > 0,
            None => self.comment_desk.can_finish_review(),
        }
    }

    /// What finishing the review does now; `None` when there is nothing to finish.
    pub fn finish_action(&self) -> Option<FinishAction> {
        self.can_finish_review().then(|| {
            if self.finishes_with_send_tasks().is_some() {
                FinishAction::SendTasks
            } else {
                FinishAction::FinishReview
            }
        })
    }

    /// A local review finishes with Send tasks, never with a review on GitHub: how many tasks it
    /// would send. `None` for a PR.
    pub fn finishes_with_send_tasks(&self) -> Option<usize> {
        self.subject
            .comments_are_agent_tasks()
            .then(|| self.queued_tasks())
    }

    /// What the reviewer has not looked at: steps with no read tick and the pieces of the safety net.
    pub fn approve_warning(&self) -> Option<ApproveWarning> {
        if self.finishes_with_send_tasks().is_some() {
            return None;
        }
        let progress = self.progress();
        let unread_safety_net_pieces = self.tour.guided().map_or(0, |guided| {
            coverage::safety_net(guided, &self.files)
                .iter()
                .map(|part| part.piece.id.to_string())
                .collect::<BTreeSet<_>>()
                .len()
        });
        let unread = progress.total - progress.read;
        let warning = match &self.tour {
            Tour::Guided(_) => ApproveWarning {
                unread_steps: unread,
                unread_files: 0,
                unread_safety_net_pieces,
            },
            Tour::Basic(_) => ApproveWarning {
                unread_steps: 0,
                unread_files: unread,
                unread_safety_net_pieces,
            },
        };
        (warning.unread_steps + warning.unread_files + warning.unread_safety_net_pieces > 0)
            .then_some(warning)
    }

    pub fn finish_review_request(
        &self,
        verdict: ReviewVerdict,
        body: &str,
    ) -> Result<FinishRequest, FinishRefusal> {
        let Some(pr) = self.subject.pull_request() else {
            return Err(FinishRefusal::NoPullRequest);
        };
        self.comment_desk.finish_review_request(pr, verdict, body)
    }

    /// A sent review is read back, so the finish button shows it.
    pub fn apply_finish(&mut self, outcome: FinishOutcome) -> FinishResult {
        let result = self.comment_desk.apply_finish(&mut self.state, outcome);
        if result == FinishResult::Sent {
            self.work.want(WorkKind::PullLastReview);
        }
        result
    }

    pub fn begin_selection(&mut self, row: LineLocation) {
        self.comment_desk.begin_selection(row);
    }

    pub fn extend_selection(&mut self, row: LineLocation) {
        self.comment_desk.extend_selection(row);
    }

    pub fn is_selecting(&self) -> bool {
        self.comment_desk.row_selection().is_some()
    }

    /// The drag in progress, for a view that knows its rows and draws them without building the page.
    pub fn row_selection(&self) -> Option<&RowSelection> {
        self.comment_desk.row_selection()
    }

    pub fn cancel_selection(&mut self) {
        self.comment_desk.cancel_selection();
    }

    /// The drag ends: when the rows of the page can take a comment, a comment box opens on them.
    fn finish_selection(&mut self, page: &Page) -> Result<Option<CommentPlace>, CommentRefusal> {
        let freshness = self.tour_freshness();
        self.comment_desk
            .finish_selection(&mut self.state, page.lines(), freshness, &self.head)
    }

    /// The comment box of a picture opens on the whole file, under the same rules as on lines.
    fn begin_file_comment(&mut self, path: &FilePath) -> Result<CommentPlace, CommentRefusal> {
        let freshness = self.tour_freshness();
        self.comment_desk
            .begin_file_comment(&mut self.state, path, freshness, &self.head)
    }

    pub fn queued_comments(&self) -> &[QueuedComment] {
        self.state.comment_queue()
    }

    /// The save of what was typed since the last save, to run in the background; `None` when nothing is unsaved.
    pub fn typing_save(&mut self) -> Option<TypingSave> {
        self.state.typing_save()
    }

    /// "Suggest change" shows only when the range starts and ends on lines of the head.
    pub fn can_suggest_change(&self, place: &CommentPlace) -> bool {
        CommentDesk::can_suggest_change(place)
    }

    /// The comment boxes open on this review, each with what it is doing; each hangs under the
    /// last line of its place, or under the picture of its file.
    pub fn comment_boxes(&self) -> impl Iterator<Item = CommentBox<'_>> {
        let agent_out_of_reach = SendMode::LOCAL
            .iter()
            .find_map(|mode| self.send_refusal(*mode))
            .filter(|refusal| *refusal == CommentRefusal::AgentLinkOff);
        self.comment_desk
            .comment_boxes(&self.state, self.tour_freshness())
            .map(move |mut comment_box| {
                if comment_box.problem.is_none() {
                    comment_box.problem = agent_out_of_reach.clone().map(CommentProblem::Refused);
                }
                comment_box
            })
    }

    pub fn comment_box(&self, place: &CommentPlace) -> Option<CommentBox<'_>> {
        self.comment_boxes()
            .find(|comment_box| &comment_box.draft.place == place)
    }

    /// The comment box that takes the keys.
    pub fn composing(&self) -> Option<&CommentPlace> {
        self.comment_desk.composing()
    }

    /// The box shows as sending until `apply_comment` takes the outcome. `None` when there is no such draft.
    pub fn start_sending(
        &mut self,
        place: &CommentPlace,
        mode: SendMode,
        audience: Audience,
    ) -> Option<CommentRequest> {
        if self.send_refusal(mode).is_some() {
            return None;
        }
        let audience = if mode.is_task() {
            Audience::Agent
        } else {
            audience
        };
        let home = self.thread_home();
        self.comment_desk
            .start_sending(&self.state, place, mode, audience, (&self.head, home))
    }

    pub fn hand_offs(&self) -> &HandOffs {
        self.state.hand_offs()
    }

    /// How many tasks wait for Send tasks.
    pub fn queued_tasks(&self) -> usize {
        self.state.hand_offs().waiting_for_send_tasks().count()
    }

    /// The finish of a local review: every queued task goes to the agent in one message, in
    /// reading order, each with its own hand-off state.
    pub fn send_tasks(&mut self) -> anyhow::Result<()> {
        let order = self.reading_order();
        let threads = self.pulled_threads.clone();
        let reading_place = |task: &HandOff, thread: &ReviewThread| {
            let screen = task.step().map_or_else(
                || Screen::File(thread.path.clone()),
                |step| Screen::Step(step.clone()),
            );
            let nth = order.iter().position(|shown| *shown == screen);
            (nth.is_none(), nth, thread.original_line)
        };
        self.record_hand_offs(|hand_offs| hand_offs.send_queued_tasks(&threads, reading_place))?;
        self.work.want(WorkKind::DeliverHandOff);
        Ok(())
    }

    /// What the tour agent, GitHub or the reviewer did to the hand-offs. It is made even while a
    /// re-pin runs, so a hand-off is not delivered twice; only its save is refused then.
    fn record_hand_offs<T>(
        &mut self,
        change: impl FnOnce(&mut HandOffs) -> T,
    ) -> anyhow::Result<T> {
        self.state.record(|state| change(state.hand_offs_mut()))
    }

    /// The oldest comment in the queue, to send again.
    pub fn queued_comment_request(&self) -> Option<CommentRequest> {
        CommentDesk::queued_comment_request(&self.state, &self.head, self.thread_home())
    }

    /// The window was asked for another PR and could not open it; named until another switch succeeds.
    pub fn switch_failed(&mut self, reason: String) {
        self.raise_problem(Problem::SwitchFailed(reason));
    }

    pub fn state_file(&self) -> ReviewStateFile {
        self.state.file().clone()
    }

    /// A comment sent from its box. Whatever came of it, the next hand-off may go now.
    pub fn apply_comment(&mut self, outcome: CommentOutcome) -> anyhow::Result<CommentResult> {
        let result = self.land_comment(outcome);
        self.work.want(WorkKind::DeliverHandOff);
        result
    }

    fn land_comment(&mut self, outcome: CommentOutcome) -> anyhow::Result<CommentResult> {
        let threads = outcome.threads.clone();
        let hand_offs_before = self.state.hand_offs().all().len();
        let code = self.code_commented_on(&outcome);
        let result = self.comment_desk.apply_comment(
            &mut self.state,
            outcome,
            (self.screen.sent_from(), code),
        );
        self.note_new_hand_off(hand_offs_before);
        self.show_pulled_threads(threads.as_ref());
        result
    }

    /// The lines a task of a local review is about, as the page shows them; a PR's hand-off has none.
    fn code_commented_on(&self, outcome: &CommentOutcome) -> Option<String> {
        self.subject.local()?;
        let place = &outcome.request.draft.place;
        let selected = Page::of(self).selected(&RowSelection::of(place)?)?;
        Some(ask_context::code_lines(selected.lines()))
    }

    fn note_new_hand_off(&mut self, hand_offs_before: usize) {
        let Some(newest) = self
            .state
            .hand_offs()
            .all()
            .get(hand_offs_before..)
            .and_then(<[_]>::last)
        else {
            return;
        };
        match (newest.thread(), newest.state()) {
            (None, HandOffState::Failed(reason)) => {
                let comment = newest.comment().chars().take(60).collect();
                let reason = reason.clone();
                self.raise_problem(Problem::HandOffNotSent { comment, reason });
            }
            _ => self.clear_problem(ProblemKind::HandOffNotSent),
        }
    }

    /// Opens code links and the links of the tour's text at the commits this review reads.
    pub fn code_opener(&self, ide: Result<Ide, NoIde>) -> CodeOpener {
        CodeOpener::new(ide, self.working_copy, &self.code_host, &self.head)
    }

    /// Asked of the clone on open and on each focus, not on each click.
    pub fn working_copy_is(&mut self, working_copy: WorkingCopy) {
        self.working_copy = working_copy;
    }

    fn links(&self) -> CodeLinks<'_> {
        CodeLinks {
            host: &self.code_host,
            base: &self.base,
            head: &self.head,
        }
    }

    /// Ids the tour does not hold are ignored: the sidebar only offers real steps.
    pub fn go_to(&mut self, id: &StepId) {
        self.go_to_screen(Screen::Step(id.clone()));
    }

    /// The cmd+k list for what the reviewer has typed so far.
    pub fn jump_targets(&self, query: &str) -> Vec<JumpTarget> {
        let Some(guided) = self.tour.guided() else {
            return jump::rank(jump::basic_targets(&self.file_tree.reading_order()), query);
        };
        let safety_net_files: Vec<&FilePath> = if self.reading_order().contains(&Screen::SafetyNet)
        {
            coverage::safety_net(guided, &self.files)
                .iter()
                .map(|part| part.piece.id.path())
                .collect()
        } else {
            Vec::new()
        };
        jump::rank(
            jump::all_targets(guided, &self.files, &safety_net_files),
            query,
        )
    }

    /// The cmd+k palette for what the reviewer has typed so far: places by kind, then actions.
    pub(super) fn palette_with(
        &self,
        query: &str,
        build: Build,
        page_toggles: Vec<PaletteAction>,
    ) -> Vec<PaletteGroup> {
        jump::palette(
            self.jump_targets(""),
            &self.palette_actions_with(build, page_toggles),
            query,
        )
    }

    /// What the palette can do on the open screen.
    /// Checking for updates only where the build takes them.
    /// `page_toggles` are the actions on the screen's boxes and the focused change's file, which only the page knows.
    pub(super) fn palette_actions_with(
        &self,
        build: Build,
        page_toggles: Vec<PaletteAction>,
    ) -> Vec<PaletteAction> {
        let step = match &self.screen {
            Screen::Step(id) => Some(id),
            Screen::Summary | Screen::Overview | Screen::File(_) | Screen::SafetyNet => None,
        };
        let tick = match &self.screen {
            Screen::Step(id) if self.is_ticked(id) => Some(PaletteAction::MarkStepUnread),
            Screen::Step(_) => Some(PaletteAction::MarkStepRead),
            Screen::File(path) if self.is_file_ticked(path) => Some(PaletteAction::MarkFileUnread),
            Screen::File(_) => Some(PaletteAction::MarkFileRead),
            Screen::Summary | Screen::Overview | Screen::SafetyNet => None,
        };
        let wrap = if self.display_options.wraps_lines() {
            PaletteAction::StopWrappingLines
        } else {
            PaletteAction::WrapLines
        };
        let whitespace = if self.display_options.ignores_whitespace() {
            PaletteAction::ShowWhitespace
        } else {
            PaletteAction::HideWhitespace
        };
        let next_unread = (self.first_unread_step().is_some()
            || self.first_unread_file().is_some())
        .then_some(PaletteAction::NextUnreadStep);
        let ask = step.map(|_| PaletteAction::AskAboutStep);
        let finish = self.finish_action().map(|finish| match finish {
            FinishAction::FinishReview => PaletteAction::FinishReview,
            FinishAction::SendTasks => PaletteAction::SendTasks,
        });
        let check_for_updates = build
            .takes_updates()
            .then_some(PaletteAction::CheckForUpdates);
        next_unread
            .into_iter()
            .chain(tick)
            .chain(ask)
            .chain(self.guided_tour_palette_actions())
            .chain([wrap, whitespace])
            .chain(page_toggles)
            .chain(finish)
            .chain(
                self.pr_controls
                    .iter()
                    .flat_map(PrControls::palette_actions),
            )
            .chain(self.can_open_pr().then_some(PaletteAction::OpenPr))
            .chain((!self.related_issues().is_empty()).then_some(PaletteAction::OpenIssue))
            .chain(self.subject.pull_request().into_iter().flat_map(|_| {
                [
                    PaletteAction::OpenOnGitHub,
                    PaletteAction::OpenChecksOnGitHub,
                ]
            }))
            .chain([
                PaletteAction::SwitchPr,
                PaletteAction::Settings,
                PaletteAction::Shortcuts,
            ])
            .chain(check_for_updates)
            .chain([PaletteAction::RestartApp])
            .collect()
    }

    /// A folded step opens for the session so the file or symbol the reviewer asked for is on screen.
    pub fn jump_to(&mut self, target: &JumpTarget) {
        self.go_to_screen(target.screen.clone());
        if let Screen::Step(id) = &target.screen
            && self.is_folded(id)
        {
            self.toggle_fold(id);
        }
    }

    pub fn go_to_summary(&mut self) {
        self.go_to_screen(Screen::Summary);
    }

    pub fn go_to_overview(&mut self) {
        self.go_to_screen(Screen::Overview);
    }

    /// Ids of files the tour does not hold are ignored, like ids of steps.
    pub fn go_to_file(&mut self, path: &FilePath) {
        self.go_to_screen(Screen::File(path.clone()));
    }

    pub fn go_to_safety_net(&mut self) {
        self.go_to_screen(Screen::SafetyNet);
    }

    fn go_to_screen(&mut self, screen: Screen) {
        if self.reading_order().contains(&screen) {
            self.screen = screen;
        }
    }

    /// False when there is no screen after this one.
    pub fn go_forward(&mut self) -> bool {
        self.step_by(1)
    }

    /// For the scroll script, which scrolls through every piece, so a folded step is opened.
    pub fn go_forward_unfolded(&mut self) -> bool {
        if !self.go_forward() {
            return false;
        }
        if let Screen::Step(id) = self.screen.clone()
            && self.is_folded(&id)
        {
            self.toggle_fold(&id);
        }
        true
    }

    /// The Next button: on to the next screen that is not a step already read, which the sidebar
    /// still reaches. False when there is none.
    pub fn go_on(&mut self) -> bool {
        match self.onward_screen() {
            Some(screen) => {
                self.screen = screen;
                true
            }
            None => false,
        }
    }

    fn onward_screen(&self) -> Option<Screen> {
        let reading_order = self.reading_order();
        let here = reading_order
            .iter()
            .position(|screen| screen == &self.screen)?;
        reading_order
            .into_iter()
            .skip(here + 1)
            .find(|screen| !self.is_read_screen(screen))
    }

    fn is_read_screen(&self, screen: &Screen) -> bool {
        match screen {
            Screen::Step(id) => self.is_ticked(id),
            Screen::File(path) => self.is_file_ticked(path),
            Screen::Summary | Screen::Overview | Screen::SafetyNet => false,
        }
    }

    fn is_unread_screen(&self, screen: &Screen) -> bool {
        match screen {
            Screen::Step(id) => !self.is_ticked(id),
            Screen::File(path) => !self.is_file_ticked(path),
            Screen::Summary | Screen::Overview | Screen::SafetyNet => false,
        }
    }

    /// False on the summary, the first screen.
    pub fn go_back(&mut self) -> bool {
        self.step_by(-1)
    }

    /// The first step after the open screen that is not read yet, starting over from the top.
    /// False when every step is read.
    pub fn go_to_next_unread(&mut self) -> bool {
        let reading_order = self.reading_order();
        let here = reading_order
            .iter()
            .position(|screen| screen == &self.screen)
            .unwrap_or(0);
        let after = reading_order.iter().skip(here + 1);
        let next = after
            .chain(reading_order.iter().take(here + 1))
            .find(|screen| self.is_unread_screen(screen))
            .cloned();
        match next {
            Some(screen) if screen != self.screen => {
                self.screen = screen;
                true
            }
            _ => false,
        }
    }

    pub fn tour_lead(&self) -> TourLead {
        let progress = self.progress();
        if progress.total == 0 {
            return TourLead::Nowhere;
        }
        let finish = self.finish_action();
        if let Tour::Basic(_) = &self.tour {
            return match self.first_unread_file() {
                _ if progress.is_complete() => TourLead::FinishFiles {
                    files: progress.total,
                    finish,
                },
                Some(first) if progress.read == 0 => TourLead::StartFile(first),
                Some(next) => TourLead::ContinueFile(next),
                None => TourLead::Nowhere,
            };
        }
        if progress.is_complete() {
            return TourLead::Finish {
                steps: progress.total,
                finish,
            };
        }
        match self.first_unread_step() {
            Some(first) if progress.read == 0 => TourLead::Start(first),
            Some(next) => TourLead::Continue(next),
            None => TourLead::Nowhere,
        }
    }

    pub fn route(&self) -> Vec<RouteStop> {
        fn count(steps: &[Step]) -> usize {
            steps.iter().map(|step| 1 + count(&step.children)).sum()
        }
        let steps = self.tour.guided().map_or(&[][..], |guided| &guided.steps);
        steps
            .iter()
            .map(|step| RouteStop {
                id: step.id.clone(),
                title: step.title.clone(),
                why: step.why.clone(),
                details: count(&step.children),
                is_read: self.is_ticked(&step.id),
                has_check: step.check.is_some(),
            })
            .collect()
    }

    pub fn summary_stats(&self) -> SummaryStats {
        SummaryStats {
            steps: self.progress().total,
            files: self.files.len(),
            added_lines: self.files.iter().map(|file| file.added).sum(),
            removed_lines: self.files.iter().map(|file| file.removed).sum(),
            open_threads: self
                .pulled_threads
                .iter()
                .filter(|thread| !thread.is_resolved)
                .count(),
        }
    }

    /// The first step in reading order that is not read yet.
    pub fn first_unread_step(&self) -> Option<UnreadStep> {
        self.steps_in_reading_order.iter().find_map(|screen| {
            let Screen::Step(id) = screen else {
                return None;
            };
            (!self.is_ticked(id)).then(|| UnreadStep {
                id: id.clone(),
                title: self.step(id).map_or_default(|step| step.title.clone()),
            })
        })
    }

    pub fn is_file_ticked(&self, path: &FilePath) -> bool {
        self.state.file_read_tick(path).is_some()
    }

    /// The file on screen, if it is not read yet: the footer ticks it on the way on.
    pub fn file_to_tick_on_screen(&self) -> Option<FilePath> {
        match &self.screen {
            Screen::File(path) if !self.is_file_ticked(path) => Some(path.clone()),
            _ => None,
        }
    }

    pub fn is_read(&self, subject: &ReadSubject) -> bool {
        match subject {
            ReadSubject::Step(id) => self.is_ticked(id),
            ReadSubject::File(path) => self.is_file_ticked(path),
        }
    }

    /// The first file in reading order that is not read yet; none in a guided tour.
    pub fn first_unread_file(&self) -> Option<UnreadFile> {
        self.steps_in_reading_order.iter().find_map(|screen| {
            let Screen::File(path) = screen else {
                return None;
            };
            (!self.is_file_ticked(path)).then(|| UnreadFile { path: path.clone() })
        })
    }

    /// Only a PR has viewed marks on GitHub to keep in step with the ticks.
    fn want_viewed_synced(&mut self) {
        if self.subject.syncs_viewed_marks() {
            self.work.want_in(WorkKind::SyncViewed, PUSH_DEBOUNCE);
        }
    }

    /// Only files of this tour can be read; a file the tour does not hold is ignored. Viewed
    /// marks follow it one to one.
    pub fn tick_file(&mut self, path: &FilePath) -> anyhow::Result<()> {
        self.want_viewed_synced();
        if !self.reading_order().contains(&Screen::File(path.clone())) {
            return Ok(());
        }
        let coverage = self.coverage();
        let subject = ReadSubject::File(path.clone());
        self.state.change(|state| {
            state.tick_file(path, &self.head);
            self.viewed_sync.wish_after_tick(state, &coverage, &subject);
        })
    }

    pub fn untick_file(&mut self, path: &FilePath) -> anyhow::Result<()> {
        self.want_viewed_synced();
        let coverage = self.coverage();
        let subject = ReadSubject::File(path.clone());
        self.state.change(|state| {
            state.untick_file(path);
            self.viewed_sync
                .wish_after_untick(state, &coverage, &subject);
        })
    }

    pub fn is_ticked(&self, id: &StepId) -> bool {
        self.state.read_tick(id).is_some()
    }

    /// The tick is kept in memory even when saving fails, so the reviewer loses nothing this session.
    /// Viewed marks sync once no other tick or untick came for `PUSH_DEBOUNCE`.
    pub fn tick(&mut self, id: &StepId) -> anyhow::Result<()> {
        self.want_viewed_synced();
        if !self.reading_order().contains(&Screen::Step(id.clone())) {
            return Ok(());
        }
        let coverage = self.coverage();
        let subject = ReadSubject::Step(id.clone());
        self.state.change(|state| {
            state.tick(id, &self.head);
            self.fold_flipped.remove(id);
            self.viewed_sync.wish_after_tick(state, &coverage, &subject);
        })
    }

    pub fn untick(&mut self, id: &StepId) -> anyhow::Result<()> {
        self.want_viewed_synced();
        let coverage = self.coverage();
        let subject = ReadSubject::Step(id.clone());
        self.state.change(|state| {
            state.untick(id);
            self.fold_flipped.remove(id);
            self.viewed_sync
                .wish_after_untick(state, &coverage, &subject);
        })
    }

    /// Skim, ticked and gone steps fold; the reviewer can flip that for the session without losing the tick.
    pub fn is_folded(&self, id: &StepId) -> bool {
        let folds_by_default = self.is_ticked(id)
            || self
                .step(id)
                .is_some_and(|step| step.skim || self.is_gone(step));
        folds_by_default != self.fold_flipped.contains(id)
    }

    /// Opens or folds every file box of the step at once.
    pub fn toggle_fold(&mut self, id: &StepId) {
        if !self.fold_flipped.remove(id) {
            self.fold_flipped.insert(id.clone());
        }
        let step = Screen::Step(id.clone());
        self.box_fold_flipped.retain(|(screen, _)| *screen != step);
    }

    /// A folded step still lists its file boxes, each folded on its own. The box of a file screen
    /// starts open.
    pub fn is_box_folded(&self, screen: &Screen, path: &FilePath) -> bool {
        let folds_by_default = match screen {
            Screen::Step(id) => self.is_folded(id),
            Screen::Summary | Screen::Overview | Screen::File(_) | Screen::SafetyNet => false,
        };
        folds_by_default
            != self
                .box_fold_flipped
                .contains(&(screen.clone(), path.clone()))
    }

    /// Folds or opens each of the boxes, leaving one already as asked alone.
    pub fn set_boxes_folded(&mut self, screen: &Screen, paths: &[FilePath], folded: bool) {
        for path in paths {
            if self.is_box_folded(screen, path) != folded {
                self.toggle_box_fold(screen, path);
            }
        }
    }

    pub fn toggle_box_fold(&mut self, screen: &Screen, path: &FilePath) {
        let key = (screen.clone(), path.clone());
        if !self.box_fold_flipped.remove(&key) {
            self.box_fold_flipped.insert(key);
        }
    }

    pub fn read_unit(&self) -> ReadUnit {
        match &self.tour {
            Tour::Guided(_) => ReadUnit::Steps,
            Tour::Basic(_) => ReadUnit::Files,
        }
    }

    /// Only steps of this tour count: ticks of steps a refresh removed stay in the file but not here.
    pub fn progress(&self) -> ReadProgress {
        let reading_order = self.reading_order();
        let subjects = reading_order.iter().filter_map(|screen| match screen {
            Screen::Step(id) => Some(ReadSubject::Step(id.clone())),
            Screen::File(path) => Some(ReadSubject::File(path.clone())),
            Screen::Summary | Screen::Overview | Screen::SafetyNet => None,
        });
        ReadProgress {
            read: subjects
                .clone()
                .filter(|subject| self.is_read(subject))
                .count(),
            total: subjects.count(),
        }
    }

    /// The banner text while viewed sync is off; ticks keep working without it.
    pub fn sync_banner(&self) -> Option<&str> {
        self.viewed_sync.banner()
    }

    /// Drives the "not synced" dot.
    pub fn not_synced(&self) -> bool {
        self.viewed_sync.not_synced(&self.state)
    }

    /// `None` for a local review: GitHub has no viewed marks of it.
    pub fn sync_request(&self) -> Option<SyncRequest> {
        let pr = self.subject.pull_request()?;
        Some(
            self.viewed_sync
                .request(&self.state, &self.coverage(), pr, &self.head),
        )
    }

    /// An error means something stays queued or unread; the next sync tries again.
    /// An answer for another head, e.g. from before a re-pin, is dropped: the next sync asks again.
    pub fn apply_sync(&mut self, outcome: SyncOutcome) -> anyhow::Result<()> {
        if self.viewed_sync.is_off() || !outcome.is_for(&self.head) {
            return Ok(());
        }
        let coverage = self.coverage();
        let mut error = None;
        let saved = self.state.record(|state| {
            let (unticked, sync_error) = self
                .viewed_sync
                .take_in(state, &coverage, &self.head, outcome);
            for subject in &unticked {
                if let ReadSubject::Step(step) = subject {
                    self.fold_flipped.remove(step);
                }
            }
            error = sync_error;
        });
        self.clear_problem(ProblemKind::ViewedNotSynced);
        match error {
            Some(GitHubError::NotAnswering(cause)) => {
                self.github_not_answering(cause, Affected::ViewedMarks);
                Err(GitHubError::NotAnswering(cause).into())
            }
            Some(error) => {
                self.github_answered(Affected::ViewedMarks);
                let settled = self.viewed_sync.settle(error);
                if let Err(error) = &settled {
                    self.raise_problem(Problem::ViewedNotSynced(error.root_cause().to_string()));
                }
                settled
            }
            None => {
                self.github_answered(Affected::ViewedMarks);
                saved
            }
        }
    }

    fn coverage(&self) -> Coverage {
        match &self.tour {
            Tour::Guided(guided) => Coverage::of(guided, &self.files),
            Tour::Basic(_) => Coverage::of_files(self.files.iter().map(|file| &file.path)),
        }
    }

    /// Changed code no page can show, by piece id: the reviewer has to read it on GitHub.
    pub fn changes_not_shown(&self) -> Vec<String> {
        let unplaced = self
            .files
            .iter()
            .flat_map(|file| file.not_shown.iter().cloned());
        let outside_their_file = self.files.iter().flat_map(|file| {
            file.pieces
                .iter()
                .filter(|piece| {
                    !self
                        .files
                        .iter()
                        .any(|other| other.path == *piece.id.path())
                })
                .map(|piece| piece.id.to_string())
        });
        unplaced.chain(outside_their_file).collect()
    }

    /// What went wrong that the reviewer must know about, until it is put right.
    pub fn problems(&self) -> Vec<Problem> {
        let not_saved = self.state.not_saved().map(Problem::NotSaved);
        let not_kept = self
            .state
            .refused_while_repinning()
            .then_some(Problem::ChangeNotKeptWhileRepinning);
        not_saved
            .into_iter()
            .chain(not_kept)
            .chain(self.problems.list().cloned())
            .collect()
    }

    pub fn reading_place(&self) -> ReadingPlace {
        let reading_order = self.reading_order();
        let position = reading_order
            .iter()
            .position(|screen| screen == &self.screen);
        let is_step = |screen: &&Screen| matches!(screen, Screen::Step(_));
        let is_file = |screen: &&Screen| matches!(screen, Screen::File(_));
        let next = self.onward_screen().and_then(|screen| match &screen {
            Screen::Step(id) => Some(NextScreen::Step {
                title: self.step(id)?.title.clone(),
            }),
            Screen::SafetyNet => Some(NextScreen::SafetyNet),
            Screen::File(path) => Some(NextScreen::File { path: path.clone() }),
            Screen::Summary | Screen::Overview => None,
        });
        ReadingPlace {
            step_number: match (&self.screen, position) {
                (Screen::Step(_), Some(position)) => Some(
                    reading_order
                        .iter()
                        .take(position + 1)
                        .filter(is_step)
                        .count(),
                ),
                _ => None,
            },
            step_count: reading_order.iter().filter(is_step).count(),
            file_number: match (&self.screen, position) {
                (Screen::File(_), Some(position)) => Some(
                    reading_order
                        .iter()
                        .take(position + 1)
                        .filter(is_file)
                        .count(),
                ),
                _ => None,
            },
            file_count: reading_order.iter().filter(is_file).count(),
            has_previous: position.is_some_and(|position| position > 0),
            next,
        }
    }

    fn step_by(&mut self, offset: isize) -> bool {
        let reading_order = self.reading_order();
        let next = reading_order
            .iter()
            .position(|screen| screen == &self.screen)
            .and_then(|position| position.checked_add_signed(offset))
            .and_then(|position| reading_order.get(position));
        match next {
            Some(screen) => {
                self.screen = screen.clone();
                true
            }
            None => false,
        }
    }
}

/// The head a basic tour's file was last read at: its tick, or the tick a re-pin took away.
fn read_head_of_file<'a>(state: &'a ReviewState, path: &FilePath) -> Option<&'a Sha> {
    state
        .file_read_tick(path)
        .or_else(|| state.file_read_before(path))
        .map(|tick| &tick.head)
}

/// The head a step was last read at: its tick, or the tick a re-pin took away.
fn read_head_of<'a>(state: &'a ReviewState, step: &StepId) -> Option<&'a Sha> {
    state
        .read_tick(step)
        .or_else(|| state.read_before(step))
        .map(|tick| &tick.head)
}
