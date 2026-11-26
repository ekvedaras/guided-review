//! A PR review's PR as the host has it, and what the reviewer asked to change of it: the draft state
//! (`D`) and merging (`Shift+M`), each sent only on the same press again. A local review has none.

use std::time::{Duration, Instant};

use super::capability::{Capability, HostCapabilities};
use super::jump::PaletteAction;
use super::merging::{AutoMerge, MergeChange, MergeOffer, Merging};
use super::ports::HostError;
use super::pr_status::{CHECKS_RECHECK, Checks, PrStatus, Readiness};
use super::press_again::{PressAgain, PressStep};
use super::problems::Problem;
use crate::keys::Command;
use crate::merge_method::MergeMethod;
use crate::review_subject::ReviewSubject;

#[derive(Debug)]
pub(super) struct PrControls {
    /// `None` until the host answers; kept as it was while a read fails.
    status: Option<PrStatus>,
    readiness: PressAgain<Readiness>,
    /// `None` on a host that has no merging: nothing offers it.
    merging: Option<PressAgain<MergeChange>>,
    /// How the PR would merge, as the host's merging port last said; `None` until it answers, and
    /// always on a host that has no merging.
    merging_state: Option<Merging>,
}

impl PrControls {
    pub(super) fn of(subject: &ReviewSubject, host: HostCapabilities) -> Option<Self> {
        subject.pull_request().map(|_| Self {
            status: None,
            readiness: PressAgain::default(),
            merging: host.has(Capability::Merging).then(PressAgain::default),
            merging_state: None,
        })
    }

    /// A rebuilt review goes on with what the host said and what the reviewer pressed. Whether it
    /// offers merging is its own build's, which read the host's capabilities last.
    pub(super) fn carry_on_from(&mut self, replaced: Self) {
        let merging = self
            .merging
            .take()
            .map(|fresh| replaced.merging.unwrap_or(fresh));
        let merging_state = merging.as_ref().and(replaced.merging_state);
        *self = Self {
            merging,
            merging_state,
            ..replaced
        };
    }

    pub(super) fn status(&self) -> Option<PrStatus> {
        self.status.clone()
    }

    /// How long until the status is read again: only while its checks run.
    pub(super) fn status_is(&mut self, status: PrStatus) -> Option<Duration> {
        let read_again = status
            .open()
            .is_some_and(|open| open.checks == Checks::Running)
            .then_some(CHECKS_RECHECK);
        self.status = Some(status);
        read_again
    }

    pub(super) fn merging_is(&mut self, merging: Merging) {
        if self.merging.is_some() {
            self.merging_state = Some(merging);
        }
    }

    pub(super) fn merging(&self) -> Option<Merging> {
        self.status.as_ref()?.open()?;
        self.merging_state
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
    /// a confirmed change is on its way to the host.
    pub(super) fn ask_to_change_merging(
        &mut self,
        now: Instant,
        preferred: MergeMethod,
    ) -> PressStep {
        let state = self.merging();
        let (Some(state), Some(merging)) = (state, self.merging.as_mut()) else {
            return PressStep::Nothing;
        };
        if merging.to_send().is_some() {
            return PressStep::Nothing;
        }
        let offered = state
            .change(preferred)
            .map_err(|refusal| refusal.to_string());
        merging.press(
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
        let merging = self
            .merging()
            .and_then(|merging| merging.offer().ok())
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
        if let Some(merging) = &mut self.merging {
            merging.stage_to_send(change);
        }
    }

    pub(super) fn merging_to_send(&self) -> Option<MergeChange> {
        self.merging.as_ref()?.to_send()
    }

    /// A change the host took shows at once, until the host is read back; one it refused is a problem.
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

    /// A change the host took shows at once, until the host is read back; one it refused is a problem.
    pub(super) fn merging_landed(
        &mut self,
        change: MergeChange,
        result: Result<(), HostError>,
    ) -> Result<(), Problem> {
        if let Some(merging) = &mut self.merging {
            merging.landed(change);
        }
        result.map_err(|error| Problem::MergingNotChanged(change, error.short_reason()))?;
        let auto_merge = match change {
            MergeChange::EnableAutoMerge(method) => AutoMerge::On(method),
            MergeChange::DisableAutoMerge => AutoMerge::Off,
            MergeChange::MergeNow(_) => {
                self.status = Some(PrStatus::Merged);
                return Ok(());
            }
        };
        if let Some(state) = &mut self.merging_state {
            state.auto_merge = auto_merge;
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
