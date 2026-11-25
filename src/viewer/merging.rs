//! What GitHub says of merging the PR: whether auto-merge is on, whether anything blocks a merge,
//! the merge methods the repo allows and what the viewer may do. From it, the one thing the
//! auto-merge key offers: turn auto-merge on or off, or merge now when nothing blocks the PR.

use std::fmt;

use crate::merge_method::MergeMethod;

/// The merge methods a repo allows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MergeMethods {
    merge: bool,
    squash: bool,
    rebase: bool,
}

impl MergeMethods {
    pub fn only(allowed: &[MergeMethod]) -> Self {
        Self {
            merge: allowed.contains(&MergeMethod::Merge),
            squash: allowed.contains(&MergeMethod::Squash),
            rebase: allowed.contains(&MergeMethod::Rebase),
        }
    }

    pub fn allows(self, method: MergeMethod) -> bool {
        match method {
            MergeMethod::Merge => self.merge,
            MergeMethod::Squash => self.squash,
            MergeMethod::Rebase => self.rebase,
        }
    }

    /// The preferred method where the repo allows it, else the repo's first allowed one.
    pub fn pick(self, preferred: MergeMethod) -> MergeMethod {
        if self.allows(preferred) {
            return preferred;
        }
        MergeMethod::ALL
            .into_iter()
            .find(|method| self.allows(*method))
            .unwrap_or(preferred)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoMerge {
    Off,
    /// GitHub merges by itself, with this method, once nothing blocks the PR.
    On(MergeMethod),
}

/// Whether a merge of the open PR would go through now: GitHub's `mergeStateStatus`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mergeability {
    /// Nothing blocks it, so GitHub refuses auto-merge and takes a merge at once.
    Now,
    /// Checks, reviews, rules, conflicts or a draft hold it back, or GitHub has not worked it out yet.
    Blocked,
}

impl Mergeability {
    /// `UNSTABLE` (checks that are not required fail) and `HAS_HOOKS` merge like `CLEAN`.
    pub fn from_github(merge_state_status: &str) -> Self {
        match merge_state_status {
            "CLEAN" | "UNSTABLE" | "HAS_HOOKS" => Self::Now,
            _ => Self::Blocked,
        }
    }
}

/// What GitHub says of merging the open PR, and what the viewer may do about it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Merging {
    pub mergeability: Mergeability,
    pub auto_merge: AutoMerge,
    pub methods: MergeMethods,
    /// The repo's "Allow auto-merge" setting, which only says why auto-merge is not offered.
    pub repo_allows_auto_merge: bool,
    pub viewer_may_enable_auto_merge: bool,
    pub viewer_may_disable_auto_merge: bool,
    /// Write access to the repo.
    pub viewer_may_merge: bool,
}

/// What the auto-merge key and button offer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeOffer {
    EnableAutoMerge,
    DisableAutoMerge,
    MergeNow,
}

/// Why the key does nothing on this PR.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(
    clippy::enum_variant_names,
    reason = "each names what GitHub refuses, and each refused thing happens to be a merge"
)]
pub enum MergeRefusal {
    MayNotDisableAutoMerge,
    MayNotMerge,
    RepoDisallowsAutoMerge,
    MayNotEnableAutoMerge,
}

impl fmt::Display for MergeRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str(match self {
            Self::MayNotDisableAutoMerge => "GitHub does not let you turn off this PR's auto-merge",
            Self::MayNotMerge => "Only people with write access can merge the PR",
            Self::RepoDisallowsAutoMerge => "The repo does not allow auto-merge",
            Self::MayNotEnableAutoMerge => "GitHub does not let you turn on auto-merge for this PR",
        })
    }
}

/// What the confirmed press asks GitHub to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeChange {
    EnableAutoMerge(MergeMethod),
    DisableAutoMerge,
    MergeNow(MergeMethod),
}

impl Merging {
    /// Turn auto-merge off while it is on; merge now while nothing blocks the PR; else turn
    /// auto-merge on. Each only where GitHub says the viewer may.
    pub fn offer(self) -> Result<MergeOffer, MergeRefusal> {
        match (self.mergeability, self.auto_merge) {
            (_, AutoMerge::On(_)) if self.viewer_may_disable_auto_merge => {
                Ok(MergeOffer::DisableAutoMerge)
            }
            (_, AutoMerge::On(_)) => Err(MergeRefusal::MayNotDisableAutoMerge),
            (Mergeability::Now, AutoMerge::Off) if self.viewer_may_merge => {
                Ok(MergeOffer::MergeNow)
            }
            (Mergeability::Now, AutoMerge::Off) => Err(MergeRefusal::MayNotMerge),
            (Mergeability::Blocked, AutoMerge::Off) if self.viewer_may_enable_auto_merge => {
                Ok(MergeOffer::EnableAutoMerge)
            }
            (Mergeability::Blocked, AutoMerge::Off) if !self.repo_allows_auto_merge => {
                Err(MergeRefusal::RepoDisallowsAutoMerge)
            }
            (Mergeability::Blocked, AutoMerge::Off) => Err(MergeRefusal::MayNotEnableAutoMerge),
        }
    }

    /// The title bar button's words; `None` hides it, as the viewer cannot act.
    pub fn button_label(self) -> Option<&'static str> {
        Some(match self.offer().ok()? {
            MergeOffer::EnableAutoMerge => "Auto-merge",
            MergeOffer::DisableAutoMerge => "Auto-merge on",
            MergeOffer::MergeNow => "Merge now",
        })
    }

    /// The offer with the method it would use: the preferred one where the repo allows it.
    pub fn change(self, preferred: MergeMethod) -> Result<MergeChange, MergeRefusal> {
        let method = self.methods.pick(preferred);
        Ok(match self.offer()? {
            MergeOffer::EnableAutoMerge => MergeChange::EnableAutoMerge(method),
            MergeOffer::DisableAutoMerge => MergeChange::DisableAutoMerge,
            MergeOffer::MergeNow => MergeChange::MergeNow(method),
        })
    }
}
