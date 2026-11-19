use super::ReviewCore;
use crate::app_folders::AppFolders;
use crate::viewer::ai_review::AiReview;
use crate::viewer::ai_review_run::{
    AiReviewAction, AiReviewRefusal, AiReviewRun, AiReviewStage, ReviewAgentLife,
};
use crate::viewer::finding_threads::FindingThreads;
use crate::viewer::tour_agent::{AgentStatus, TerminalId};

impl ReviewCore {
    pub fn ai_review_run(&self) -> &AiReviewRun {
        &self.ai_review_run
    }

    /// `None` when Run AI review, or Try again after a failure, can start a run now.
    pub fn run_ai_review_refusal(&self) -> Option<AiReviewRefusal> {
        if self.ai_review_run.is_on() {
            return Some(AiReviewRefusal::AlreadyRunning);
        }
        self.agent_link_is_off
            .then_some(AiReviewRefusal::AgentLinkOff)
    }

    /// What the sidebar's AI review row says: the run while there is one, else what the findings file says.
    pub fn ai_review_stage(&self) -> AiReviewStage {
        let refusal = self.run_ai_review_refusal();
        let has_findings = self.has_findings();
        let action_if_free = |action| refusal.is_none().then_some(action);
        match &self.ai_review_run {
            AiReviewRun::Starting => AiReviewStage {
                line: "Starting the review agent...".to_owned(),
                is_failure: false,
                action: None,
                can_show_agent: false,
                has_findings,
            },
            AiReviewRun::Running(running) => AiReviewStage {
                line: match running.status {
                    Some(AgentStatus::WaitingForYou) => "The review agent waits for you".to_owned(),
                    _ => "Reviewing...".to_owned(),
                },
                is_failure: false,
                action: None,
                can_show_agent: true,
                has_findings,
            },
            AiReviewRun::Failed { reason, agent } => AiReviewStage {
                line: reason.clone(),
                is_failure: true,
                action: action_if_free(AiReviewAction::TryAgain),
                can_show_agent: agent.is_some(),
                has_findings,
            },
            AiReviewRun::NotRun => AiReviewStage {
                line: self.line_with_fixes(),
                is_failure: matches!(self.ai_review, AiReview::Unreadable(_)),
                action: action_if_free(match self.ai_review {
                    AiReview::NotRun => AiReviewAction::Run,
                    AiReview::Findings(_) | AiReview::Unreadable(_) => AiReviewAction::RunAgain,
                }),
                can_show_agent: self.review_agent.is_alive(),
                has_findings,
            },
        }
    }

    /// The findings' count, and the commits of the fixes the tour agent made.
    fn line_with_fixes(&self) -> String {
        let commits: Vec<&str> = self.state.fix_requests().fixed_commits().collect();
        let line = self.ai_review.line();
        match commits.len() {
            0 => line,
            1 => format!("{line} · 1 fix: {}", commits.join(", ")),
            fixes => format!("{line} · {fixes} fixes: {}", commits.join(", ")),
        }
    }

    pub fn has_findings(&self) -> bool {
        self.ai_review.counts().total() > 0
    }

    /// Run AI review and Try again: the run starts, and the app makes the worktree and starts the agent.
    pub fn ask_to_run_ai_review(&mut self) -> Result<(), AiReviewRefusal> {
        if let Some(refusal) = self.run_ai_review_refusal() {
            return Err(refusal);
        }
        self.ai_review_run = AiReviewRun::starting();
        self.keep_ai_run_open(true);
        Ok(())
    }

    pub fn ai_review_agent_started(&mut self, started: anyhow::Result<TerminalId>) {
        if started.is_ok() {
            self.review_agent = ReviewAgentLife::Alive(None);
        }
        self.ai_review_run.started(started);
    }

    /// An agent left idle after its findings were read waits to be talked to: it is no run to fail.
    pub fn ai_review_agent_resumed(&mut self, terminal: TerminalId, status: Option<AgentStatus>) {
        self.review_agent = ReviewAgentLife::Alive(status);
        let left_idle_after_findings =
            status == Some(AgentStatus::Idle) && !self.state.ai_run_is_open();
        if left_idle_after_findings {
            return;
        }
        self.ai_review_run.resumed(terminal, status);
    }

    pub fn ai_review_agent_status(&mut self, status: AgentStatus) {
        self.review_agent = ReviewAgentLife::Alive(Some(status));
        self.ai_review_run.status_is(status);
        self.questions_hear(|threads| threads.agent_status_is(status));
    }

    pub fn ai_review_agent_screen(&mut self, lines: Vec<String>) {
        self.ai_review_run.screen_is(lines);
    }

    /// Herdr no longer follows the review agent: its terminal ended.
    pub fn ai_review_agent_gone(&mut self) {
        self.review_agent = ReviewAgentLife::Gone;
        self.ai_review_run.gone();
        self.questions_hear(FindingThreads::agent_gone);
    }

    /// The review agent said it is done: its findings are read, and it has no run open.
    pub fn findings_done(&mut self, folders: &AppFolders) {
        self.read_findings(folders);
        if matches!(self.ai_review, AiReview::Findings(_)) {
            self.keep_ai_run_open(false);
        }
    }

    fn keep_ai_run_open(&mut self, open: bool) {
        if let Err(error) = self.state.record(|state| state.ai_run_is(open)) {
            eprintln!("could not keep whether the review agent is still at work: {error:#}");
        }
    }

    /// Kept only when a question moved, so a status change with nothing asked writes nothing.
    fn questions_hear(&mut self, news: impl FnOnce(&mut FindingThreads) -> bool) {
        if self.state.finding_threads().is_empty() {
            return;
        }
        let mut threads = self.state.finding_threads().clone();
        if !news(&mut threads) {
            return;
        }
        if let Err(error) = self
            .state
            .record(|state| *state.finding_threads_mut() = threads)
        {
            eprintln!("could not keep where the questions on findings are: {error:#}");
        }
        self.place_threads();
    }

    /// The findings file was read again: a run that was waiting for it is over, with findings or a reason.
    pub(in crate::viewer) fn run_ends_with(&mut self, read: &AiReview) {
        match read {
            AiReview::Findings(_) => self.ai_review_run.findings_were_read(),
            AiReview::Unreadable(reason) => self.ai_review_run.findings_cannot_be_read(reason),
            AiReview::NotRun => {}
        }
    }
}
