use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use guided_review::pull_request::{PullRequestTips, PullRequestUrl, Sha};
use guided_review::viewer::{
    AgentLaunch, AgentLauncher, AgentPort, Ask, AskStore, CommentPlace, PendingReviewExists,
    PullRequestTipsPort, Reaction, ReactionCount, ReviewThread, ReviewThreadsPort, ReviewVerdict,
    ReviewWritesPort, SendMode, Side, ThreadNotCreated, TurnStopper, ViewedFile, ViewedFilesPort,
    ViewedState, ViewedSyncUnavailable,
};

/// What the app core sent out, in order, across both fakes.
#[derive(Clone, Default)]
pub struct SentLog(Arc<Mutex<Vec<String>>>);

impl SentLog {
    pub fn entries(&self) -> Vec<String> {
        self.0.lock().unwrap().clone()
    }

    fn push(&self, entry: String) {
        self.0.lock().unwrap().push(entry);
    }
}

pub struct FakeGitHub {
    pub sent: SentLog,
    pub tips: (Sha, Sha),
    viewed: Mutex<BTreeMap<String, ViewedState>>,
    pushes_fail: AtomicBool,
    refuses_with: Mutex<Option<String>>,
    pub threads: Vec<ReviewThread>,
    has_pending_review: AtomicBool,
    answers_thread_null: AtomicBool,
    cannot_reach_github: AtomicBool,
    /// Says "one pending review per user" to the next `comment_now`, as if one appeared meanwhile.
    pending_review_appears_on_comment_now: AtomicBool,
    viewer_is_author: AtomicBool,
    /// What the reviewer changed since: GitHub shows it on the next read of the threads.
    resolved_now: Mutex<BTreeMap<String, bool>>,
    reacted_now: Mutex<BTreeMap<(String, Reaction), bool>>,
}

