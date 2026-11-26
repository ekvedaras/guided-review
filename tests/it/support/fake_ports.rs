use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use super::file_path;
use guided_review::viewer::{
    AutoMerge, Capability, HostCapabilities, MergeChange, MergeMethod, MergeMethods, Mergeability,
    Merging,
};

/// A PR that checks or reviews still hold back, in a repo that allows every merge method and
/// auto-merge, read by someone with write access.
pub fn blocked_merging() -> Merging {
    Merging {
        mergeability: Mergeability::Blocked,
        auto_merge: AutoMerge::Off,
        methods: MergeMethods::only(&MergeMethod::ALL),
        repo_allows_auto_merge: true,
        viewer_may_enable_auto_merge: true,
        viewer_may_disable_auto_merge: false,
        viewer_may_merge: true,
    }
}
use guided_review::code_source::CodeSource;
use guided_review::file_path::FilePath;
use guided_review::issue::IssueUrl;
use guided_review::navigation::{ServerMessage, ServerMessages};
use guided_review::pull_request::{
    ChangeTips, HostSite, HostedRepo, PullRequestUrl, RepoName, Sha,
};
use guided_review::viewer::{
    AgentLaunch, AgentLauncher, AgentPort, Ask, AskId, AskProcesses, AskStore, ChangeRequestsPort,
    ChangeTipsPort, Checks, ClonePort, CodeHost, CommentId, CommentPlace, CommentReactionsPort,
    HostError, HostedCodePort, IssueDetails, IssueDetailsPort, MergingPort, NoAccess, NotDelivered,
    OpenPrStatus, OutageCause, PictureUploadPort, PrStatus, PullOfBranch,
    PullRequestDescriptionPort, PullRequestIssues, PullRequestOrigin, PullRequestOriginPort,
    PullRequestReactionsPort, PullRequestReviews, PullRequestStatusPort, PullsOfBranchPort,
    Reaction, ReactionChange, ReactionCount, Readiness, RelevantPullRequests,
    RelevantPullRequestsPort, Resolution, ReviewOutcome, ReviewThread, ReviewThreadsPort,
    ReviewVerdict, ReviewWritesPort, Reviewer, SendMode, Side, Stance, Standing, SubmittedReview,
    ThreadComment, ThreadId, TurnStopper, Viewed, ViewedFile, ViewedFilesPort, ViewedState, Whose,
};

/// What the app core sent out: what it asked of GitHub, and the lines it said to the agent.
#[derive(Clone, Default)]
pub struct SentLog(Arc<Mutex<Sent>>);

#[derive(Default)]
struct Sent {
    host: Vec<HostCall>,
    /// The looks for a PR of a branch, kept apart: a local review makes them and no other call.
    branch_lookups: Vec<String>,
    agent: Vec<String>,
}

/// One request the app core made of `FakeHost`, in the order it made them.
#[derive(Debug, Clone, PartialEq)]
pub enum HostCall {
    TipsOf(PullRequestUrl),
    ViewedFilesOf(PullRequestUrl),
    SetViewed {
        pr: PullRequestUrl,
        path: String,
        viewed: Viewed,
    },
    ThreadsOf(PullRequestUrl),
    PendingReviewOf(PullRequestUrl),
    AuthorOf(PullRequestUrl),
    AddThread {
        pr: PullRequestUrl,
        place: CommentPlace,
        body: String,
    },
    CommentNow {
        pr: PullRequestUrl,
        place: CommentPlace,
        body: String,
    },
    Reply {
        thread_id: ThreadId,
        body: String,
        mode: SendMode,
    },
    SetResolved {
        thread_id: ThreadId,
        resolution: Resolution,
    },
    SetReaction {
        comment_id: CommentId,
        reaction: Reaction,
        change: ReactionChange,
    },
    SetPullRequestReaction {
        pr: PullRequestUrl,
        reaction: Reaction,
        change: ReactionChange,
    },
    EditComment {
        comment_id: CommentId,
        body: String,
    },
    DeleteComment {
        comment_id: CommentId,
    },
    Submit {
        verdict: ReviewVerdict,
        body: String,
    },
    SetDescription {
        pr: PullRequestUrl,
        body: String,
    },
    UploadPicture {
        pr: PullRequestUrl,
        name: String,
    },
    StatusOf(PullRequestUrl),
    MergingOf(PullRequestUrl),
    SetReadiness {
        pr: PullRequestUrl,
        readiness: Readiness,
    },
    ChangeMerging {
        pr: PullRequestUrl,
        change: MergeChange,
    },
}

impl SentLog {
    pub fn host_calls(&self) -> Vec<HostCall> {
        self.0.lock().unwrap().host.clone()
    }

    /// `owner/repo:branch` of each look for the open PR of a branch.
    pub fn branch_lookups(&self) -> Vec<String> {
        self.0.lock().unwrap().branch_lookups.clone()
    }

    pub fn agent_lines(&self) -> Vec<String> {
        self.0.lock().unwrap().agent.clone()
    }

    fn record(&self, call: HostCall) {
        self.0.lock().unwrap().host.push(call);
    }

    fn push(&self, line: String) {
        self.0.lock().unwrap().agent.push(line);
    }
}

/// Something going wrong on GitHub's side, or on the way to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostFault {
    /// Every write fails as if the network were down: the host is not answering.
    ConnectionLost,
    /// Changing a file's viewed state is answered with an error that is no outage.
    ViewedPushesAnswerAnError,
    /// Every viewed call is refused, as for a token that may not do it.
    TokenRefused(String),
    /// There is no token at all: every GraphQL call is refused, as GitHub refuses them without one, and
    /// the REST reads (tips, title, description, head branch, pulls of a branch) answer, as for a public repo.
    NoToken,
    /// A new thread is answered with `thread: null` and no error.
    NewThreadsAnswerNull,
    /// A new thread is taken but not listed when the threads are read next.
    NewThreadsUnlisted,
    /// The next `comment_now` finds a pending review that appeared meanwhile. Used up by it.
    PendingReviewAppearsOnCommentNow,
    /// Reading the review threads is answered with an error that is no outage.
    ThreadReadsAnswerAnError,
    /// Reading the PR's head, description, pending review and author is answered with an error that
    /// is no outage.
    ReadsAnswerAnError,
    /// Every read of the PR (head, description, threads, viewed files, pending review, author) fails the way
    /// the HTTP client reports it, so the outage is told from the rest by the real `From`.
    ReadsFailWith(ReadFailure),
    /// Listing the relevant pull requests is answered with an error that is no outage.
    RelevantListAnswersAnError,
    /// A reply is refused as GitHub does for a thread that was deleted.
    ThreadIsGone,
    /// Merging now is refused, as GitHub refuses it while a rule still waits for a required check,
    /// though it said nothing blocks the PR. Turning auto-merge on is taken.
    MergeNowRefused,
}

