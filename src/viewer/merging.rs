//! What GitHub says of merging the PR: whether auto-merge is on, whether anything blocks a merge,
//! the merge methods the repo allows and what the viewer may do. From it, the one thing the
//! auto-merge key offers: turn auto-merge on or off, or merge now when nothing blocks the PR.

use std::fmt;

/// How a merge lands on the base branch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MergeMethod {
    /// The maintainer's default (2026-11-17): history stays as it was written.
    #[default]
    Merge,
    Squash,
    Rebase,
}

impl MergeMethod {
    /// In GitHub's order, which is the order a repo's first allowed one is taken in.
    pub const ALL: [Self; 3] = [Self::Merge, Self::Squash, Self::Rebase];

    /// GraphQL's `PullRequestMergeMethod`.
    pub fn github_name(self) -> &'static str {
        match self {
            Self::Merge => "MERGE",
            Self::Squash => "SQUASH",
            Self::Rebase => "REBASE",
        }
    }

    pub fn of_github_name(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|method| method.github_name() == name)
    }

    /// Its value of `merge_method` in `config.toml`.
    pub fn config_text(self) -> &'static str {
        match self {
            Self::Merge => "merge",
            Self::Squash => "squash",
            Self::Rebase => "rebase",
        }
    }

    pub fn of_config_text(text: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|method| method.config_text() == text)
    }

    /// As the title bar, Settings and toasts name it.
    pub fn label(self) -> &'static str {
        match self {
            Self::Merge => "merge commit",
            Self::Squash => "squash",
            Self::Rebase => "rebase",
        }
    }
}

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

/// Whether a merge would go through now. GitHub's `mergeStateStatus`, with merged and closed PRs apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mergeability {
    /// Nothing blocks it, so GitHub refuses auto-merge and takes a merge at once.
    Now,
    /// Checks, reviews, rules, conflicts or a draft hold it back, or GitHub has not worked it out yet.
    Blocked,
    /// Merged or closed: nothing to merge.
    NotOpen,
}

impl Mergeability {
    /// From the PR's `state` and `mergeStateStatus`. `UNSTABLE` (checks that are not required
    /// fail) and `HAS_HOOKS` merge like `CLEAN`.
    pub fn from_github(state: &str, merge_state_status: &str) -> Self {
        match (state, merge_state_status) {
            ("OPEN", "CLEAN" | "UNSTABLE" | "HAS_HOOKS") => Self::Now,
            ("OPEN", _) => Self::Blocked,
            _ => Self::NotOpen,
        }
    }
}

/// What GitHub says of merging the PR, and what the viewer may do about it.
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
pub enum MergeRefusal {
    NotOpen,
    MayNotDisableAutoMerge,
    MayNotMerge,
    RepoDisallowsAutoMerge,
    MayNotEnableAutoMerge,
}

impl fmt::Display for MergeRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str(match self {
            Self::NotOpen => "The PR is merged or closed",
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

/// How much of the title bar button there is room for. The PR's title is what the bar is for, so
/// the button gives way first: its key caps, then the method, then the button itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonDetail {
    Full,
    WithoutKeys,
    WithoutMethod,
    Hidden,
}

/// Pixels a character of the bar's `text_sm` takes, rounded up.
const CHAR_WIDTH: f32 = 8.;
/// The PR's title keeps this much before anything else of the bar is shown.
const TITLE_MIN_WIDTH: f32 = 240.;
/// The bar's side margins, the PR icon, the chevron, the gaps, and the Jump and GitHub buttons.
const BAR_FIXED_WIDTH: f32 = 380.;
/// A button's icon, padding and gap, around its words.
const BUTTON_CHROME_WIDTH: f32 = 44.;
/// The key caps of the button.
const KEYS_WIDTH: f32 = 40.;
/// A chip's padding.
const CHIP_CHROME_WIDTH: f32 = 24.;

impl ButtonDetail {
    /// The most of the button that fits the title bar `bar_width` wide, next to the PR's name and
    /// the status chip's words, with the title keeping its minimum.
    pub fn fitting(
        merging: Merging,
        bar_width: f32,
        subject_chars: usize,
        chip_chars: usize,
    ) -> Self {
        let Some(label) = merging.button_label() else {
            return Self::Hidden;
        };
        let taken = BAR_FIXED_WIDTH
            + TITLE_MIN_WIDTH
            + subject_chars as f32 * CHAR_WIDTH
            + chip_chars as f32 * CHAR_WIDTH
            + CHIP_CHROME_WIDTH;
        let room = bar_width - taken;
        let method = match merging.auto_merge {
            AutoMerge::On(method) => method.label().len() as f32 * CHAR_WIDTH,
            AutoMerge::Off => 0.,
        };
        let label_width = BUTTON_CHROME_WIDTH + label.len() as f32 * CHAR_WIDTH;
        [
            (Self::Full, label_width + method + KEYS_WIDTH),
            (Self::WithoutKeys, label_width + method),
            (Self::WithoutMethod, label_width),
        ]
        .into_iter()
        .find_map(|(detail, width)| (width <= room).then_some(detail))
        .unwrap_or(Self::Hidden)
    }
}

impl Merging {
    /// Turn auto-merge off while it is on; merge now while nothing blocks the PR; else turn
    /// auto-merge on. Each only where GitHub says the viewer may.
    pub fn offer(self) -> Result<MergeOffer, MergeRefusal> {
        match (self.mergeability, self.auto_merge) {
            (Mergeability::NotOpen, _) => Err(MergeRefusal::NotOpen),
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
