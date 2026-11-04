use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use super::file_path;
use guided_review::file_path::FilePath;
use guided_review::pull_request::{PullRequestTips, PullRequestUrl, Sha};
use guided_review::viewer::{
    AgentLaunch, AgentLauncher, AgentPort, Ask, AskStore, CommentId, CommentPlace, GitHubError,
    NoAccess, PullRequestDescriptionPort, PullRequestTipsPort, Reaction, ReactionChange,
    ReactionCount, Resolution, ReviewThread, ReviewThreadsPort, ReviewVerdict, ReviewWritesPort,
    SendMode, ThreadId, TurnStopper, Viewed, ViewedFile, ViewedFilesPort, ViewedState,
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
    Submit {
        verdict: ReviewVerdict,
        body: String,
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
}

pub struct FakeGitHub {
    pub sent: SentLog,
    pub tips: (Sha, Sha),
    viewed: Mutex<BTreeMap<String, ViewedState>>,
    pub threads: Vec<ReviewThread>,
    has_pending_review: AtomicBool,
    faults: Mutex<Vec<GitHubFault>>,
    viewer_is_author: AtomicBool,
    /// What the reviewer changed since: GitHub shows it on the next read of the threads.
    resolved_now: Mutex<BTreeMap<ThreadId, bool>>,
    reacted_now: Mutex<BTreeMap<(CommentId, Reaction), bool>>,
}

impl FakeGitHub {
    pub fn with_pull(base_branch_tip: &str, head: &str, sent: &SentLog) -> Self {
        Self {
            sent: sent.clone(),
            tips: (Sha::new(base_branch_tip), Sha::new(head)),
            viewed: Mutex::default(),
            threads: Vec::new(),
            has_pending_review: AtomicBool::new(false),
            faults: Mutex::default(),
            viewer_is_author: AtomicBool::new(false),
            resolved_now: Mutex::default(),
            reacted_now: Mutex::default(),
        }
    }

    pub fn with_viewer_as_pull_author(self) -> Self {
        self.viewer_is_author.store(true, Ordering::SeqCst);
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

    fn fail_if_reads_fail(&self) -> Result<(), GitHubError> {
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
    fn pull_request_tips(&self, pr: &PullRequestUrl) -> anyhow::Result<PullRequestTips> {
        self.sent.record(GitHubCall::TipsOf(pr.clone()));
        self.fail_if_reads_fail()?;
        Ok(PullRequestTips {
            base_branch_tip: self.tips.0.clone(),
            head: self.tips.1.clone(),
        })
    }
}

impl PullRequestDescriptionPort for FakeGitHub {
    fn pull_request_description(&self, _pr: &PullRequestUrl) -> anyhow::Result<String> {
        Ok(String::new())
    }
}

impl ViewedFilesPort for FakeGitHub {
    fn viewed_files(&self, pr: &PullRequestUrl) -> Result<Vec<ViewedFile>, GitHubError> {
        self.sent.record(GitHubCall::ViewedFilesOf(pr.clone()));
        self.check_access()?;
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

impl ReviewThreadsPort for FakeGitHub {
    fn review_threads(&self, pr: &PullRequestUrl) -> Result<Vec<ReviewThread>, GitHubError> {
        self.sent.record(GitHubCall::ThreadsOf(pr.clone()));
        if self.is_faulty(&GitHubFault::ThreadReadsFail) {
            return Err(anyhow::anyhow!("network is down").into());
        }
        let resolved = self.resolved_now.lock().unwrap();
        let reacted = self.reacted_now.lock().unwrap();
        let mut threads = self.threads.clone();
        for thread in &mut threads {
            if let Some(is_resolved) = resolved.get(&thread.id) {
                thread.is_resolved = *is_resolved;
            }
            for comment in &mut thread.comments {
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
        Ok(threads)
    }

    fn reply_to_thread(
        &self,
        _pr: &PullRequestUrl,
        thread_id: &ThreadId,
        body: &str,
        mode: SendMode,
    ) -> Result<(), GitHubError> {
        self.sent.record(GitHubCall::Reply {
            thread_id: thread_id.clone(),
            body: body.to_owned(),
            mode,
        });
        self.fail_if_connection_lost()?;
        if mode == SendMode::AddToReview {
            self.has_pending_review.store(true, Ordering::SeqCst);
        }
        Ok(())
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
}

impl ReviewWritesPort for FakeGitHub {
    fn has_pending_review(&self, pr: &PullRequestUrl) -> Result<bool, GitHubError> {
        self.sent.record(GitHubCall::PendingReviewOf(pr.clone()));
        Ok(self.has_pending_review_now())
    }

    fn viewer_authored(&self, pr: &PullRequestUrl) -> Result<bool, GitHubError> {
        self.sent.record(GitHubCall::AuthorOf(pr.clone()));
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
                .map(|id| format!(", resuming {id}"))
                .unwrap_or_default()
        ));
        let turns: Vec<&str> = self.turns.iter().map(String::as_str).collect();
        Ok(Box::new(FakeAgent::answering(&turns, &self.sent)))
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
