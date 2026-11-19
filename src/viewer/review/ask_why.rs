//! Ask why on a finding: to the review agent while it lives, on the finding's thread; else in the Ask
//! panel, on the finding's lines with the finding told.

use std::fmt::Write as _;
use std::time::Instant;

use super::finding_actions::FindingRefusal;
use super::{Review, ReviewCore};
use crate::config::AskSettings;
use crate::findings::{Anchor, AnchorSide, Finding, FindingId};
use crate::viewer::ai_review_run::ReviewAgentLife;
use crate::viewer::ask::{Place, ShownOn};
use crate::viewer::ask_desk::AskProcesses;
use crate::viewer::ask_target::AskTarget;
use crate::viewer::finding_placement::FindingHome;
use crate::viewer::finding_threads::{
    FindingThread, WhyDeliveryOutcome, WhyDeliveryRequest, WhyReply, WhyState,
};
use crate::viewer::step_page::{LineLocation, Side};
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
        if !self.review_agent.is_alive() {
            self.ask_in_panel(&finding, question, settings, now, processes);
            return Ok(WhyAsked::InAskPanel);
        }
        if self.state.finding_threads().is_asking(id) {
            return Err(FindingRefusal::BeingAsked);
        }
        self.state
            .record(|state| state.finding_threads_mut().ask(finding, question))
            .map_err(|error| FindingRefusal::NotKept(format!("{error:#}")))?;
        self.place_threads();
        Ok(WhyAsked::ToReviewAgent)
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
        let clone = self.ask_desk.clone_folder().cloned();
        let mut target = self.ask_target(place, label, clone, settings);
        if let AskTarget::Ready { context, .. } = &mut target {
            write!(
                context,
                "\n\nAn AI review of this change reported this finding here ({}):\n{}\n\
                 The reviewer asks about it. Check it against the code: the AI review may be wrong.",
                finding.severity, finding.body
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

    /// The lines of a finding on the screen that shows it; its step or file for a finding on a file; the
    /// whole tour for one on the whole change.
    pub(super) fn ask_place_of_finding(&self, finding: &Finding) -> Place {
        let home = self.findings.home_of(&finding.id);
        let shown_on = match home {
            Some(FindingHome::Step(step)) => Some(ShownOn::Step { step: step.clone() }),
            Some(FindingHome::SafetyNet) => Some(ShownOn::SafetyNet),
            Some(FindingHome::File(path)) => Some(ShownOn::File { path: path.clone() }),
            Some(FindingHome::Change) | None => None,
        };
        match (&finding.anchor, shown_on) {
            (Anchor::Lines { file, side, lines }, Some(shown_on)) => {
                let at = |line| LineLocation {
                    path: file.clone(),
                    side: match side {
                        AnchorSide::Base => Side::Base,
                        AnchorSide::Head => Side::Head,
                    },
                    line,
                };
                let (start, end) = (at(lines.start), at(lines.end));
                let pieces = self
                    .pieces_shown_on(shown_on.clone())
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|piece| {
                        piece.lines().any(|line| {
                            line.location.path == start.path
                                && line.location.side == start.side
                                && (start.line..=end.line).contains(&line.location.line)
                        })
                    })
                    .map(|piece| piece.id)
                    .collect();
                Place::Lines {
                    shown_on,
                    start,
                    end,
                    pieces,
                }
            }
            (Anchor::File { .. }, Some(ShownOn::Step { step })) => Place::Step { step },
            (Anchor::File { file }, Some(ShownOn::File { .. })) => {
                Place::File { path: file.clone() }
            }
            _ => Place::Tour,
        }
    }

    /// The Ask why threads a snapshot scene shows on the open findings.
    #[cfg(feature = "snapshots")]
    pub fn stage_finding_threads(&mut self) {
        let findings = match &self.ai_review {
            crate::viewer::ai_review::AiReview::Findings(file) => file.findings.clone(),
            _ => Vec::new(),
        };
        self.show_staged_finding_threads(&findings);
    }

    #[cfg(feature = "snapshots")]
    pub fn unstage_finding_threads(&mut self) {
        self.show_staged_finding_threads(&[]);
    }

    #[cfg(feature = "snapshots")]
    fn show_staged_finding_threads(&mut self, findings: &[Finding]) {
        self.record_reporting("the staged finding threads", |state| {
            *state.finding_threads_mut() =
                crate::viewer::finding_threads::FindingThreads::staged(findings);
        });
        self.place_threads();
    }

    pub fn finding_thread(&self, id: &FindingId) -> Option<&FindingThread> {
        self.state.finding_threads().thread_of(id)
    }

    pub fn why_state_of(&self, id: &FindingId) -> Option<&WhyState> {
        self.finding_thread(id).map(FindingThread::state)
    }
}