impl FakeGitHub {
    pub fn with_pull(base_branch_tip: &str, head: &str, sent: &SentLog) -> Self {
        Self {
            sent: sent.clone(),
            tips: (Sha::new(base_branch_tip), Sha::new(head)),
            viewed: Mutex::default(),
            pushes_fail: AtomicBool::new(false),
            refuses_with: Mutex::default(),
            threads: Vec::new(),
            has_pending_review: AtomicBool::new(false),
            answers_thread_null: AtomicBool::new(false),
            cannot_reach_github: AtomicBool::new(false),
            pending_review_appears_on_comment_now: AtomicBool::new(false),
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

    pub fn answer_new_threads_with_null(&self, on: bool) {
        self.answers_thread_null.store(on, Ordering::SeqCst);
    }

    pub fn lose_connection(&self, lost: bool) {
        self.cannot_reach_github.store(lost, Ordering::SeqCst);
    }

    pub fn a_pending_review_appears_on_comment_now(&self) {
        self.pending_review_appears_on_comment_now
            .store(true, Ordering::SeqCst);
    }

    fn describe(place: &CommentPlace, body: &str) -> String {
        if place.is_file_comment() {
            return format!("{} FILE {body:?}", place.path());
        }
        let side = |side: Side| if side == Side::Base { "LEFT" } else { "RIGHT" };
        let start = if place.is_single_line() {
            String::new()
        } else {
            format!("{} {} to ", side(place.start.side), place.start.line)
        };
        format!(
            "{} {start}{} {} {body:?}",
            place.path(),
            side(place.end.side),
            place.end.line
        )
    }

    pub fn viewing(self, path: &str, state: ViewedState) -> Self {
        self.set_viewed(path, state);
        self
    }

    /// A change made on the GitHub web page.
    pub fn set_viewed(&self, path: &str, state: ViewedState) {
        self.viewed.lock().unwrap().insert(path.to_string(), state);
    }

    pub fn viewed_state(&self, path: &str) -> ViewedState {
        self.viewed.lock().unwrap()[path]
    }

    pub fn pushes_fail(&self, fail: bool) {
        self.pushes_fail.store(fail, Ordering::SeqCst);
    }

    pub fn refuse_everything(&self, reason: &str) {
        *self.refuses_with.lock().unwrap() = Some(reason.to_string());
    }

    fn check_access(&self) -> anyhow::Result<()> {
        match self.refuses_with.lock().unwrap().clone() {
            Some(reason) => Err(ViewedSyncUnavailable(reason).into()),
            None => Ok(()),
        }
    }
}

impl PullRequestTipsPort for FakeGitHub {
    fn pull_request_tips(&self, pr: &PullRequestUrl) -> anyhow::Result<PullRequestTips> {
        self.sent.push(format!("github: tips of {pr}"));
        Ok(PullRequestTips {
            base_branch_tip: self.tips.0.clone(),
            head: self.tips.1.clone(),
        })
    }
}

impl ViewedFilesPort for FakeGitHub {
    fn viewed_files(&self, pr: &PullRequestUrl) -> anyhow::Result<Vec<ViewedFile>> {
        self.sent.push(format!("github: viewed files of {pr}"));
        self.check_access()?;
        Ok(self
            .viewed
            .lock()
            .unwrap()
            .iter()
            .map(|(path, state)| ViewedFile {
                path: path.clone(),
                state: *state,
            })
            .collect())
    }

    fn set_file_viewed(&self, pr: &PullRequestUrl, path: &str, viewed: bool) -> anyhow::Result<()> {
        self.sent
            .push(format!("github: viewed={viewed} {path} of {pr}"));
        self.check_access()?;
        if self.pushes_fail.load(Ordering::SeqCst) {
            anyhow::bail!("network is down");
        }
        let state = if viewed {
            ViewedState::Viewed
        } else {
            ViewedState::Unviewed
        };
        self.set_viewed(path, state);
        Ok(())
    }
}

impl ReviewThreadsPort for FakeGitHub {
    fn review_threads(&self, pr: &PullRequestUrl) -> anyhow::Result<Vec<ReviewThread>> {
        self.sent.push(format!("github: threads of {pr}"));
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
        thread_id: &str,
        body: &str,
        mode: SendMode,
    ) -> anyhow::Result<()> {
        self.sent
            .push(format!("github: reply to {thread_id} {mode:?} {body:?}"));
        if self.cannot_reach_github.load(Ordering::SeqCst) {
            anyhow::bail!("network is down");
        }
        if mode == SendMode::AddToReview {
            self.has_pending_review.store(true, Ordering::SeqCst);
        }
        Ok(())
    }

    fn set_thread_resolved(&self, thread_id: &str, resolved: bool) -> anyhow::Result<()> {
        let verb = if resolved { "resolve" } else { "unresolve" };
        self.sent.push(format!("github: {verb} {thread_id}"));
        if self.cannot_reach_github.load(Ordering::SeqCst) {
            anyhow::bail!("network is down");
        }
        self.resolved_now
            .lock()
            .unwrap()
            .insert(thread_id.to_string(), resolved);
        Ok(())
    }

    fn set_reaction(&self, comment_id: &str, reaction: Reaction, on: bool) -> anyhow::Result<()> {
        let verb = if on { "react" } else { "unreact" };
        self.sent.push(format!(
            "github: {verb} {} on {comment_id}",
            reaction.github_name()
        ));
        if self.cannot_reach_github.load(Ordering::SeqCst) {
            anyhow::bail!("network is down");
        }
        self.reacted_now
            .lock()
            .unwrap()
            .insert((comment_id.to_string(), reaction), on);
        Ok(())
    }
}

impl ReviewWritesPort for FakeGitHub {
    fn has_pending_review(&self, pr: &PullRequestUrl) -> anyhow::Result<bool> {
        self.sent.push(format!("github: pending review of {pr}"));
        Ok(self.has_pending_review_now())
    }

    fn viewer_authored(&self, pr: &PullRequestUrl) -> anyhow::Result<bool> {
        self.sent.push(format!("github: author of {pr}"));
        Ok(self.viewer_is_author.load(Ordering::SeqCst))
    }

    fn add_review_thread(
        &self,
        pr: &PullRequestUrl,
        place: &CommentPlace,
        body: &str,
    ) -> anyhow::Result<()> {
        self.sent.push(format!(
            "github: add thread {} to {pr}",
            Self::describe(place, body)
        ));
        if self.cannot_reach_github.load(Ordering::SeqCst) {
            anyhow::bail!("network is down");
        }
        self.has_pending_review.store(true, Ordering::SeqCst);
        if self.answers_thread_null.load(Ordering::SeqCst) {
            return Err(ThreadNotCreated("thread: null".into()).into());
        }
        Ok(())
    }

    fn comment_now(
        &self,
        pr: &PullRequestUrl,
        place: &CommentPlace,
        body: &str,
    ) -> anyhow::Result<()> {
        self.sent.push(format!(
            "github: comment now {} on {pr}",
            Self::describe(place, body)
        ));
        if self.cannot_reach_github.load(Ordering::SeqCst) {
            anyhow::bail!("network is down");
        }
        if self
            .pending_review_appears_on_comment_now
            .swap(false, Ordering::SeqCst)
        {
            self.has_pending_review.store(true, Ordering::SeqCst);
        }
        if self.has_pending_review_now() {
            return Err(PendingReviewExists.into());
        }
        if self.answers_thread_null.load(Ordering::SeqCst) {
            self.has_pending_review.store(true, Ordering::SeqCst);
            return Err(ThreadNotCreated("thread: null".into()).into());
        }
        Ok(())
    }

    fn submit_review(
        &self,
        _pr: &PullRequestUrl,
        verdict: ReviewVerdict,
        body: &str,
    ) -> anyhow::Result<()> {
        self.sent
            .push(format!("github: submit {} {body:?}", verdict.github_name()));
        if self.cannot_reach_github.load(Ordering::SeqCst) {
            anyhow::bail!("network is down");
        }
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
                .map(|turn| turn.lines().map(str::to_string).collect())
                .collect(),
        }
    }
}

impl AgentPort for FakeAgent {
    fn ask(&mut self, message: &str, on_line: &mut dyn FnMut(&str)) -> anyhow::Result<()> {
        self.sent.push(format!("agent: {message}"));
        let turn = self.turns.pop_front().unwrap_or_default();
        turn.iter().for_each(|line| on_line(line));
        Ok(())
    }

    fn turn_stopper(&self) -> Arc<dyn TurnStopper> {
        Arc::new(FakeTurnStopper(self.sent.clone()))
    }
}

struct FakeTurnStopper(SentLog);

impl TurnStopper for FakeTurnStopper {
    fn stop_turn(&self) -> anyhow::Result<()> {
        self.0.push("agent: stop the turn".to_string());
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
