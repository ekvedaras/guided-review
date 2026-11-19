use super::ReviewCore;
use crate::viewer::ai_review::AiReview;
use crate::viewer::ai_review_run::{AiReviewRefusal, AiReviewRun};
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

    /// Run AI review and Try again: the run starts, and the app makes the worktree and starts the agent.
    pub fn ask_to_run_ai_review(&mut self) -> Result<(), AiReviewRefusal> {
        if let Some(refusal) = self.run_ai_review_refusal() {
            return Err(refusal);
        }
        self.ai_review_run = AiReviewRun::starting();
        Ok(())
    }

    pub fn ai_review_agent_started(&mut self, started: anyhow::Result<TerminalId>) {
        self.ai_review_run.started(started);
    }

    pub fn ai_review_agent_resumed(&mut self, terminal: TerminalId, status: Option<AgentStatus>) {
        self.ai_review_run.resumed(terminal, status);
    }

    pub fn ai_review_agent_status(&mut self, status: AgentStatus) {
        self.ai_review_run.status_is(status);
    }

    pub fn ai_review_agent_screen(&mut self, lines: Vec<String>) {
        self.ai_review_run.screen_is(lines);
    }

    /// Herdr no longer follows the review agent: its terminal ended.
    pub fn ai_review_agent_gone(&mut self) {
        self.ai_review_run.gone();
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
