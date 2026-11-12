use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use super::file_path;
use guided_review::file_path::FilePath;
use guided_review::pull_request::{PullRequestTips, PullRequestUrl, Sha};
use guided_review::viewer::{
    AgentLaunch, AgentLauncher, AgentPort, Ask, AskId, AskProcesses, AskStore, CommentId,
    CommentPlace, GitHubError, NoAccess, NotDelivered, PictureUploadPort,
    PullRequestDescriptionPort, PullRequestTipsPort, Reaction, ReactionChange, ReactionCount,
    RelevantPullRequests, RelevantPullRequestsPort, Resolution, ReviewOutcome, ReviewThread,
    ReviewThreadsPort, ReviewVerdict, ReviewWritesPort, SendMode, SubmittedReview, ThreadId,
    TurnStopper, Viewed, ViewedFile, ViewedFilesPort, ViewedState,
};

/// What the app core sent out: what it asked of GitHub, and the lines it said to the agent.
#[derive(Clone, Default)]
pub struct SentLog(Arc<Mutex<Sent>>);

#[derive(Default)]
struct Sent {
    github: Vec<GitHubCall>,
    agent: Vec<String>,
}

/// One request the app core made of `FakeGitHub`, in the order it made them.
#[derive(Debug, Clone, PartialEq)]
pub enum GitHubCall {
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
}

impl SentLog {
    pub fn github_calls(&self) -> Vec<GitHubCall> {
        self.0.lock().unwrap().github.clone()
    }

    pub fn agent_lines(&self) -> Vec<String> {
        self.0.lock().unwrap().agent.clone()
    }

    fn record(&self, call: GitHubCall) {
        self.0.lock().unwrap().github.push(call);
    }

    fn push(&self, line: String) {
        self.0.lock().unwrap().agent.push(line);
    }
}

/// Something going wrong on GitHub's side, or on the way to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitHubFault {
    /// Every write fails as if the network were down.
    ConnectionLost,
    /// Changing a file's viewed state fails as if the network were down.
    ViewedPushesFail,
    /// Every viewed call is refused, as for a token that may not do it.
    TokenRefused(String),
    /// A new thread is answered with `thread: null` and no error.
    NewThreadsAnswerNull,
    /// The next `comment_now` finds a pending review that appeared meanwhile. Used up by it.
    PendingReviewAppearsOnCommentNow,
    /// Reading the review threads fails as if the network were down.
    ThreadReadsFail,
    /// Reading the PR's head, description, pending review and author fails as if the network were down.
    ReadsFail,
    /// Every read of the PR (head, description, threads, viewed files, pending review, author) fails the way
    /// the HTTP client reports it, so the outage is told from the rest by the real `From`.
    ReadsFailWith(ReadFailure),
    /// Listing the relevant pull requests fails as if the network were down.
    RelevantListFails,
    /// A reply is refused as GitHub does for a thread that was deleted.
    ThreadIsGone,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadFailure {
    /// GitHub answers with this HTTP status.
    Status(u16),
    TimedOut,
}

impl ReadFailure {
    fn error(&self) -> GitHubError {
        let error = match self {
            Self::Status(status) => ureq::Error::StatusCode(*status),
            Self::TimedOut => ureq::Error::Timeout(ureq::Timeout::Global),
        };
        anyhow::Error::from(error)
            .context("could not read https://github.com/acme/shop/pull/7 from GitHub")
            .into()
    }
}

pub struct FakeGitHub {
    pub sent: SentLog,
    pub tips: (Sha, Sha),
    viewed: Mutex<BTreeMap<String, ViewedState>>,
    pub threads: Vec<ReviewThread>,
    relevant: Mutex<RelevantPullRequests>,
    has_pending_review: AtomicBool,
    faults: Mutex<Vec<GitHubFault>>,
    viewer_is_author: AtomicBool,
    last_review: Mutex<Option<SubmittedReview>>,
    /// What the reviewer changed since: GitHub shows it on the next read of the threads.
    resolved_now: Mutex<BTreeMap<ThreadId, bool>>,
    reacted_now: Mutex<BTreeMap<(CommentId, Reaction), bool>>,
    edited_now: Mutex<BTreeMap<CommentId, String>>,
    deleted_now: Mutex<BTreeSet<CommentId>>,
    description: Mutex<String>,
}

