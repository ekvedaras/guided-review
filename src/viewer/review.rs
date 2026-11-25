mod ask_why;
mod finding_actions;
mod finding_outcome;
#[cfg(feature = "snapshots")]
mod finding_staging;
mod guided_tour;

pub use ask_why::{DEFAULT_WHY_QUESTION, WhyAsked};
pub use finding_outcome::{FindingRefusal, FindingsBatch};
mod comments;
mod jobs;
mod kept;
mod names;
mod pr;
mod pr_controls;
mod reading;
mod run_ai_review;
mod stations;
mod work;

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt;
use std::sync::Arc;
use std::time::Instant;

use crate::code_source::CodeSource;
use crate::coverage::{self, PiecePart};
use crate::local_facts::{LocalChange, LocalFacts};
use crate::local_work_tree::OnDisk;
use crate::navigation::{FileSearch, Lookups};
use crate::pieces::{self, ChangedFile, PieceId};
use crate::pull_request::{ChangeTips, Sha};
use crate::review_subject::ReviewSubject;
use crate::tour::{BasicTour, GuidedTour, Step, Tour};

use std::path::{Path, PathBuf};

use crate::config::AskSettings;

use super::ai_review::AiReview;
use super::ai_review_run::{AiReviewRun, ReviewAgentLife};
use super::ask::{Ask, AskId, Asks, Place, ShownOn};
use super::ask_context;
use super::ask_desk::{AskDesk, AskProcesses};
use super::ask_target::{AskTarget, Unavailable};
use super::branch_link::BranchLink;
use super::carried::ReadCarrier;
use super::code_link::{CodeHost, CodeLinks, CodeOpener, WorkingCopy};
use super::comment_desk::{CommentDesk, CommentGesture};
use super::comment_send::CommentRequest;
use super::comments::Audience;
use super::comments::{CommentPlace, CommentRefusal, SendMode, Sending};
use super::diff_display::{DiffDisplay, FilesAtHead, FilesAtHeadRequest};
use super::diff_line::DiffLine;
use super::disk_news::DiskNews;
use super::display_options::DisplayOptions;
use super::file_block::FileBlock;
use super::file_tree::{FileTree, FolderPath, TreeRow};
use super::finding_placement::{FindingHome, FindingPlacement, FindingView};
use super::finish_review::FinishAction;
use super::github_outage::Read;
use super::github_outage::{Affected, OutageCause};
use super::guided_tour::GuidedTourProgress;
use super::hand_offs::{AgentReplied, HandOffId};
use super::highlight::{HighlightRequest, Highlights};
use super::image_preview::ImagePreviewRequest;
use super::jump::{self, JumpTarget, PaletteAction, PaletteGroup};
use super::local_thread_store::ThreadHome;
use super::markdown::Markdown;
use super::move_offer::MoveFrom;
use super::name_desk::NameDesk;
use super::outdated_lines::FollowedLines;
use super::overview::{OverviewFile, OverviewPage, OverviewStats};
use super::piece_diff::PieceDiff;
use super::ports::{AgentLaunch, GitHubError};
use super::pr_controls::PrControls;
use super::pr_notes::PrNotes;
use super::pr_origin::PullRequestOrigin;
use super::pr_reviewers::Standing;
use super::pr_reviewers::SubmittedReview;
use super::problems::{Problem, ProblemKind, Problems};
use super::refresh_brief::RefreshBrief;
use super::related_issues::RelatedIssues;
use super::replacing_jobs::{AfterJob, JobOutcome, ReplacingJob, ReplacingJobs, StartedJob};
use super::review_state::{RepinRecord, ReviewState, ReviewStateFile};
use super::review_thread::{ReviewThread, ThreadId};
use super::reviewing_mark::ReviewingMark;
use super::row_selection::{RowSelection, SelectedLines};
use super::saved_state::SavedState;
use super::step_page::{FilePage, SafetyNetPage, SinceRead, StepPage};
use super::thread_actions::TourFreshness;
use super::threads::{self, ThreadPlacement, ThreadWork};
use super::used_findings::FindingsOfThreads;
use super::viewed_sync::ViewedSync;
use super::work_agenda::WorkAgenda;
use crate::app_folders::AppFolders;
use crate::build::Build;
use crate::file_path::FilePath;
use crate::findings::{Finding, FindingId};
use crate::ide::{Ide, NoIde};
use crate::repin::RepinStatus;

pub use crate::tour::StepId;

use super::screen::{NextScreen, ReadSubject, Screen};

pub use kept::Review;