/// GitHub's words when a ruleset's required check has not reported yet.
pub const RULE_VIOLATION: &str =
    "Repository rule violations found\n\nRequired status check \"All Tests Passed\" is expected.";

/// What GitHub answers a GraphQL call it could not run, with HTTP 200: an error, not an outage.
pub const HOST_ERROR: &str =
    "GitHub answered with errors: Something went wrong while executing your query.";

fn answered_with_an_error() -> HostError {
    HostError::Other(anyhow::anyhow!(HOST_ERROR))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadFailure {
    /// GitHub answers with this HTTP status.
    Status(u16),
    TimedOut,
}

impl ReadFailure {
    fn error(&self) -> HostError {
        let error = match self {
            Self::Status(status) => ureq::Error::StatusCode(*status),
            Self::TimedOut => ureq::Error::Timeout(ureq::Timeout::Global),
        };
        anyhow::Error::from(error)
            .context("could not read https://github.com/acme/shop/pull/7 from GitHub")
            .into()
    }
}

/// What a GitHub PR's host offers, for a review whose test is not about what the host lacks.
pub fn every_capability() -> HostCapabilities {
    HostCapabilities::of(&FakeHost::with_pull("", "", &SentLog::default()))
}

pub struct FakeHost {
    pub sent: SentLog,
    lacks: BTreeSet<Capability>,
    pub tips: (Sha, Sha),
    viewed: Mutex<BTreeMap<String, ViewedState>>,
    pub threads: Vec<ReviewThread>,
    relevant: Mutex<RelevantPullRequests>,
    has_pending_review: AtomicBool,
    faults: Mutex<Vec<HostFault>>,
    viewer_is_author: AtomicBool,
    last_review: Mutex<Option<SubmittedReview>>,
    standings: Mutex<Vec<Standing>>,
    /// What the reviewer changed since: GitHub shows it on the next read of the threads.
    resolved_now: Mutex<BTreeMap<ThreadId, bool>>,
    reacted_now: Mutex<BTreeMap<(CommentId, Reaction), bool>>,
    edited_now: Mutex<BTreeMap<CommentId, String>>,
    deleted_now: Mutex<BTreeSet<CommentId>>,
    replied_now: Mutex<Vec<(ThreadId, ThreadComment)>>,
    /// New threads, which GitHub lists to their author pending or posted.
    made_now: Mutex<Vec<ReviewThread>>,
    /// The ids the next new threads and their comments get, in order.
    new_thread_ids: Mutex<VecDeque<(ThreadId, CommentId)>>,
    title: Mutex<String>,
    description: Mutex<String>,
    status: Mutex<PrStatus>,
    merging: Mutex<Merging>,
    pulls_of_branches: Mutex<Vec<(String, PullOfBranch)>>,
    head_branches: Mutex<Vec<(String, String)>>,
    issues: Mutex<PullRequestIssues>,
    origin: Mutex<Option<PullRequestOrigin>>,
    issue_details: Mutex<Vec<(IssueUrl, IssueDetails)>>,
    code: Option<Arc<dyn CodeSource>>,
}

impl FakeHost {
    pub fn with_pull(base_branch_tip: &str, head: &str, sent: &SentLog) -> Self {
        Self {
            sent: sent.clone(),
            lacks: BTreeSet::new(),
            tips: (Sha::new(base_branch_tip), Sha::new(head)),
            viewed: Mutex::default(),
            threads: Vec::new(),
            relevant: Mutex::default(),
            has_pending_review: AtomicBool::new(false),
            faults: Mutex::default(),
            viewer_is_author: AtomicBool::new(false),
            last_review: Mutex::default(),
            standings: Mutex::default(),
            resolved_now: Mutex::default(),
            reacted_now: Mutex::default(),
            edited_now: Mutex::default(),
            deleted_now: Mutex::default(),
            replied_now: Mutex::default(),
            made_now: Mutex::default(),
            new_thread_ids: Mutex::default(),
            title: Mutex::new("Hello world".to_owned()),
            description: Mutex::default(),
            status: Mutex::new(PrStatus::Open(OpenPrStatus {
                readiness: Readiness::Ready,
                checks: Checks::NoneRun,
                check_runs: Arc::default(),
                viewer_may_change_readiness: true,
            })),
            merging: Mutex::new(blocked_merging()),
            pulls_of_branches: Mutex::default(),
            head_branches: Mutex::default(),
            issues: Mutex::default(),
            origin: Mutex::default(),
            issue_details: Mutex::default(),
            code: None,
        }
    }

    /// The host reads the PR's repo as `code` does, for a review with no clone.
    pub fn serving_code(self, code: Arc<dyn CodeSource>) -> Self {
        Self {
            code: Some(code),
            ..self
        }
    }

    /// A host with no port for `capability`, as GitLab has no viewed marks.
    pub fn without(mut self, capability: Capability) -> Self {
        self.lacks.insert(capability);
        self
    }

    fn offers(&self, capability: Capability) -> bool {
        !self.lacks.contains(&capability)
    }

    /// The PR is opened from `branch` of its own repo.
    pub fn with_head_branch(self, pr: &str, branch: &str) -> Self {
        self.head_branches
            .lock()
            .unwrap()
            .push((pr.to_owned(), branch.to_owned()));
        self
    }

    /// An open PR whose head is `branch`.
    pub fn with_open_pull_from(self, branch: &str, pr: &str, title: &str) -> Self {
        self.pulls_of_branches.lock().unwrap().push((
            branch.to_owned(),
            PullOfBranch {
                pr: pr.parse().unwrap(),
                title: title.to_owned(),
            },
        ));
        self
    }

    /// The ids GitHub gives the next new threads and their comments, so a test can name them.
    pub fn naming_new_threads(self, ids: &[(&str, &str)]) -> Self {
        self.new_thread_ids.lock().unwrap().extend(
            ids.iter()
                .map(|(thread, comment)| (ThreadId::new(thread), CommentId::new(comment))),
        );
        self
    }

    pub fn with_title(self, title: &str) -> Self {
        title.clone_into(&mut self.title.lock().unwrap());
        self
    }

    /// The PR's body as it is on GitHub.
    pub fn with_description(self, body: &str) -> Self {
        body.clone_into(&mut self.description.lock().unwrap());
        self
    }

    pub fn description_now(&self) -> String {
        self.description.lock().unwrap().clone()
    }

    /// The PR's status as it is on GitHub.
    pub fn with_issues(self, issues: PullRequestIssues) -> Self {
        *self.issues.lock().unwrap() = issues;
        self
    }

    /// Who opened the PR and the branches it merges from and into.
    pub fn with_origin(self, origin: PullRequestOrigin) -> Self {
        self.set_origin(origin);
        self
    }

    /// A change made on the GitHub web page, like retargeting the PR.
    pub fn set_origin(&self, origin: PullRequestOrigin) {
        *self.origin.lock().unwrap() = Some(origin);
    }

    /// What GitHub says of an issue.
    pub fn with_issue_details(self, issue: IssueUrl, details: IssueDetails) -> Self {
        self.issue_details.lock().unwrap().push((issue, details));
        self
    }

    pub fn with_status(self, status: PrStatus) -> Self {
        self.set_status(status);
        self
    }

    /// How the PR would merge, as the host's merging port says.
    pub fn with_merging(self, merging: Merging) -> Self {
        *self.merging.lock().unwrap() = merging;
        self
    }

    /// A change made on the GitHub web page, or by a check finishing.
    pub fn set_status(&self, status: PrStatus) {
        *self.status.lock().unwrap() = status;
    }

    pub fn status_now(&self) -> PrStatus {
        self.status.lock().unwrap().clone()
    }

    pub fn with_viewer_as_pull_author(self) -> Self {
        self.viewer_is_author.store(true, Ordering::SeqCst);
        self
    }

    /// A review the viewer sent on the GitHub web page.
    pub fn with_standings(self, standings: Vec<Standing>) -> Self {
        *self.standings.lock().unwrap() = standings;
        self
    }

    pub fn with_last_review(self, review: SubmittedReview) -> Self {
        *self.last_review.lock().unwrap() = Some(review);
        self
    }

    /// A pending review started on the GitHub web page.
    pub fn with_web_pending_review(self) -> Self {
        self.has_pending_review.store(true, Ordering::SeqCst);
        self
    }

    pub fn has_pending_review_now(&self) -> bool {
        self.has_pending_review.load(Ordering::SeqCst)
    }

    pub fn inject(&self, fault: HostFault) {
        let mut faults = self.faults.lock().unwrap();
        if !faults.contains(&fault) {
            faults.push(fault);
        }
    }

    pub fn heal(&self, fault: &HostFault) {
        self.faults.lock().unwrap().retain(|active| active != fault);
    }

    fn is_faulty(&self, fault: &HostFault) -> bool {
        self.faults.lock().unwrap().contains(fault)
    }

    fn use_up(&self, fault: &HostFault) -> bool {
        let mut faults = self.faults.lock().unwrap();
        let before = faults.len();
        faults.retain(|active| active != fault);
        faults.len() != before
    }

    fn fail_if_connection_lost(&self) -> Result<(), HostError> {
        self.refuse_without_token()?;
        if self.is_faulty(&HostFault::ConnectionLost) {
            return Err(HostError::NotAnswering(
                HostSite::GitHub,
                OutageCause::NoConnection,
            ));
        }
        Ok(())
    }

    fn fail_if_read_fails_with(&self) -> Result<(), HostError> {
        let failure = self
            .faults
            .lock()
            .unwrap()
            .iter()
            .find_map(|fault| match fault {
                HostFault::ReadsFailWith(failure) => Some(failure.clone()),
                _ => None,
            });
        failure.map_or(Ok(()), |failure| Err(failure.error()))
    }

    fn fail_if_reads_fail(&self) -> Result<(), HostError> {
        self.refuse_without_token()?;
        self.fail_if_public_reads_fail()
    }

    /// GitHub's REST reads go without a token too, and answer for a public repo.
    fn fail_if_public_reads_fail(&self) -> Result<(), HostError> {
        self.fail_if_read_fails_with()?;
        if self.is_faulty(&HostFault::ReadsAnswerAnError) {
            return Err(answered_with_an_error());
        }
        Ok(())
    }

    fn refuse_without_token(&self) -> Result<(), HostError> {
        if self.is_faulty(&HostFault::NoToken) {
            return Err(HostError::NoAccess(NoAccess::new(
                "no GitHub token: run `gh auth login` or set GITHUB_TOKEN",
            )));
        }
        Ok(())
    }

    fn check_access(&self) -> Result<(), HostError> {
        self.refuse_without_token()?;
        let refusal = self
            .faults
            .lock()
            .unwrap()
            .iter()
            .find_map(|fault| match fault {
                HostFault::TokenRefused(reason) => Some(reason.clone()),
                _ => None,
            });
        match refusal {
            Some(reason) => Err(HostError::NoAccess(NoAccess::new(reason))),
            None => Ok(()),
        }
    }

    pub fn viewing(self, path: &str, state: ViewedState) -> Self {
        self.set_viewed(path, state);
        self
    }

    /// A change made on the GitHub web page.
    pub fn set_viewed(&self, path: &str, state: ViewedState) {
        self.viewed.lock().unwrap().insert(path.to_owned(), state);
    }

    pub fn viewed_state(&self, path: &str) -> ViewedState {
        *self.viewed.lock().unwrap().get(path).unwrap()
    }
}

impl PullsOfBranchPort for FakeHost {
    fn open_pull_of_branch(
        &self,
        repo: &HostedRepo,
        branch: &guided_review::review_subject::BranchName,
    ) -> Result<Option<PullOfBranch>, HostError> {
        self.sent.0.lock().unwrap().branch_lookups.push(format!(
            "{}:{}",
            repo.repo,
            branch.as_str()
        ));
        self.fail_if_public_reads_fail()?;
        Ok(self
            .pulls_of_branches
            .lock()
            .unwrap()
            .iter()
            .find(|(head, pull)| head == branch.as_str() && pull.pr.hosted_repo() == *repo)
            .map(|(_, pull)| pull.clone()))
    }

    fn pull_request_head_branch(
        &self,
        pr: &PullRequestUrl,
    ) -> Result<Option<guided_review::review_subject::BranchName>, HostError> {
        self.head_branch_of(pr)
    }
}

impl FakeHost {
    fn head_branch_of(
        &self,
        pr: &PullRequestUrl,
    ) -> Result<Option<guided_review::review_subject::BranchName>, HostError> {
        self.sent
            .0
            .lock()
            .unwrap()
            .branch_lookups
            .push(format!("head of {}", pr.short()));
        self.fail_if_public_reads_fail()?;
        let found = self
            .head_branches
            .lock()
            .unwrap()
            .iter()
            .find(|(url, _)| *url == pr.to_string())
            .map(|(_, branch)| branch.clone());
        Ok(found.map(|branch| guided_review::review_subject::BranchName::new(&branch).unwrap()))
    }
}

impl CodeHost for FakeHost {
    fn viewed_marks(&self) -> Option<&dyn ViewedFilesPort> {
        self.offers(Capability::ViewedMarks).then_some(self)
    }

    fn merging(&self) -> Option<&dyn MergingPort> {
        self.offers(Capability::Merging).then_some(self)
    }

    fn pull_request_reactions(&self) -> Option<&dyn PullRequestReactionsPort> {
        self.offers(Capability::PullRequestReactions)
            .then_some(self)
    }

    fn picture_uploads(&self) -> Option<&dyn PictureUploadPort> {
        self.offers(Capability::PictureUploads).then_some(self)
    }

    fn issue_tracker(&self) -> Option<&dyn IssueDetailsPort> {
        self.offers(Capability::IssueTracker).then_some(self)
    }

    fn change_requests(&self) -> Option<&dyn ChangeRequestsPort> {
        self.offers(Capability::ChangeRequests).then_some(self)
    }

    fn comment_reactions(&self) -> Option<&dyn CommentReactionsPort> {
        self.offers(Capability::CommentReactions).then_some(self)
    }
}

impl HostedCodePort for FakeHost {
    fn code_of(&self, repo: &RepoName) -> anyhow::Result<Arc<dyn CodeSource>> {
        self.code
            .clone()
            .ok_or_else(|| anyhow::anyhow!("FakeHost serves no code of {repo}"))
    }
}

/// A clone is a git folder whose origin is the repo on GitHub, as `gh repo clone` makes it.
impl ClonePort for FakeHost {
    fn clone_repo(&self, repo: &RepoName, folder: &Path) -> anyhow::Result<()> {
        self.fail_if_connection_lost()?;
        let git = |args: &[&std::ffi::OsStr]| {
            let status = std::process::Command::new("git").args(args).status()?;
            anyhow::ensure!(status.success(), "git {args:?} failed");
            anyhow::Ok(())
        };
        git(&["init".as_ref(), "-q".as_ref(), folder.as_os_str()])?;
        git(&[
            "-C".as_ref(),
            folder.as_os_str(),
            "remote".as_ref(),
            "add".as_ref(),
            "origin".as_ref(),
            format!("https://github.com/{repo}.git").as_ref(),
        ])
    }
}

impl ChangeTipsPort for FakeHost {
    fn pull_request_tips(&self, pr: &PullRequestUrl) -> Result<ChangeTips, HostError> {
        self.sent.record(HostCall::TipsOf(pr.clone()));
        self.fail_if_public_reads_fail()?;
        Ok(ChangeTips {
            base_branch_tip: self.tips.0.clone(),
            head: self.tips.1.clone(),
        })
    }
}

impl PullRequestDescriptionPort for FakeHost {
    fn pull_request_title(&self, _pr: &PullRequestUrl) -> Result<String, HostError> {
        self.fail_if_public_reads_fail()?;
        Ok(self.title.lock().unwrap().clone())
    }

    fn pull_request_description(&self, _pr: &PullRequestUrl) -> Result<String, HostError> {
        self.fail_if_public_reads_fail()?;
        Ok(self.description_now())
    }

    fn set_pull_request_description(&self, pr: &PullRequestUrl, body: &str) -> anyhow::Result<()> {
        self.sent.record(HostCall::SetDescription {
            pr: pr.clone(),
            body: body.to_owned(),
        });
        self.fail_if_connection_lost()?;
        body.clone_into(&mut self.description.lock().unwrap());
        Ok(())
    }

    fn pull_request_issues(&self, _pr: &PullRequestUrl) -> Result<PullRequestIssues, HostError> {
        self.fail_if_reads_fail()?;
        Ok(self.issues.lock().unwrap().clone())
    }
}

impl PullRequestOriginPort for FakeHost {
    fn pull_request_origin(&self, pr: &PullRequestUrl) -> Result<PullRequestOrigin, HostError> {
        self.fail_if_reads_fail()?;
        self.origin
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| HostError::Other(anyhow::anyhow!("no origin for {pr}")))
    }
}