impl FakeGitHub {
    pub fn with_pull(base_branch_tip: &str, head: &str, sent: &SentLog) -> Self {
        Self {
            sent: sent.clone(),
            tips: (Sha::new(base_branch_tip), Sha::new(head)),
            viewed: Mutex::default(),
            threads: Vec::new(),
            relevant: Mutex::default(),
            has_pending_review: AtomicBool::new(false),
            faults: Mutex::default(),
            viewer_is_author: AtomicBool::new(false),
            last_review: Mutex::default(),
            resolved_now: Mutex::default(),
            reacted_now: Mutex::default(),
            edited_now: Mutex::default(),
            deleted_now: Mutex::default(),
            description: Mutex::default(),
        }
    }

    /// The PR's body as it is on GitHub.
    pub fn with_description(self, body: &str) -> Self {
        body.clone_into(&mut self.description.lock().unwrap());
        self
    }

    pub fn description_now(&self) -> String {
        self.description.lock().unwrap().clone()
    }

    pub fn with_viewer_as_pull_author(self) -> Self {
        self.viewer_is_author.store(true, Ordering::SeqCst);
        self
    }

    /// A review the viewer sent on the GitHub web page.
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

    pub fn inject(&self, fault: GitHubFault) {
        let mut faults = self.faults.lock().unwrap();
        if !faults.contains(&fault) {
            faults.push(fault);
        }
    }

    pub fn heal(&self, fault: &GitHubFault) {
        self.faults.lock().unwrap().retain(|active| active != fault);
    }

    fn is_faulty(&self, fault: &GitHubFault) -> bool {
        self.faults.lock().unwrap().contains(fault)
    }

    fn use_up(&self, fault: &GitHubFault) -> bool {
        let mut faults = self.faults.lock().unwrap();
        let before = faults.len();
        faults.retain(|active| active != fault);
        faults.len() != before
    }

    fn fail_if_connection_lost(&self) -> Result<(), GitHubError> {
        if self.is_faulty(&GitHubFault::ConnectionLost) {
            return Err(anyhow::anyhow!("network is down").into());
        }
        Ok(())
    }

    fn fail_if_reads_fail_with(&self) -> Result<(), GitHubError> {
        let failure = self
            .faults
            .lock()
            .unwrap()
            .iter()
            .find_map(|fault| match fault {
                GitHubFault::ReadsFailWith(failure) => Some(failure.clone()),
                _ => None,
            });
        failure.map_or(Ok(()), |failure| Err(failure.error()))
    }

    fn fail_if_reads_fail(&self) -> Result<(), GitHubError> {
        self.fail_if_reads_fail_with()?;
        if self.is_faulty(&GitHubFault::ReadsFail) {
            return Err(anyhow::anyhow!("network is down").into());
        }
        Ok(())
    }

