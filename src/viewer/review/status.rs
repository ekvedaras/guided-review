use super::ReviewCore;
use crate::viewer::pr_status::PrStatus;

impl ReviewCore {
    /// `None` until GitHub has been read, and for a review with no PR.
    pub fn pr_status(&self) -> Option<&PrStatus> {
        self.pr_status.as_ref()
    }

    pub fn pr_status_is(&mut self, status: PrStatus) {
        self.pr_status = Some(status);
    }
}