impl IssueDetailsPort for FakeHost {
    fn issue_details(
        &self,
        issues: &[IssueUrl],
    ) -> Result<Vec<(IssueUrl, IssueDetails)>, HostError> {
        self.fail_if_reads_fail()?;
        Ok(self
            .issue_details
            .lock()
            .unwrap()
            .iter()
            .filter(|(url, _)| issues.contains(url))
            .cloned()
            .collect())
    }
}

impl PullRequestStatusPort for FakeHost {
    fn pull_request_status(&self, pr: &PullRequestUrl) -> Result<PrStatus, HostError> {
        self.sent.record(HostCall::StatusOf(pr.clone()));
        self.fail_if_reads_fail()?;
        Ok(self.status_now())
    }

    fn set_pull_request_readiness(
        &self,
        pr: &PullRequestUrl,
        readiness: Readiness,
    ) -> Result<(), HostError> {
        self.sent.record(HostCall::SetReadiness {
            pr: pr.clone(),
            readiness,
        });
        self.fail_if_connection_lost()?;
        if let PrStatus::Open(status) = &mut *self.status.lock().unwrap() {
            status.readiness = readiness;
        }
        Ok(())
    }
}

impl MergingPort for FakeHost {
    fn pull_request_merging(&self, pr: &PullRequestUrl) -> Result<Merging, HostError> {
        self.sent.record(HostCall::MergingOf(pr.clone()));
        self.fail_if_reads_fail()?;
        Ok(*self.merging.lock().unwrap())
    }

