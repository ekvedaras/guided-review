//! A PR review's PR as GitHub has it, and what the reviewer asked to change of it: the draft state
//! (`D`) and merging (`Shift+M`), each sent only on the same press again. A local review has none.

use std::time::Instant;

use super::jump::PaletteAction;
use super::merging::{AutoMerge, MergeChange, MergeOffer};
use super::ports::HostError;
use super::pr_status::{PrStatus, Readiness};
use super::press_again::{PressAgain, PressStep};
use super::problems::Problem;
use crate::keys::Command;
use crate::merge_method::MergeMethod;
use crate::review_subject::ReviewSubject;

#[derive(Debug, Default)]
pub(super) struct PrControls {
    /// `None` until GitHub answers; kept as it was while a read fails.
    status: Option<PrStatus>,
    /// The reviewer's merge method for this PR's repo, from Settings.
    merge_method: MergeMethod,
    readiness: PressAgain<Readiness>,
    merging: PressAgain<MergeChange>,
}

impl PrControls {
    pub(super) fn of(subject: &ReviewSubject) -> Option<Self> {
        subject.pull_request().map(|_| Self::default())
    }

    /// A rebuilt review goes on with what GitHub said and what the reviewer pressed; its merge
    /// method is what the config says now.
    pub(super) fn carry_on_from(&mut self, replaced: Self) {
        *self = Self {
            merge_method: self.merge_method,
            ..replaced
        };
    }

    pub(super) fn status(&self) -> Option<&PrStatus> {
        self.status.as_ref()
    }

    pub(super) fn status_is(&mut self, status: PrStatus) {
        self.status = Some(status);
    }

    pub(super) fn merge_method(&self) -> MergeMethod {
        self.merge_method
    }

    pub(super) fn prefer_merge_method(&mut self, method: MergeMethod) {
        self.merge_method = method;
    }

    /// A merged or closed PR has nothing to change, so the key does nothing.
    pub(super) fn ask_to_change_readiness(&mut self, now: Instant) -> PressStep {
        let Some(status) = self.status.as_ref().and_then(PrStatus::open) else {
            return PressStep::Nothing;
        };
        let offered = if status.viewer_may_change_readiness {
            Ok(status.readiness.other())
        } else {
            Err(
                "Only the author and people with write access can change whether the PR is a draft"
                    .to_owned(),
            )
        };
        self.readiness
            .press(Command::ToggleDraft, offered, now, |target| {
                match target {
                    Readiness::Draft => "convert the PR to a draft",
                    Readiness::Ready => "mark the PR ready for review",
                }
                .to_owned()
            })
    }

    /// A merged or closed PR has nothing to change, so the key does nothing, and nor does it while
    /// a confirmed change is on its way to GitHub.
    pub(super) fn ask_to_change_merging(&mut self, now: Instant) -> PressStep {
        let Some(status) = self.status.as_ref().and_then(PrStatus::open) else {
            return PressStep::Nothing;
        };
        if self.merging.to_send().is_some() {
            return PressStep::Nothing;
        }
        let preferred = self.merge_method;
        let offered = status
            .merging
            .change(preferred)
            .map_err(|refusal| refusal.to_string());
        self.merging.press(
            Command::ToggleAutoMerge,
            offered,
            now,
            |change| match change {
                MergeChange::EnableAutoMerge(method) => {
                    format!("turn on auto-merge ({})", method_words(method, preferred))
                }
                MergeChange::DisableAutoMerge => "turn off auto-merge".to_owned(),
                MergeChange::MergeNow(method) => format!(
                    "merge the PR now, as nothing blocks it ({})",
                    method_words(method, preferred)
                ),
            },
        )
    }

    /// Convert to draft or Mark as ready for review, then Enable or Disable auto-merge or Merge
    /// now: the ones that apply to an open PR, where the viewer may.
    pub(super) fn palette_actions(&self) -> impl Iterator<Item = PaletteAction> + use<> {
        let status = self.status.as_ref().and_then(PrStatus::open);
        let readiness = status
            .filter(|status| status.viewer_may_change_readiness)
            .map(|status| match status.readiness {
                Readiness::Ready => PaletteAction::ConvertToDraft,
                Readiness::Draft => PaletteAction::MarkReadyForReview,
            });
        let merging =
            status
                .and_then(|status| status.merging.offer().ok())
                .map(|offer| match offer {
                    MergeOffer::EnableAutoMerge => PaletteAction::EnableAutoMerge,
                    MergeOffer::DisableAutoMerge => PaletteAction::DisableAutoMerge,
                    MergeOffer::MergeNow => PaletteAction::MergeNow,
                });
        readiness.into_iter().chain(merging)
    }

    pub(super) fn readiness_to_send(&self) -> Option<Readiness> {
        self.readiness.to_send()
    }

    #[cfg(feature = "snapshots")]
    pub(super) fn stage_merging_to_send(&mut self, change: Option<MergeChange>) {
        self.merging.stage_to_send(change);
    }

    pub(super) fn merging_to_send(&self) -> Option<MergeChange> {
        self.merging.to_send()
    }

    /// A change GitHub took shows at once, until GitHub is read back; one it refused is a problem.
    pub(super) fn readiness_landed(
        &mut self,
        readiness: Readiness,
        result: Result<(), HostError>,
    ) -> Result<(), Problem> {
        self.readiness.landed(readiness);
        result.map_err(|error| Problem::ReadinessNotChanged(readiness, error.short_reason()))?;
        if let Some(status) = self.status.as_mut().and_then(PrStatus::open_mut) {
            status.readiness = readiness;
        }
        Ok(())
    }

    /// A change GitHub took shows at once, until GitHub is read back; one it refused is a problem.
    pub(super) fn merging_landed(
        &mut self,
        change: MergeChange,
        result: Result<(), HostError>,
    ) -> Result<(), Problem> {
        self.merging.landed(change);
        result.map_err(|error| Problem::MergingNotChanged(change, error.short_reason()))?;
        let auto_merge = match change {
            MergeChange::EnableAutoMerge(method) => AutoMerge::On(method),
            MergeChange::DisableAutoMerge => AutoMerge::Off,
            MergeChange::MergeNow(_) => {
                self.status = Some(PrStatus::Merged);
                return Ok(());
            }
        };
        if let Some(status) = self.status.as_mut().and_then(PrStatus::open_mut) {
            status.merging.auto_merge = auto_merge;
        }
        Ok(())
    }
}

/// The method used, and why when it is not the reviewer's.
fn method_words(used: MergeMethod, preferred: MergeMethod) -> String {
    if used == preferred {
        used.label().to_owned()
    } else {
        format!(
            "{}: the repo does not allow {}",
            used.label(),
            preferred.label()
        )
    }
}
