//! Ask why on a finding: to the review agent while it lives, on the finding's thread; else in the Ask
//! panel, on the finding's lines with the finding told.

use std::fmt::Write as _;
use std::time::Instant;

use super::finding_outcome::FindingRefusal;
use super::{Review, ReviewCore};
use crate::config::AskSettings;
use crate::findings::{Finding, FindingId};
use crate::prompt_safety::quoted;
use crate::viewer::ai_review_run::ReviewAgentLife;
use crate::viewer::ask_desk::AskProcesses;
use crate::viewer::ask_target::AskTarget;
use crate::viewer::finding_threads::{
    FindingThread, WhyDeliveryOutcome, WhyDeliveryRequest, WhyReply, WhyState,
};
use crate::viewer::tour_agent::AgentStatus;

/// Where a question about a finding went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WhyAsked {
    /// Onto the finding's thread, for the review agent that reported it.
    ToReviewAgent,
    /// The review agent is gone: the Ask panel took it, on the finding's place.
    InAskPanel,
}

/// What Ask why asks when the reviewer typed nothing.
pub const DEFAULT_WHY_QUESTION: &str = "Why is this a finding? What did you see in the code?";

impl Review {
    pub fn ask_why(
        &mut self,
        id: &FindingId,
        question: &str,
        settings: &AskSettings,
        now: Instant,
        processes: &mut dyn AskProcesses,
    ) -> Result<WhyAsked, FindingRefusal> {
        let finding = self.open_finding(id)?;
        let question = match question.trim() {
            "" => DEFAULT_WHY_QUESTION,
            typed => typed,
        };
        if !self.review_agent_is_alive() {
            self.ask_in_panel(&finding, question, settings, now, processes);
            return Ok(WhyAsked::InAskPanel);
        }
        self.ask_review_agent_why(finding, question)
    }

    /// A fresh Ask on the finding's place, told the finding, takes the question.
    fn ask_in_panel(
        &mut self,
        finding: &Finding,
        question: &str,
        settings: &AskSettings,
        now: Instant,
        processes: &mut dyn AskProcesses,
    ) {
        let place = self.ask_place_of_finding(finding);
        let label = place
            .lines_label()
            .unwrap_or_else(|| finding.anchor.to_string());
        let clone = self.asks().clone_folder().cloned();
        let mut target = self.ask_target(place, label, clone, settings);
        if let AskTarget::Ready { context, .. } = &mut target {
            write!(
                context,
                "\n\nAn AI review of this change reported this finding here ({}):\n{}\n\
                 The reviewer asks about it. Check it against the code: the AI review may be wrong.",
                finding.severity,
                quoted(&finding.body)
            )
            .ok();
        }
        let head = self.head().clone();
        self.check_asks_for_code_changes();
        self.change_asks(|desk| {
            desk.open(target, &head, now, processes);
            desk.send(question, now, processes);
        });
    }
}

impl ReviewCore {
    fn review_agent_is_alive(&self) -> bool {
        self.review_agent.is_alive()
    }

    /// The question goes onto the finding's thread, for the review agent that reported it.
    fn ask_review_agent_why(
        &mut self,
        finding: Finding,
        question: &str,
    ) -> Result<WhyAsked, FindingRefusal> {
        if self.state.finding_threads().is_asking(&finding.id) {
            return Err(FindingRefusal::BeingAsked);
        }
        self.state
            .record(|state| state.finding_threads_mut().ask(finding, question))
            .map_err(|error| FindingRefusal::NotKept(format!("{error:#}")))?;
        self.place_threads();
        Ok(WhyAsked::ToReviewAgent)
    }

    /// The questions that wait, as one message, once the review agent is idle.
    pub fn why_delivery(&self) -> Option<WhyDeliveryRequest> {
        if self.review_agent != ReviewAgentLife::Alive(Some(AgentStatus::Idle)) {
            return None;
        }
        self.state.finding_threads().delivery_request(&self.subject)
    }

    pub fn why_delivered(&mut self, outcome: WhyDeliveryOutcome) {
        if let Err(error) = self
            .state
            .record(|state| state.finding_threads_mut().apply_delivery(outcome))
        {
            eprintln!("could not keep where the questions on findings are: {error:#}");
        }
        self.place_threads();
    }

    /// The review agent's word on a question, from `agent-reply`.
    pub fn why_replied(&mut self, id: &FindingId, reply: &WhyReply) -> anyhow::Result<()> {
        self.state.record(|state| state.why_replied(id, reply))??;
        self.place_threads();
        Ok(())
    }

    pub fn finding_thread(&self, id: &FindingId) -> Option<&FindingThread> {
        self.state.finding_threads().thread_of(id)
    }

    pub fn why_state_of(&self, id: &FindingId) -> Option<&WhyState> {
        self.finding_thread(id).map(FindingThread::state)
    }
}