    fn change_merging(&self, pr: &PullRequestUrl, change: MergeChange) -> Result<(), HostError> {
        self.sent.record(HostCall::ChangeMerging {
            pr: pr.clone(),
            change,
        });
        self.fail_if_connection_lost()?;
        let auto_merge = match change {
            MergeChange::EnableAutoMerge(method) => AutoMerge::On(method),
            MergeChange::DisableAutoMerge => AutoMerge::Off,
            MergeChange::MergeNow(_) if self.is_faulty(&HostFault::MergeNowRefused) => {
                return Err(HostError::Refused(RULE_VIOLATION.to_owned()));
            }
            MergeChange::MergeNow(_) => {
                *self.status.lock().unwrap() = PrStatus::Merged;
                return Ok(());
            }
        };
        self.merging.lock().unwrap().auto_merge = auto_merge;
        Ok(())
    }
}

impl PullRequestReactionsPort for FakeHost {
    fn set_pull_request_reaction(
        &self,
        pr: &PullRequestUrl,
        reaction: Reaction,
        change: ReactionChange,
    ) -> Result<(), HostError> {
        self.sent.record(HostCall::SetPullRequestReaction {
            pr: pr.clone(),
            reaction,
            change,
        });
        self.fail_if_connection_lost()?;
        Ok(())
    }
}

