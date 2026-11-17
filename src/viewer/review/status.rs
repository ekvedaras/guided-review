use super::ReviewCore;
use crate::viewer::github_outage::Affected;
use crate::viewer::pr_status::PrStatus;
use crate::viewer::problems::{ProblemKind, Read};

impl ReviewCore {
    /// `None` until GitHub has been read, and for a review with no PR.
    pub fn pr_status(&self) -> Option<&PrStatus> {
        self.pr_status.as_ref()
    }

    pub fn pr_status_is(&mut self, status: PrStatus) {
        self.clear_problem(ProblemKind::NotRead(Read::PrStatus));
        self.github_answered(Affected::PrStatus);
        self.pr_status = Some(status);
    }
}
