//! What a snapshot scene shows of the findings' fixes and threads.

use super::ReviewCore;
use crate::findings::Finding;
use crate::viewer::ai_review::AiReview;
use crate::viewer::finding_decisions::Fate;
use crate::viewer::finding_threads::FindingThreads;
use crate::viewer::fix_requests::FixRequests;

impl ReviewCore {
    /// The fixes a snapshot scene shows on the open findings.
    pub fn stage_fix_requests(&mut self) {
        let findings = match &self.ai_review {
            AiReview::Findings(file) => file.findings.clone(),
            _ => Vec::new(),
        };
        self.show_staged_fix_requests(&findings);
    }

    pub fn unstage_fix_requests(&mut self) {
        self.show_staged_fix_requests(&[]);
    }

    fn show_staged_fix_requests(&mut self, findings: &[Finding]) {
        self.record_reporting("the staged fix requests", |state| {
            *state.fix_requests_mut() = FixRequests::staged(findings);
        });
        self.place_threads();
    }

    /// The last open finding, dismissed, as a snapshot scene shows the dismissed section.
    pub fn stage_dismissed_finding(&mut self) {
        let Some(finding) = (match &self.ai_review {
            AiReview::Findings(file) => file.findings.last().cloned(),
            _ => None,
        }) else {
            return;
        };
        let id = finding.id.clone();
        self.record_reporting("the staged dismissed finding", |state| {
            state.decide_finding(finding, Fate::Dismissed);
        });
        self.close_finding(&id);
    }

    /// The Ask why threads a snapshot scene shows on the open findings.
    pub fn stage_finding_threads(&mut self) {
        let findings = match &self.ai_review {
            AiReview::Findings(file) => file.findings.clone(),
            _ => Vec::new(),
        };
        self.show_staged_finding_threads(&findings);
    }

    pub fn unstage_finding_threads(&mut self) {
        self.show_staged_finding_threads(&[]);
    }

    fn show_staged_finding_threads(&mut self, findings: &[Finding]) {
        self.record_reporting("the staged finding threads", |state| {
            *state.finding_threads_mut() = FindingThreads::staged(findings);
        });
        self.place_threads();
    }
}