/// One row of the step tree. Rows come in reading order, so indenting by `depth` draws the tree.
pub struct SidebarEntry<'a> {
    pub id: StepId,
    pub title: &'a str,
    pub depth: usize,
    pub is_current: bool,
    pub is_ticked: bool,
    pub thread_count: usize,
    /// The open findings of the AI review on the step.
    pub finding_count: usize,
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

/// What the sidebar counts for the safety net, cheap enough to ask on every change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SafetyNetSize {
    pub pieces: usize,
    pub threads: usize,
    pub findings: usize,
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
    /// What GitHub last said of the PR's author and branches; `None` until it answers, and always for a local review.
    pr_origin: Option<PullRequestOrigin>,
    base: Sha,
    head: Sha,
    /// Unknown until GitHub answers.
    pr_head: Option<Sha>,
    /// A local review's folder against the snapshot it reads; unknown until the folder is looked at.
    on_disk: Option<OnDisk>,
    /// What GitHub last said of the viewer's sent review; kept as it was while a read fails.
    last_review: Option<SubmittedReview>,
    /// Where each reviewer stands, as last read.
    standings: Vec<Standing>,
    /// `None` for a local review, which has no PR to change.
    pr_controls: Option<PrControls>,
    files: Vec<ChangedFile>,
    /// Guided: summary, then parent, its children, the next parent (from the big picture to the
    /// details), then the safety net when it holds anything. Basic: overview, then the files in
    /// tree order.
    steps_in_reading_order: Vec<Screen>,
    has_uncovered_changes: bool,
    pulled_threads: Vec<ReviewThread>,
    /// A snapshot scene's threads, kept over the threads GitHub answers with while the scene shows.
    #[cfg(feature = "snapshots")]
    staged_threads: Vec<ReviewThread>,
    followed_lines: BTreeMap<ThreadId, FollowedLines>,
    threads: ThreadPlacement,
    findings: FindingPlacement,
    /// The finding the reviewer was last taken to, so the next one comes after it.
    finding_cursor: Option<FindingId>,
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
    /// The names hovered in the diff, looked up at the commits this review reads. Not saved.
    names: NameDesk,
    reviewing_mark: ReviewingMark,
    /// Agent replies taken while a re-pin ran, which the re-pinned review applies and saves.
    replies_after_repin: Vec<(HandOffId, AgentReplied)>,
    /// The last news of the tour agent said tour agents cannot be reached. Not saved.
    agent_link_is_off: bool,
    /// The review agent's findings, read from their file. Not saved: the file is the state.
    ai_review: AiReview,
    ai_review_run: AiReviewRun,
    /// Not saved: the app looks for the review agent when it opens the review.
    review_agent: ReviewAgentLife,
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
        let page = Arc::clone(self.page());
        let Some(rows) = RowSelection::of(place).and_then(|lines| page.selected(&lines)) else {
            return Ok(None);
        };
        self.suggest_lines(place, rows.lines())
    }

    /// A review reopened after a re-pin goes on with what the one it replaces was doing; see
    /// `ReviewCore::carry_on_from`.
    pub fn carry_on_from(&mut self, replaced: Self) {
        ReviewCore::carry_on_from(self, replaced.into_core());
    }

    /// A job's outcome carries the review built to replace this one, so the review, not its core,
    /// lands it.
    pub fn land_job(&mut self, outcome: JobOutcome) -> AfterJob {
        let Some(started) = self.landed_job(outcome.job()) else {
            return AfterJob::Nothing;
        };
        let stays = AfterJob::Stays { toast: None };
        match outcome {
            JobOutcome::Repin(Ok(replacement))
            | JobOutcome::Refresh(Ok(replacement))
            | JobOutcome::Reload(Ok(replacement)) => AfterJob::Reopen {
                replacement: Box::new(replacement),
                screen: self.screen().clone(),
            },
            JobOutcome::SwitchToGuided(Ok(replacement)) => AfterJob::Reopen {
                replacement: Box::new(replacement),
                screen: Screen::Summary,
            },
            JobOutcome::Repin(Err(error)) => {
                self.writes_here_again();
                self.job_failed(ReplacingJob::Repin, &error, None);
                stays
            }
            JobOutcome::Refresh(Err(error)) => {
                self.job_failed(
                    ReplacingJob::Refresh,
                    &error,
                    Some(Problem::TourNotRefreshed(format!("{error:#}"))),
                );
                stays
            }
            JobOutcome::Reload(Err(error)) => {
                self.job_failed(
                    ReplacingJob::Reload,
                    &error,
                    Some(Problem::TourNotReloaded(format!("{error:#}"))),
                );
                stays
            }
            JobOutcome::SwitchToGuided(Err(error)) => {
                self.job_failed(ReplacingJob::SwitchToGuided, &error, None);
                AfterJob::Stays {
                    toast: Some(format!("Could not switch to the guided tour: {error:#}")),
                }
            }
            JobOutcome::MoveToPr(moved) => {
                let StartedJob::MoveToPr(started) = started else {
                    return AfterJob::Nothing;
                };
                match moved {
                    Ok(()) => {
                        self.job_ended_the_review();
                        match started.from {
                            MoveFrom::TheLocalReview => AfterJob::ShowPr {
                                moved_away: self.subject().clone(),
                                pr: started.offer.pr,
                            },
                            MoveFrom::ThePr => AfterJob::DropAndReopen(started.change_set.into()),
                        }
                    }
                    Err(error) => {
                        self.writes_here_again();
                        self.job_failed(ReplacingJob::MoveToPr, &error, None);
                        AfterJob::Stays {
                            toast: Some(started.failure_text(&error)),
                        }
                    }
                }
            }
            JobOutcome::DropReview(Ok(())) => {
                self.job_ended_the_review();
                AfterJob::Dropped(self.subject().clone())
            }
            JobOutcome::DropReview(Err(error)) => {
                self.writes_here_again();
                self.job_failed(
                    ReplacingJob::DropReview,
                    &error,
                    Some(Problem::ReviewNotDropped(format!("{error:#}"))),
                );
                stays
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
        let clone = self.asks().clone_folder().cloned();
        let target = self.ask_target(place, label, clone, settings);
        let head = self.head().clone();
        self.check_asks_for_code_changes();
        self.change_asks(|desk| desk.open(target, &head, now, processes));
    }

    fn check_asks_for_code_changes(&mut self) {
        let changed = self.asks_whose_code_changed();
        self.change_asks(|desk| desk.code_changed_is(changed));
    }
}

impl ReviewCore {
    /// A review reopened after a re-pin goes on with what the one it replaces was doing: what its
    /// comment boxes were doing (which takes the keys, which are sending and why a send did not
    /// go), its work, whose outcomes still land here, its branch link and PR controls, which those
    /// outcomes filled, and which screen's end the window saw. What the config and the disk say is
    /// `ReviewBuilder`'s, which built this review.
    fn carry_on_from(&mut self, replaced: Self) {
        let switched_to_guided =
            matches!(replaced.tour, Tour::Basic(_)) && matches!(self.tour, Tour::Guided(_));
        self.comment_desk.keep_boxes_of(replaced.comment_desk);
        self.work = replaced.work;
        self.jobs = replaced.jobs;
        self.branch_link = replaced.branch_link;
        self.related_issues.carry_on_from(replaced.related_issues);
        self.pr_origin = replaced.pr_origin;
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
        let changed = self.asks_whose_code_changed();
        self.ask_desk.code_changed_is(changed);
        for (id, replied) in replaced.replies_after_repin {
            if let Err(error) = self.hand_off_replied(id, &replied) {
                eprintln!("could not carry the reply to {id} into the re-pinned review: {error:#}");
            }
        }
    }

    fn suggest_lines(
        &mut self,
        place: &CommentPlace,
        lines: &[DiffLine],
    ) -> anyhow::Result<Option<String>> {
        CommentDesk::suggest_change(&mut self.state, place, lines)
    }

    fn asks_whose_code_changed(&self) -> HashSet<AskId> {
        self.ask_desk.ids_where(|ask| self.code_changed_since(ask))
    }

    /// This review replaces the basic review of its subject: its steps start with the read ticks
    /// carried from that review's files, and `carrier` carries the files it reads until it hands
    /// its whole state over.
    pub(in crate::viewer) fn switched_from_basic(&mut self, carrier: ReadCarrier) {
        self.steps_started_read = Some(self.progress().read);
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
                let shown = super::piece_diff::pieces_of(step, guided, &self.files);
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
}

impl ReviewCore {
    fn open(
        tour: Tour,
        source: &dyn CodeSource,
        state_file: ReviewStateFile,
    ) -> anyhow::Result<Self> {
        let (mut state, saved_review_kept_as) = state_file.load_or_set_aside()?;
        let parts_not_read = state.parts_not_read();
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
        let mut reviewing_mark = ReviewingMark::new(head.clone());
        if state.is_done_at(&head) {
            reviewing_mark.done();
        }
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
        let names = NameDesk::new(Arc::new(Lookups::new(FileSearch::for_source(
            source,
            files
                .iter()
                .flat_map(|file| [Some(&file.path), file.old_path.as_ref()])
                .flatten()
                .cloned()
                .collect(),
        ))));
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
            pr_origin: None,
            base,
            head,
            pr_head: None,
            on_disk: None,
            last_review: None,
            standings: Vec::new(),
            pr_controls,
            files,
            steps_in_reading_order,
            has_uncovered_changes,
            pulled_threads: Vec::new(),
            #[cfg(feature = "snapshots")]
            staged_threads: Vec::new(),
            followed_lines: BTreeMap::new(),
            threads: ThreadPlacement::default(),
            findings: FindingPlacement::default(),
            finding_cursor: None,
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
                if !parts_not_read.is_empty() {
                    problems.raise(Problem::SavedReviewPartsNotRead(parts_not_read.join(", ")));
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
            names,
            reviewing_mark,
            replies_after_repin: Vec::new(),
            agent_link_is_off: false,
            ai_review: AiReview::NotRun,
            ai_review_run: AiReviewRun::NotRun,
            review_agent: ReviewAgentLife::Gone,
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

    /// The window had the end of the screen on show.
    fn saw_end_of_screen(&mut self) {
        if !self.has_seen_end_of_screen() {
            self.seen_end_of = Some(self.screen.clone());
        }
    }

    pub const fn ai_review(&self) -> &AiReview {
        &self.ai_review
    }

    /// The sample findings of a snapshot scene, and their removal.
    #[cfg(feature = "snapshots")]
    pub fn ai_review_is(&mut self, ai_review: AiReview) {
        self.ai_review = ai_review;
        self.place_threads();
    }

    /// The run a snapshot scene shows, and its removal.
    #[cfg(feature = "snapshots")]
    pub fn ai_review_run_is(&mut self, run: AiReviewRun) {
        self.ai_review_run = run;
    }

    /// Reads the review agent's findings file again; `findings_done` is for the agent's word that it is done.
    pub fn read_findings(&mut self, folders: &AppFolders) {
        let mut read = AiReview::read(folders, &self.subject);
        if let AiReview::Findings(file) = &mut read {
            let decided = self.state.finding_decisions();
            file.findings
                .retain(|finding| !decided.has_decided(finding));
            let held: Vec<FindingId> = file.findings.iter().map(|f| f.id.clone()).collect();
            file.findings.extend(
                decided
                    .reopened()
                    .filter(|reopened| !held.contains(&reopened.id))
                    .cloned(),
            );
        }
        self.run_ends_with(&read);
        self.ai_review = read;
        self.place_threads();
    }

    /// Goes to the screen of the next finding, most severe first, round again after the last.
    pub fn go_to_next_finding(&mut self) -> Option<FindingId> {
        let AiReview::Findings(file) = &self.ai_review else {
            return None;
        };
        let mut by_severity: Vec<&Finding> = file.findings.iter().collect();
        by_severity.sort_by_key(|finding| finding.severity);
        let after_cursor = self
            .finding_cursor
            .as_ref()
            .and_then(|cursor| by_severity.iter().position(|finding| finding.id == *cursor))
            .map_or(0, |position| position + 1);
        let next = by_severity
            .get(after_cursor)
            .or_else(|| by_severity.first())?
            .id
            .clone();
        self.go_to_finding(&next).then_some(next)
    }

    pub(super) fn screen_of_home(&self, home: &FindingHome) -> Screen {
        match home {
            FindingHome::Step(id) => Screen::Step(id.clone()),
            FindingHome::File(path) => Screen::File(path.clone()),
            FindingHome::SafetyNet => Screen::SafetyNet,
            FindingHome::Change if self.tour.guided().is_some() => Screen::Summary,
            FindingHome::Change => Screen::Overview,
        }
    }

    /// Goes to the screen of one finding, as Next finding does; false when it is not open.
    pub fn go_to_finding(&mut self, id: &FindingId) -> bool {
        let Some(home) = self.findings.home_of(id) else {
            return false;
        };
        let screen = self.screen_of_home(home);
        self.go_to_screen(screen);
        if let Screen::Step(id) = &self.screen
            && self.is_folded(id)
        {
            self.toggle_fold(&id.clone());
        }
        self.finding_cursor = Some(id.clone());
        true
    }

    /// The findings of the open screen: on their lines, or after the code.
    pub(super) fn findings_here(&self) -> &[FindingView] {
        let home = match &self.screen {
            Screen::Step(id) => FindingHome::Step(id.clone()),
            Screen::File(path) => FindingHome::File(path.clone()),
            Screen::SafetyNet => FindingHome::SafetyNet,
            Screen::Summary | Screen::Overview => FindingHome::Change,
        };
        self.findings.in_home(&home)
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
            &self.agent_of_threads(),
            FindingsOfThreads::of(&self.state),
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
        self.comment_desk.thread_work(self.state.reply_drafts())
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
            &self.agent_of_threads(),
            FindingsOfThreads::of(&self.state),
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

    /// The commits the review reads between.
    pub fn pinned_tips(&self) -> ChangeTips {
        self.tour.pin().pinned_tips()
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
    /// The pieces the screen shows, as its page draws them; `None` when the tour has no such screen.
    fn pieces_shown_on(&self, shown_on: ShownOn) -> Option<Vec<PieceDiff>> {
        Some(match shown_on {
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
        })
    }

    fn lines_of(&self, place: &Place) -> Option<SelectedLines> {
        let (shown_on, only_piece) = match place {
            Place::Tour | Place::Step { .. } | Place::File { .. } => return None,
            Place::Piece { step, piece } => (ShownOn::Step { step: step.clone() }, Some(piece)),
            Place::Lines { shown_on, .. } => (shown_on.clone(), None),
        };
        let pieces = self.pieces_shown_on(shown_on)?;
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
                    finding_count: review
                        .findings
                        .in_home(&FindingHome::Step(id.clone()))
                        .len(),
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
                &self.agent_of_threads(),
                FindingsOfThreads::of(&self.state),
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
                findings: self.findings.in_home(&FindingHome::SafetyNet).len(),
            })
    }

    fn display<'a>(&'a self, links: &'a CodeLinks<'a>) -> DiffDisplay<'a> {
        self.display_options.display(links)
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
        let threads = ThreadPlacement::of(
            &self.tour,
            &self.files,
            &self.pulled_threads,
            &self.followed_lines,
            &display,
        );
        let findings = match &self.ai_review {
            AiReview::Findings(file) => file.findings.as_slice(),
            AiReview::NotRun | AiReview::Unreadable(_) => &[],
        };
        let findings = FindingPlacement::of(
            &self.tour,
            &self.files,
            findings,
            &display,
            self.state.fix_requests(),
            self.state.finding_threads(),
        );
        self.threads = threads;
        self.findings = findings;
    }

    /// "Comment now" shows only when there is surely no pending review.
    fn tour_freshness(&self) -> TourFreshness {
        if self.old_head().is_some() {
            TourFreshness::Behind
        } else {
            TourFreshness::Current
        }
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

    /// A change kept in memory stays shown when its save fails; the failure goes to stderr.
    pub(super) fn record_reporting(&mut self, what: &str, change: impl FnOnce(&mut ReviewState)) {
        if let Err(error) = self.state.record(change) {
            eprintln!("could not save {what}: {error:#}");
        }
    }

    /// The window was asked for another PR and could not open it; named until another switch succeeds.
    pub fn switch_failed(&mut self, reason: String) {
        self.raise_problem(Problem::SwitchFailed(reason));
    }

    pub fn state_file(&self) -> ReviewStateFile {
        self.state.file().clone()
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
        let finish: Vec<PaletteAction> = match self.finish_action() {
            Some(FinishAction::FinishReview) => vec![PaletteAction::FinishReview],
            Some(FinishAction::SendTasks) => vec![PaletteAction::SendTasks],
            Some(FinishAction::MarkDone) => {
                vec![PaletteAction::MarkDone, PaletteAction::FinishReview]
            }
            None => Vec::new(),
        };
        let check_for_updates = build
            .takes_updates()
            .then_some(PaletteAction::CheckForUpdates);
        next_unread
            .into_iter()
            .chain(tick)
            .chain(ask)
            .chain(self.guided_tour_palette_actions())
            .chain(self.has_findings().then_some(PaletteAction::NextFinding))
            .chain(
                self.can_list_findings()
                    .then_some(PaletteAction::FindingsList),
            )
            .chain(self.has_threads().then_some(PaletteAction::CommentsList))
            .chain(
                self.run_ai_review_refusal()
                    .is_none()
                    .then_some(PaletteAction::RunAiReview),
            )
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

    /// What finishing the review does now; `None` when there is nothing to finish.
    pub fn finish_action(&self) -> Option<FinishAction> {
        if self.can_mark_done() {
            return Some(FinishAction::MarkDone);
        }
        if self.is_marked_done() {
            return None;
        }
        self.can_finish_review().then(|| {
            if self.finishes_with_send_tasks().is_some() {
                FinishAction::SendTasks
            } else {
                FinishAction::FinishReview
            }
        })
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
