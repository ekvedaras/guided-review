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