impl PictureUploadPort for FakeHost {
    /// The address is the file's name under GitHub's attachments, so a test can tell pictures apart.
    fn upload_picture(&self, pr: &PullRequestUrl, picture: &Path) -> anyhow::Result<String> {
        let name = picture.file_name().unwrap().to_string_lossy().into_owned();
        self.sent.record(HostCall::UploadPicture {
            pr: pr.clone(),
            name: name.clone(),
        });
        anyhow::ensure!(
            guided_review::image_facts::media_type(&name).is_some(),
            "{name} is no picture GitHub takes"
        );
        std::fs::read(picture)
            .map_err(|error| anyhow::anyhow!("could not read {name}: {error}"))?;
        self.fail_if_connection_lost()?;
        Ok(format!("https://github.com/user-attachments/assets/{name}"))
    }
}

impl ViewedFilesPort for FakeHost {
    fn viewed_files(&self, pr: &PullRequestUrl) -> Result<Vec<ViewedFile>, HostError> {
        self.sent.record(HostCall::ViewedFilesOf(pr.clone()));
        self.check_access()?;
        self.fail_if_read_fails_with()?;
        Ok(self
            .viewed
            .lock()
            .unwrap()
            .iter()
            .map(|(path, state)| ViewedFile {
                path: file_path(path),
                state: *state,
            })
            .collect())
    }

    fn set_file_viewed(
        &self,
        pr: &PullRequestUrl,
        path: &FilePath,
        viewed: Viewed,
    ) -> Result<(), HostError> {
        self.sent.record(HostCall::SetViewed {
            pr: pr.clone(),
            path: path.to_string(),
            viewed,
        });
        self.check_access()?;
        self.fail_if_connection_lost()?;
        if self.is_faulty(&HostFault::ViewedPushesAnswerAnError) {
            return Err(answered_with_an_error());
        }
        let state = match viewed {
            Viewed::Yes => ViewedState::Viewed,
            Viewed::No => ViewedState::Unviewed,
        };
        self.set_viewed(path.as_str(), state);
        Ok(())
    }
}

impl FakeHost {
    pub fn with_relevant_pull_requests(self, relevant: RelevantPullRequests) -> Self {
        *self.relevant.lock().unwrap() = relevant;
        self
    }
}

impl RelevantPullRequestsPort for FakeHost {
    fn relevant_pull_requests(&self) -> Result<RelevantPullRequests, HostError> {
        if self.is_faulty(&HostFault::RelevantListAnswersAnError) {
            return Err(answered_with_an_error());
        }
        Ok(self.relevant.lock().unwrap().clone())
    }
}

impl ReviewThreadsPort for FakeHost {
    fn review_threads(&self, pr: &PullRequestUrl) -> Result<Vec<ReviewThread>, HostError> {
        self.sent.record(HostCall::ThreadsOf(pr.clone()));
        self.refuse_without_token()?;
        self.fail_if_read_fails_with()?;
        if self.is_faulty(&HostFault::ThreadReadsAnswerAnError) {
            return Err(answered_with_an_error());
        }
        let resolved = self.resolved_now.lock().unwrap();
        let reacted = self.reacted_now.lock().unwrap();
        let edited = self.edited_now.lock().unwrap();
        let deleted = self.deleted_now.lock().unwrap();
        let replied = self.replied_now.lock().unwrap();
        let mut threads = self.threads.clone();
        threads.extend(self.made_now.lock().unwrap().iter().cloned());
        for thread in &mut threads {
            thread.comments.extend(
                replied
                    .iter()
                    .filter(|(replied_to, _)| *replied_to == thread.id)
                    .map(|(_, reply)| reply.clone()),
            );
            thread
                .comments
                .retain(|comment| !deleted.contains(&comment.id));
            if let Some(is_resolved) = resolved.get(&thread.id) {
                thread.is_resolved = *is_resolved;
            }
            for comment in &mut thread.comments {
                if let Some(body) = edited.get(&comment.id) {
                    comment.body.clone_from(body);
                }
                for ((comment_id, reaction), on) in reacted.iter() {
                    if *comment_id != comment.id {
                        continue;
                    }
                    comment
                        .reactions
                        .retain(|count| count.reaction != *reaction);
                    if *on {
                        comment.reactions.push(ReactionCount {
                            reaction: *reaction,
                            count: 1,
                            viewer_has_reacted: true,
                        });
                    }
                }
            }
        }
        threads.retain(|thread| !thread.comments.is_empty());
        Ok(threads)
    }