    fn check_access(&self) -> Result<(), GitHubError> {
        let refusal = self
            .faults
            .lock()
            .unwrap()
            .iter()
            .find_map(|fault| match fault {
                GitHubFault::TokenRefused(reason) => Some(reason.clone()),
                _ => None,
            });
        match refusal {
            Some(reason) => Err(GitHubError::NoAccess(NoAccess::new(reason))),
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

impl PullRequestTipsPort for FakeGitHub {
    fn pull_request_tips(&self, pr: &PullRequestUrl) -> Result<PullRequestTips, GitHubError> {
        self.sent.record(GitHubCall::TipsOf(pr.clone()));
        self.fail_if_reads_fail()?;
        Ok(PullRequestTips {
            base_branch_tip: self.tips.0.clone(),
            head: self.tips.1.clone(),
        })
    }
}

impl PullRequestDescriptionPort for FakeGitHub {
    fn pull_request_description(&self, _pr: &PullRequestUrl) -> Result<String, GitHubError> {
        self.fail_if_reads_fail()?;
        Ok(self.description_now())
    }

    fn set_pull_request_description(&self, pr: &PullRequestUrl, body: &str) -> anyhow::Result<()> {
        self.sent.record(GitHubCall::SetDescription {
            pr: pr.clone(),
            body: body.to_owned(),
        });
        self.fail_if_connection_lost()?;
        body.clone_into(&mut self.description.lock().unwrap());
        Ok(())
    }
}

impl PictureUploadPort for FakeGitHub {
    /// The address is the file's name under GitHub's attachments, so a test can tell pictures apart.
    fn upload_picture(&self, pr: &PullRequestUrl, picture: &Path) -> anyhow::Result<String> {
        let name = picture.file_name().unwrap().to_string_lossy().into_owned();
        self.sent.record(GitHubCall::UploadPicture {
            pr: pr.clone(),
            name: name.clone(),
        });
        self.fail_if_connection_lost()?;
        Ok(format!("https://github.com/user-attachments/assets/{name}"))
    }
}

impl ViewedFilesPort for FakeGitHub {
    fn viewed_files(&self, pr: &PullRequestUrl) -> Result<Vec<ViewedFile>, GitHubError> {
        self.sent.record(GitHubCall::ViewedFilesOf(pr.clone()));
        self.check_access()?;
        self.fail_if_reads_fail_with()?;
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
    ) -> Result<(), GitHubError> {
        self.sent.record(GitHubCall::SetViewed {
            pr: pr.clone(),
            path: path.to_string(),
            viewed,
        });
        self.check_access()?;
        if self.is_faulty(&GitHubFault::ViewedPushesFail) {
            return Err(anyhow::anyhow!("network is down").into());
        }
        let state = match viewed {
            Viewed::Yes => ViewedState::Viewed,
            Viewed::No => ViewedState::Unviewed,
        };
        self.set_viewed(path.as_str(), state);
        Ok(())
    }
}

impl FakeGitHub {
    pub fn with_relevant_pull_requests(self, relevant: RelevantPullRequests) -> Self {
        *self.relevant.lock().unwrap() = relevant;
        self
    }
}

impl RelevantPullRequestsPort for FakeGitHub {
    fn relevant_pull_requests(&self) -> Result<RelevantPullRequests, GitHubError> {
        if self.is_faulty(&GitHubFault::RelevantListFails) {
            return Err(anyhow::anyhow!("network is down").into());
        }
        Ok(self.relevant.lock().unwrap().clone())
    }
}

impl ReviewThreadsPort for FakeGitHub {
    fn review_threads(&self, pr: &PullRequestUrl) -> Result<Vec<ReviewThread>, GitHubError> {
        self.sent.record(GitHubCall::ThreadsOf(pr.clone()));
        self.fail_if_reads_fail_with()?;
        if self.is_faulty(&GitHubFault::ThreadReadsFail) {
            return Err(anyhow::anyhow!("network is down").into());
        }
        let resolved = self.resolved_now.lock().unwrap();
        let reacted = self.reacted_now.lock().unwrap();
        let edited = self.edited_now.lock().unwrap();
        let deleted = self.deleted_now.lock().unwrap();
        let mut threads = self.threads.clone();
        for thread in &mut threads {
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
                    comment.reactions.push(ReactionCount {
                        reaction: *reaction,
                        count: usize::from(*on),
                        viewer_has_reacted: *on,
                    });
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
    ) -> Result<CommentId, GitHubError> {
        self.sent.record(GitHubCall::Reply {
            thread_id: thread_id.clone(),
            body: body.to_owned(),
            mode,
        });
        self.fail_if_connection_lost()?;
        if self.is_faulty(&GitHubFault::ThreadIsGone) {
            return Err(anyhow::anyhow!(
                "GitHub answered with errors: Could not resolve to a PullRequestReviewThread"
            )
            .into());
        }
        if mode == SendMode::AddToReview {
            self.has_pending_review.store(true, Ordering::SeqCst);
        }
        let replies_so_far = self
            .sent
            .github_calls()
            .iter()
            .filter(|call| matches!(call, GitHubCall::Reply { .. }))
            .count();
        Ok(CommentId::new(&format!("R{replies_so_far}")))
    }

    fn set_thread_resolved(
        &self,
        thread_id: &ThreadId,
        resolution: Resolution,
    ) -> Result<(), GitHubError> {
        self.sent.record(GitHubCall::SetResolved {
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

    fn set_reaction(
        &self,
        comment_id: &CommentId,
        reaction: Reaction,
        change: ReactionChange,
    ) -> Result<(), GitHubError> {
        self.sent.record(GitHubCall::SetReaction {
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

    fn edit_comment(&self, comment_id: &CommentId, body: &str) -> Result<(), GitHubError> {
        self.sent.record(GitHubCall::EditComment {
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

    fn delete_comment(&self, comment_id: &CommentId) -> Result<(), GitHubError> {
        self.sent.record(GitHubCall::DeleteComment {
            comment_id: comment_id.clone(),
        });
        self.fail_if_connection_lost()?;
        self.deleted_now.lock().unwrap().insert(comment_id.clone());
        Ok(())
    }
}

impl ReviewWritesPort for FakeGitHub {
    fn has_pending_review(&self, pr: &PullRequestUrl) -> Result<bool, GitHubError> {
        self.sent.record(GitHubCall::PendingReviewOf(pr.clone()));
        self.fail_if_reads_fail()?;
        Ok(self.has_pending_review_now())
    }

    fn viewer_latest_review(
        &self,
        _pr: &PullRequestUrl,
    ) -> Result<Option<SubmittedReview>, GitHubError> {
        self.fail_if_reads_fail()?;
        Ok(self.last_review.lock().unwrap().clone())
    }

    fn viewer_authored(&self, pr: &PullRequestUrl) -> Result<bool, GitHubError> {
        self.sent.record(GitHubCall::AuthorOf(pr.clone()));
        self.fail_if_reads_fail()?;
        Ok(self.viewer_is_author.load(Ordering::SeqCst))
    }

    fn add_review_thread(
        &self,
        pr: &PullRequestUrl,
        place: &CommentPlace,
        body: &str,
    ) -> Result<(), GitHubError> {
        self.sent.record(GitHubCall::AddThread {
            pr: pr.clone(),
            place: place.clone(),
            body: body.to_owned(),
        });
        self.fail_if_connection_lost()?;
        self.has_pending_review.store(true, Ordering::SeqCst);
        if self.is_faulty(&GitHubFault::NewThreadsAnswerNull) {
            return Err(GitHubError::ThreadNotCreated("thread: null".into()));
        }
        Ok(())
    }

    fn comment_now(
        &self,
        pr: &PullRequestUrl,
        place: &CommentPlace,
        body: &str,
    ) -> Result<(), GitHubError> {
        self.sent.record(GitHubCall::CommentNow {
            pr: pr.clone(),
            place: place.clone(),
            body: body.to_owned(),
        });
        self.fail_if_connection_lost()?;
        if self.use_up(&GitHubFault::PendingReviewAppearsOnCommentNow) {
            self.has_pending_review.store(true, Ordering::SeqCst);
        }
        if self.has_pending_review_now() {
            return Err(GitHubError::PendingReviewExists);
        }
        if self.is_faulty(&GitHubFault::NewThreadsAnswerNull) {
            self.has_pending_review.store(true, Ordering::SeqCst);
            return Err(GitHubError::ThreadNotCreated("thread: null".into()));
        }
        Ok(())
    }

    fn submit_review(
        &self,
        _pr: &PullRequestUrl,
        verdict: ReviewVerdict,
        body: &str,
    ) -> Result<(), GitHubError> {
        self.sent.record(GitHubCall::Submit {
            verdict,
            body: body.to_owned(),
        });
        self.fail_if_connection_lost()?;
        self.has_pending_review.store(false, Ordering::SeqCst);
        *self.last_review.lock().unwrap() = Some(SubmittedReview {
            outcome: match verdict {
                ReviewVerdict::Comment => ReviewOutcome::Commented,
                ReviewVerdict::Approve => ReviewOutcome::Approved,
                ReviewVerdict::RequestChanges => ReviewOutcome::ChangesRequested,
            },
            body: body.to_owned(),
            submitted_at: "2026-11-06T09:00:00Z".to_owned(),
            commit: self.tips.1.clone(),
        });
        Ok(())
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
