use std::time::Instant;

use super::ReviewCore;
use crate::viewer::github_outage::Affected;
use crate::viewer::jump::PaletteAction;
use crate::viewer::ports::GitHubError;
use crate::viewer::pr_status::{CHECKS_RECHECK, Checks, PrStatus, Readiness};
use crate::viewer::press_again::PressStep;
use crate::viewer::problems::{Problem, ProblemKind, Read};
use crate::viewer::shortcuts::Command;
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

    /// `D`: the first press says what the same press again does; that press, within ten seconds,
    /// converts the PR to a draft or marks it ready.
    pub fn ask_to_change_readiness(&mut self, now: Instant) -> PressStep {
        let (Some(status), Some(_)) = (self.pr_status, self.subject.pull_request()) else {
            return PressStep::Nothing;
        };
        if !status.viewer_may_change_readiness {
            return PressStep::Refused(
                "Only the author and people with write access can change whether the PR is a draft"
                    .to_owned(),
            );
        }
        let target = status.readiness.other();
        if self.readiness_asked.confirms(target, now) {
            self.readiness_to_send = Some(target);
            self.work.want(WorkKind::ChangePrReadiness);
            return PressStep::Sent;
        }
        let does = match target {
            Readiness::Draft => "convert the PR to a draft",
            Readiness::Ready => "mark the PR ready for review",
        };
        PressStep::ask(Command::ToggleDraft, does)
    }

    /// Convert to draft or Mark as ready for review: the one that applies, where the viewer may.
    pub(in crate::viewer) fn readiness_palette_action(&self) -> Option<PaletteAction> {
        let status = self
            .pr_status
            .filter(|status| status.viewer_may_change_readiness)?;
        Some(match status.readiness {
            Readiness::Ready => PaletteAction::ConvertToDraft,
            Readiness::Draft => PaletteAction::MarkReadyForReview,
        })
    }

    /// What the confirmed press wants GitHub to be told.
    pub(super) fn readiness_to_send(&self) -> Option<Readiness> {
        self.readiness_to_send
    }

    /// A change GitHub took is read back, so the title bar shows what GitHub shows, checks and all.
    pub(super) fn readiness_landed(
        &mut self,
        readiness: Readiness,
        result: Result<(), GitHubError>,
    ) {
        if self.readiness_to_send == Some(readiness) {
            self.readiness_to_send = None;
        }
        match result {
            Ok(()) => {
                self.clear_problem(ProblemKind::ReadinessNotChanged);
                if let Some(status) = &mut self.pr_status {
                    status.readiness = readiness;
                }
                self.work.want(WorkKind::PullPrStatus);
            }
            Err(error) => {
                self.raise_problem(Problem::ReadinessNotChanged(
                    readiness,
                    error.short_reason(),
                ));
            }
        }
    }
}