    fn reply_to_thread(
        &self,
        _pr: &PullRequestUrl,
        thread_id: &ThreadId,
        body: &str,
        mode: SendMode,
    ) -> Result<CommentId, HostError> {
        self.sent.record(HostCall::Reply {
            thread_id: thread_id.clone(),
            body: body.to_owned(),
            mode,
        });
        self.fail_if_connection_lost()?;
        let is_known = self
            .threads
            .iter()
            .chain(self.made_now.lock().unwrap().iter())
            .any(|thread| thread.id == *thread_id);
        if !is_known || self.is_faulty(&HostFault::ThreadIsGone) {
            return Err(anyhow::anyhow!(
                "GitHub answered with errors: Could not resolve to a PullRequestReviewThread with the id of '{thread_id}'."
            )
            .into());
        }
        match mode {
            SendMode::AddToReview => self.has_pending_review.store(true, Ordering::SeqCst),
            SendMode::CommentNow => self.post_review_now(),
            SendMode::SendNow | SendMode::AddToTasks => {}
        }
        let replies_so_far = self
            .sent
            .host_calls()
            .iter()
            .filter(|call| matches!(call, HostCall::Reply { .. }))
            .count();
        let reply = CommentId::new(&format!("R{replies_so_far}"));
        self.replied_now
            .lock()
            .unwrap()
            .push((thread_id.clone(), viewers_comment(&reply, body)));
        Ok(reply)
    }

    fn set_thread_resolved(
        &self,
        thread_id: &ThreadId,
        resolution: Resolution,
    ) -> Result<(), HostError> {
        self.sent.record(HostCall::SetResolved {
            thread_id: thread_id.clone(),
            resolution,
        });
        self.fail_if_connection_lost()?;
        self.resolved_now
            .lock()
            .unwrap()
            .insert(thread_id.clone(), resolution == Resolution::Resolved);
        Ok(())
    }

    fn edit_comment(&self, comment_id: &CommentId, body: &str) -> Result<(), HostError> {
        self.sent.record(HostCall::EditComment {
            comment_id: comment_id.clone(),
            body: body.to_owned(),
        });
        self.fail_if_connection_lost()?;
        self.edited_now
            .lock()
            .unwrap()
            .insert(comment_id.clone(), body.to_owned());
        Ok(())
    }

    fn delete_comment(&self, comment_id: &CommentId) -> Result<(), HostError> {
        self.sent.record(HostCall::DeleteComment {
            comment_id: comment_id.clone(),
        });
        self.fail_if_connection_lost()?;
        self.deleted_now.lock().unwrap().insert(comment_id.clone());
        Ok(())
    }
}

impl ReviewWritesPort for FakeHost {
    fn has_pending_review(&self, pr: &PullRequestUrl) -> Result<bool, HostError> {
        self.sent.record(HostCall::PendingReviewOf(pr.clone()));
        self.fail_if_reads_fail()?;
        Ok(self.has_pending_review_now())
    }

    fn reviews(&self, _pr: &PullRequestUrl) -> Result<PullRequestReviews, HostError> {
        self.fail_if_reads_fail()?;
        Ok(PullRequestReviews {
            viewer_last: self.last_review.lock().unwrap().clone(),
            standings: self.standings.lock().unwrap().clone(),
        })
    }

    fn viewer_authored(&self, pr: &PullRequestUrl) -> Result<bool, HostError> {
        self.sent.record(HostCall::AuthorOf(pr.clone()));
        self.fail_if_reads_fail()?;
        Ok(self.viewer_is_author.load(Ordering::SeqCst))
    }

    fn add_review_thread(
        &self,
        pr: &PullRequestUrl,
        place: &CommentPlace,
        body: &str,
    ) -> Result<(), HostError> {
        self.sent.record(HostCall::AddThread {
            pr: pr.clone(),
            place: place.clone(),
            body: body.to_owned(),
        });
        self.fail_if_connection_lost()?;
        self.has_pending_review.store(true, Ordering::SeqCst);
        if self.is_faulty(&HostFault::NewThreadsAnswerNull) {
            return Err(HostError::ThreadNotCreated("thread: null".into()));
        }
        self.make_thread(place, body);
        Ok(())
    }

    fn comment_now(
        &self,
        pr: &PullRequestUrl,
        place: &CommentPlace,
        body: &str,
    ) -> Result<(), HostError> {
        self.sent.record(HostCall::CommentNow {
            pr: pr.clone(),
            place: place.clone(),
            body: body.to_owned(),
        });
        self.fail_if_connection_lost()?;
        if self.use_up(&HostFault::PendingReviewAppearsOnCommentNow) {
            self.has_pending_review.store(true, Ordering::SeqCst);
        }
        if self.has_pending_review_now() {
            return Err(HostError::PendingReviewExists);
        }
        if self.is_faulty(&HostFault::NewThreadsAnswerNull) {
            self.has_pending_review.store(true, Ordering::SeqCst);
            return Err(HostError::ThreadNotCreated("thread: null".into()));
        }
        self.make_thread(place, body);
        self.post_review_now();
        Ok(())
    }

    fn submit_review(&self, _pr: &PullRequestUrl, body: &str) -> Result<(), HostError> {
        self.send_review(ReviewVerdict::Comment, body)
    }

    fn approve(&self, _pr: &PullRequestUrl, body: &str) -> Result<(), HostError> {
        self.send_review(ReviewVerdict::Approve, body)
    }
}

impl CommentReactionsPort for FakeHost {
    fn set_comment_reaction(
        &self,
        comment_id: &CommentId,
        reaction: Reaction,
        change: ReactionChange,
    ) -> Result<(), HostError> {
        self.sent.record(HostCall::SetReaction {
            comment_id: comment_id.clone(),
            reaction,
            change,
        });
        self.fail_if_connection_lost()?;
        self.reacted_now.lock().unwrap().insert(
            (comment_id.clone(), reaction),
            change == ReactionChange::Add,
        );
        Ok(())
    }
}

