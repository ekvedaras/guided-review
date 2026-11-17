use super::ReviewCore;
use crate::viewer::github_outage::Affected;
use crate::viewer::pr_status::{CHECKS_RECHECK, Checks, PrStatus};
use crate::viewer::problems::{ProblemKind, Read};
use crate::viewer::work_agenda::WorkKind;

impl ReviewCore {
    /// `None` until GitHub has been read, and for a review with no PR.
    pub fn pr_status(&self) -> Option<&PrStatus> {
        self.pr_status.as_ref()
    }

    pub fn pr_status_is(&mut self, status: PrStatus) {
        self.clear_problem(ProblemKind::NotRead(Read::PrStatus));
        self.github_answered(Affected::PrStatus);
        if status.checks == Checks::Running {
            self.work.want_in(WorkKind::PullPrStatus, CHECKS_RECHECK);
        }
        self.pr_status = Some(status);
    }
}
