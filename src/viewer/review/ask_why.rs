//! Ask why on a finding: to the review agent while it lives, on the finding's thread.

use super::finding_outcome::FindingRefusal;
use super::{Review, ReviewCore};
use crate::findings::{Finding, FindingId};
use crate::viewer::ai_review_run::ReviewAgentLife;
use crate::viewer::comments::{CommentPlace, CommentRefusal, SendMode};
use crate::viewer::finding_threads::{
    FindingThread, WhyDeliveryOutcome, WhyDeliveryRequest, WhyReply, WhyState,
};
use crate::viewer::tour_agent::AgentStatus;

/// Where a question about a finding went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WhyAsked {
    /// Onto the finding's thread, for the review agent that reported it.
    ToReviewAgent,
    /// The review agent is gone: the question waits as a private question to the tour agent in a comment
    /// box on the finding's lines, which the caller sends.
    ToTourAgent(CommentPlace),
}

/// What Ask why asks when the reviewer typed nothing.
pub const DEFAULT_WHY_QUESTION: &str = "Why is this a finding? What did you see in the code?";

impl Review {
    pub fn ask_why(&mut self, id: &FindingId, question: &str) -> Result<WhyAsked, FindingRefusal> {
        let finding = self.open_finding(id)?;
        let question = match question.trim() {
            "" => DEFAULT_WHY_QUESTION,
            typed => typed,
        };
        if !self.review_agent_is_alive() {
            return self.ask_tour_agent_why(&finding, question);
        }
        self.ask_review_agent_why(finding, question)
    }
}

impl ReviewCore {
    /// The review agent is gone, so the question becomes a private question to the tour agent on the
    /// finding's lines: a comment box holds it, with the finding quoted, and the caller sends it.
    fn ask_tour_agent_why(
        &mut self,
        finding: &Finding,
        question: &str,
    ) -> Result<WhyAsked, FindingRefusal> {
        if let Some(refusal) = self.send_refusal(SendMode::SendNow) {
            return Err(FindingRefusal::Comment(refusal));
        }
        let place = self
            .comment_place_of(&finding.anchor)
            .map_err(|refusal| match refusal {
                FindingRefusal::Comment(CommentRefusal::NoLines) => FindingRefusal::NoReviewAgent,
                other => other,
            })?;
        if self.state.draft_at(&place).is_some() {
            return Err(FindingRefusal::DraftInTheWay(place));
        }
        let quoted_finding: Vec<String> = finding
            .body
            .lines()
            .map(|line| format!("> {line}"))
            .collect();
        let text = format!(
            "{question}\n\nAI review finding ({}):\n{}",
            finding.severity,
            quoted_finding.join("\n")
        );
        self.comment_desk
            .add_text_at(&mut self.state, &place, &self.head, &text, &[])
            .map_err(FindingRefusal::Comment)?;
        Ok(WhyAsked::ToTourAgent(place))
    }

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