impl ChangeRequestsPort for FakeHost {
    fn request_changes(&self, _pr: &PullRequestUrl, body: &str) -> Result<(), HostError> {
        self.send_review(ReviewVerdict::RequestChanges, body)
    }
}

impl FakeHost {
    fn send_review(&self, verdict: ReviewVerdict, body: &str) -> Result<(), HostError> {
        self.sent.record(HostCall::Submit {
            verdict,
            body: body.to_owned(),
        });
        self.fail_if_connection_lost()?;
        self.record_viewers_review(
            match verdict {
                ReviewVerdict::Comment => ReviewOutcome::Commented,
                ReviewVerdict::Approve => ReviewOutcome::Approved,
                ReviewVerdict::RequestChanges => ReviewOutcome::ChangesRequested,
            },
            body,
        );
        Ok(())
    }

    /// GitHub posts a comment or reply now as a review of its own, with no body.
    fn post_review_now(&self) {
        self.record_viewers_review(ReviewOutcome::Commented, "");
    }

    /// The viewer's sent review is their last, and stands as theirs in place of any other.
    fn record_viewers_review(&self, outcome: ReviewOutcome, body: &str) {
        self.has_pending_review.store(false, Ordering::SeqCst);
        *self.last_review.lock().unwrap() = Some(SubmittedReview {
            outcome,
            body: body.to_owned(),
            submitted_at: "2026-11-06T09:00:00Z".to_owned(),
            commit: self.tips.1.clone(),
        });
        let mut standings = self.standings.lock().unwrap();
        standings.retain(|standing| !standing.is_viewer());
        standings.push(Standing::of(
            Reviewer::person(VIEWER_LOGIN, None),
            Whose::Viewers,
            Stance::Reviewed {
                outcome,
                commit: self.tips.1.clone(),
            },
        ));
    }

    fn make_thread(&self, place: &CommentPlace, body: &str) {
        if self.is_faulty(&HostFault::NewThreadsUnlisted) {
            return;
        }
        let mut made = self.made_now.lock().unwrap();
        let fallback = format!("N{}", made.len() + 1);
        let (id, comment_id) = self
            .new_thread_ids
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| {
                (
                    ThreadId::new(&fallback),
                    CommentId::new(&format!("{fallback}C")),
                )
            });
        let (line, start_line, side, is_file_comment) = match place {
            CommentPlace::File(_) => (None, None, Side::Head, true),
            CommentPlace::Lines { start, end } => (
                Some(end.line),
                (start != end).then_some(start.line),
                end.side,
                false,
            ),
        };
        made.push(ReviewThread {
            id,
            is_resolved: false,
            is_outdated: false,
            path: place.path().clone(),
            line,
            start_line,
            side,
            is_file_comment,
            original_line: line,
            original_start_line: start_line,
            original_commit: Some(self.tips.1.as_str().to_owned()),
            comments: vec![viewers_comment(&comment_id, body)],
        });
    }
}

/// The login `FakeHost` knows the viewer by.
const VIEWER_LOGIN: &str = "me";

fn viewers_comment(id: &CommentId, body: &str) -> ThreadComment {
    ThreadComment {
        id: id.clone(),
        author: VIEWER_LOGIN.to_owned(),
        avatar_url: None,
        is_bot: false,
        by_agent: None,
        by_viewer: true,
        body: body.to_owned(),
        url: format!("https://github.com/acme/shop/pull/7#discussion_r{id}"),
        reactions: Vec::new(),
    }
}

/// Replays scripted answers, one stream line at a time, one scripted turn per question.
pub struct FakeAgent {
    pub sent: SentLog,
    turns: VecDeque<Vec<String>>,
}

impl FakeAgent {
    pub fn answering(turns: &[&str], sent: &SentLog) -> Self {
        Self {
            sent: sent.clone(),
            turns: turns
                .iter()
                .map(|turn| turn.lines().map(str::to_owned).collect())
                .collect(),
        }
    }
}

impl AgentPort for FakeAgent {
    fn ask(&mut self, message: &str, on_line: &mut dyn FnMut(&str)) -> anyhow::Result<()> {
        self.sent.push(format!("agent: {message}"));
        let turn = self.turns.pop_front().unwrap_or_default();
        for line in &turn {
            on_line(line);
        }
        Ok(())
    }

    fn turn_stopper(&self) -> Arc<dyn TurnStopper> {
        Arc::new(FakeTurnStopper(self.sent.clone()))
    }
}

struct FakeTurnStopper(SentLog);

impl TurnStopper for FakeTurnStopper {
    fn stop_turn(&self) -> anyhow::Result<()> {
        self.0.push("agent: stop the turn".to_owned());
        Ok(())
    }
}

/// Hands out one `FakeAgent` per start and logs how it was launched.
pub struct FakeAgentLauncher {
    pub sent: SentLog,
    pub turns: Vec<String>,
}

impl AgentLauncher for FakeAgentLauncher {
    fn start(&self, launch: &AgentLaunch) -> anyhow::Result<Box<dyn AgentPort>> {
        self.sent.push(format!(
            "agent started in {} with {} / {}{}",
            launch.cwd.display(),
            launch.model,
            launch.effort,
            launch
                .resume
                .as_ref()
                .map_or_default(|id| format!(", resuming {id}"))
        ));
        let turns: Vec<&str> = self.turns.iter().map(String::as_str).collect();
        Ok(Box::new(FakeAgent::answering(&turns, &self.sent)))
    }
}

/// Stands in for the Asks' process threads: logs what the desk does to them, and lets the next
/// deliveries fail as if the process had died.
#[derive(Default)]
pub struct FakeAskProcesses {
    pub log: Vec<String>,
    running: Vec<AskId>,
    pub deliveries_failing: usize,
}

impl AskProcesses for FakeAskProcesses {
    fn start(&mut self, ask: AskId, launch: AgentLaunch) {
        assert!(launch.cwd.exists(), "an Ask's process starts in the clone");
        self.log
            .push(format!("start {}", serde_json::to_string(&ask).unwrap()));
        self.running.push(ask);
    }

