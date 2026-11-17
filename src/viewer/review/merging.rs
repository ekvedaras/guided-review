use std::time::Instant;

use super::ReviewCore;
use crate::viewer::jump::PaletteAction;
use crate::viewer::merging::{AutoMerge, MergeChange, MergeMethod, MergeOffer, Mergeability};
use crate::viewer::ports::GitHubError;
use crate::viewer::press_again::PressStep;
use crate::viewer::problems::{Problem, ProblemKind};
use crate::viewer::shortcuts::Command;
use crate::viewer::work_agenda::WorkKind;

impl ReviewCore {
    /// The merge method Settings names for this PR's repo.
    pub fn prefer_merge_method(&mut self, method: MergeMethod) {
        self.merge_method = method;
    }

    /// `Shift+M`: the first press says what the same press again does, naming the merge method;
    /// that press, within ten seconds, turns auto-merge on or off, or merges the PR now.
    pub fn ask_to_change_merging(&mut self, now: Instant) -> PressStep {
        let (Some(status), Some(_)) = (self.pr_status.as_ref(), self.subject.pull_request()) else {
            return PressStep::Nothing;
        };
        let change = match status.merging.change(self.merge_method) {
            Ok(change) => change,
            Err(refusal) => return PressStep::Refused(refusal.to_string()),
        };
        if self.merging_asked.confirms(change, now) {
            self.merging_to_send = Some(change);
            self.work.want(WorkKind::ChangeMerging);
            return PressStep::Sent;
        }
        let does = match change {
            MergeChange::EnableAutoMerge(method) => {
                format!("turn on auto-merge ({})", self.method_words(method))
            }
            MergeChange::DisableAutoMerge => "turn off auto-merge".to_owned(),
            MergeChange::MergeNow(method) => format!(
                "merge the PR now, as nothing blocks it ({})",
                self.method_words(method)
            ),
        };
        PressStep::ask(Command::ToggleAutoMerge, &does)
    }

    /// The method used, and why when it is not the reviewer's.
    fn method_words(&self, used: MergeMethod) -> String {
        if used == self.merge_method {
            used.label().to_owned()
        } else {
            format!(
                "{}: the repo does not allow {}",
                used.label(),
                self.merge_method.label()
            )
        }
    }

    /// Enable or Disable auto-merge, or Merge now: the one that applies, where the viewer may.
    pub(in crate::viewer) fn merging_palette_action(&self) -> Option<PaletteAction> {
        Some(match self.pr_status.as_ref()?.merging.offer().ok()? {
            MergeOffer::EnableAutoMerge => PaletteAction::EnableAutoMerge,
            MergeOffer::DisableAutoMerge => PaletteAction::DisableAutoMerge,
            MergeOffer::MergeNow => PaletteAction::MergeNow,
        })
    }

    pub(super) fn merging_to_send(&self) -> Option<MergeChange> {
        self.merging_to_send
    }

    /// A change GitHub took is read back, so the title bar shows what GitHub shows.
    pub(super) fn merging_landed(&mut self, change: MergeChange, result: Result<(), GitHubError>) {
        if self.merging_to_send == Some(change) {
            self.merging_to_send = None;
        }
        match result {
            Ok(()) => {
                self.clear_problem(ProblemKind::MergingNotChanged);
                if let Some(status) = &mut self.pr_status {
                    let merging = &mut status.merging;
                    match change {
                        MergeChange::EnableAutoMerge(method) => {
                            merging.auto_merge = AutoMerge::On(method);
                        }
                        MergeChange::DisableAutoMerge => merging.auto_merge = AutoMerge::Off,
                        MergeChange::MergeNow(_) => merging.mergeability = Mergeability::NotOpen,
                    }
                }
                self.work.want(WorkKind::PullPrStatus);
            }
            Err(error) => {
                self.raise_problem(Problem::MergingNotChanged(change, error.short_reason()));
            }
        }
    }
}
