use std::time::Instant;

use super::ReviewCore;
use crate::config::Config;
use crate::merge_method::MergeMethod;
use crate::viewer::host_outage::Affected;
use crate::viewer::host_outage::Read;
use crate::viewer::merging::{MergeChange, Merging};
use crate::viewer::ports::HostError;
use crate::viewer::pr_status::{PrStatus, Readiness};
use crate::viewer::press_again::PressStep;
use crate::viewer::problems::{Problem, ProblemKind};
use crate::viewer::work_agenda::WorkKind;

impl ReviewCore {
    /// `None` until GitHub has been read, and for a review with no PR.
    pub fn pr_status(&self) -> Option<PrStatus> {
        self.pr_controls.as_ref()?.status()
    }

    pub fn pr_status_is(&mut self, status: PrStatus) {
        let Some(controls) = &mut self.pr_controls else {
            return;
        };
        let read_again = controls.status_is(status);
        self.clear_problem(ProblemKind::NotRead(Read::PrStatus));
        self.host_answered(Affected::PrStatus);
        if let Some(wait) = read_again {
            self.work.want_in(WorkKind::PullPrStatus, wait);
        }
    }

    /// How the open PR would merge, as the host's merging port says.
    pub fn pr_merging_is(&mut self, merging: Merging) {
        if let Some(controls) = &mut self.pr_controls {
            controls.merging_is(merging);
        }
    }

    /// What the title bar's merge button acts on: `None` for a PR not open or not read yet, and on a
    /// host that has no merging.
    pub fn merging(&self) -> Option<Merging> {
        self.pr_controls.as_ref()?.merging()
    }

    /// Settings name a merge method per repo; `Shift+M` uses the one of this PR's repo.
    pub fn read_merge_method_from(&mut self, config: &Config) {
        if let Some(pr) = self.subject.pull_request() {
            self.merge_method = config.merge_method(&pr.hosted_repo());
        }
    }

    /// A merge commit for a review with no PR, which has nothing to merge.
    pub fn merge_method(&self) -> MergeMethod {
        self.merge_method
    }

    /// The merge method `Shift+M` uses, as if Settings named it.
    pub fn prefer_merge_method(&mut self, method: MergeMethod) {
        self.merge_method = method;
    }

    /// `D`: the first press says what the same press again does; that press, within ten seconds,
    /// converts the PR to a draft or marks it ready.
    pub fn ask_to_change_readiness(&mut self, now: Instant) -> PressStep {
        let Some(controls) = &mut self.pr_controls else {
            return PressStep::Nothing;
        };
        let step = controls.ask_to_change_readiness(now);
        if step == PressStep::Sent {
            self.work.want(WorkKind::ChangePrReadiness);
        }
        step
    }

    /// `Shift+M`: the first press says what the same press again does, naming the merge method;
    /// that press, within ten seconds, turns auto-merge on or off, or merges the PR now.
    pub fn ask_to_change_merging(&mut self, now: Instant) -> PressStep {
        let Some(controls) = &mut self.pr_controls else {
            return PressStep::Nothing;
        };
        let step = controls.ask_to_change_merging(now, self.merge_method);
        if step == PressStep::Sent {
            self.work.want(WorkKind::ChangeMerging);
        }
        step
    }

    /// The confirmed change GitHub has not answered yet.
    pub fn merge_in_flight(&self) -> Option<MergeChange> {
        self.merging_to_send()
    }

    /// The title bar of a snapshot scene as if a merge change were sent and GitHub has not answered.
    #[cfg(feature = "snapshots")]
    pub fn stage_merge_in_flight(&mut self, change: Option<MergeChange>) {
        if let Some(controls) = &mut self.pr_controls {
            controls.stage_merging_to_send(change);
        }
    }

    pub(super) fn readiness_to_send(&self) -> Option<Readiness> {
        self.pr_controls.as_ref()?.readiness_to_send()
    }

    pub(super) fn merging_to_send(&self) -> Option<MergeChange> {
        self.pr_controls.as_ref()?.merging_to_send()
    }

    pub(super) fn readiness_landed(&mut self, readiness: Readiness, result: Result<(), HostError>) {
        if let Some(controls) = &mut self.pr_controls {
            let landed = controls.readiness_landed(readiness, result);
            self.pr_change_landed(ProblemKind::ReadinessNotChanged, landed);
        }
    }

    pub(super) fn merging_landed(&mut self, change: MergeChange, result: Result<(), HostError>) {
        if let Some(controls) = &mut self.pr_controls {
            let landed = controls.merging_landed(change, result);
            self.pr_change_landed(ProblemKind::MergingNotChanged, landed);
        }
    }

    /// A change GitHub took is read back, so the title bar shows what GitHub shows, checks and all.
    fn pr_change_landed(&mut self, kind: ProblemKind, landed: Result<(), Problem>) {
        match landed {
            Ok(()) => {
                self.clear_problem(kind);
                self.work.want(WorkKind::PullPrStatus);
            }
            Err(problem) => self.raise_problem(problem),
        }
    }
}