    fn stop(&mut self, ask: AskId) {
        self.log
            .push(format!("stop {}", serde_json::to_string(&ask).unwrap()));
        self.running.retain(|running| *running != ask);
    }

    fn is_running(&self, ask: AskId) -> bool {
        self.running.contains(&ask)
    }

    fn deliver(&mut self, ask: AskId, message: String) -> Result<(), NotDelivered> {
        if self.deliveries_failing > 0 {
            self.deliveries_failing -= 1;
            self.log.push(format!(
                "not delivered to {}",
                serde_json::to_string(&ask).unwrap()
            ));
            return Err(NotDelivered);
        }
        let question = message.lines().last().unwrap_or_default();
        self.log.push(format!(
            "deliver to {}: {question}",
            serde_json::to_string(&ask).unwrap()
        ));
        Ok(())
    }

    fn turn_stopper(&self, _: AskId) -> Option<Arc<dyn TurnStopper>> {
        None
    }
}

/// Stands in for the per-PR state file.
#[derive(Clone, Default)]
pub struct FakeAskStore(Arc<Mutex<Vec<Ask>>>);

impl FakeAskStore {
    pub fn saved(&self) -> Vec<Ask> {
        self.0.lock().unwrap().clone()
    }
}

impl AskStore for FakeAskStore {
    fn load(&self) -> anyhow::Result<Vec<Ask>> {
        Ok(self.saved())
    }

    fn save(&self, asks: &[Ask]) -> anyhow::Result<()> {
        *self.0.lock().unwrap() = asks.to_vec();
        Ok(())
    }
}

/// Stands in for GitHub's releases: the latest one, and the files it serves by address.
#[derive(Default)]
pub struct FakeReleases {
    pub latest: Option<guided_review::update::PublishedRelease>,
    pub files: BTreeMap<String, Vec<u8>>,
    pub downloaded: Mutex<Vec<String>>,
}

impl guided_review::update::ReleasesPort for FakeReleases {
    fn latest_release(&self) -> anyhow::Result<guided_review::update::PublishedRelease> {
        self.latest
            .clone()
            .ok_or_else(|| anyhow::anyhow!("GitHub is down"))
    }

    fn download(&self, url: &str) -> anyhow::Result<Vec<u8>> {
        self.downloaded.lock().unwrap().push(url.to_owned());
        self.files
            .get(url)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("404 for {url}"))
    }
}

/// Stands in for searching a commit for the files that hold a word: answers what it was given
/// and keeps the words it was asked for.
#[derive(Clone, Default)]
pub struct FakeFilesWithWord {
    files: BTreeMap<String, Vec<FilePath>>,
    searches: Arc<Mutex<Vec<String>>>,
}

impl FakeFilesWithWord {
    pub fn with(mut self, word: &str, paths: &[&str]) -> Self {
        self.files.insert(
            word.to_owned(),
            paths.iter().map(|path| file_path(path)).collect(),
        );
        self
    }

    pub fn searches(&self) -> Vec<String> {
        self.searches.lock().unwrap().clone()
    }
}

impl guided_review::navigation::FilesWithWord for FakeFilesWithWord {
    fn files_with(
        &self,
        _sha: &Sha,
        word: &guided_review::navigation::Word,
    ) -> anyhow::Result<Vec<FilePath>> {
        self.searches.lock().unwrap().push(word.as_str().to_owned());
        Ok(self.files.get(word.as_str()).cloned().unwrap_or_default())
    }
}

/// Stands in for a code navigator adapter: reports the states it was told to wait in, then gives
/// the answer it was given, and keeps what it was asked.
#[derive(Clone)]
pub struct FakeNavigator {
    waits: Vec<guided_review::navigation::Waiting>,
    answered_by: guided_review::navigation::AnsweredBy,
    asked: Arc<Mutex<Vec<guided_review::navigation::NavigationRequest>>>,
}

impl FakeNavigator {
    pub fn answering_by(answered_by: guided_review::navigation::AnsweredBy) -> Self {
        Self {
            waits: Vec::new(),
            answered_by,
            asked: Arc::default(),
        }
    }

    pub fn waiting(mut self, states: &[guided_review::navigation::Waiting]) -> Self {
        self.waits = states.to_vec();
        self
    }

    pub fn asked(&self) -> Vec<guided_review::navigation::NavigationRequest> {
        self.asked.lock().unwrap().clone()
    }
}

impl guided_review::navigation::CodeNavigator for FakeNavigator {
    fn navigate(
        &self,
        _code: &dyn CodeSource,
        request: &guided_review::navigation::NavigationRequest,
        waiting: &dyn Fn(guided_review::navigation::Waiting),
    ) -> anyhow::Result<guided_review::navigation::Answer> {
        self.asked.lock().unwrap().push(request.clone());
        for state in &self.waits {
            waiting(state.clone());
        }
        Ok(guided_review::navigation::Answer {
            lookup: guided_review::navigation::Lookup {
                word: request.word.clone(),
                definitions: Vec::new(),
                usages: Arc::new([guided_review::navigation::Usage {
                    place: guided_review::navigation::Place {
                        path: file_path("src/checkout.ts"),
                        line: crate::support::line(5),
                    },
                    text: "return cart.total() + price();".into(),
                }]),
                reach: guided_review::navigation::Reach::WholeRepo,
                cut_short: false,
                on_disk_files: Arc::new([]),
            },
            answered_by: self.answered_by.clone(),
        })
    }
}

/// Server messages nobody reads.
pub struct IgnoredServerMessages;

impl ServerMessages for IgnoredServerMessages {
    fn show(&self, _message: ServerMessage) {}
}

/// Server messages kept in the order they were shown.
#[derive(Default)]
pub struct RecordedServerMessages(Mutex<Vec<ServerMessage>>);

impl RecordedServerMessages {
    pub fn shown(&self) -> Vec<ServerMessage> {
        self.0.lock().unwrap().clone()
    }
}

impl ServerMessages for RecordedServerMessages {
    fn show(&self, message: ServerMessage) {
        self.0.lock().unwrap().push(message);
    }
}
