use super::SettingProblem;

/// One page of Settings, picked in the list on its left.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsPage {
    Agents,
    Code,
    GitHub,
    Keys,
    About,
}

impl SettingsPage {
    pub const ALL: [Self; 5] = [
        Self::Agents,
        Self::Code,
        Self::GitHub,
        Self::Keys,
        Self::About,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Self::Agents => "Agents",
            Self::Code => "Code",
            Self::GitHub => "GitHub",
            Self::Keys => "Keys",
            Self::About => "About",
        }
    }

    pub fn of(problem: &SettingProblem) -> Self {
        match problem {
            SettingProblem::InstructionsFileGone(..)
            | SettingProblem::InstructionsUnreadable(..)
            | SettingProblem::AskEffortUnknown(_)
            | SettingProblem::AskModelBlank => Self::Agents,
            SettingProblem::Clone { .. }
            | SettingProblem::IdeTemplateBlank
            | SettingProblem::MoreLinesNotACount(_) => Self::Code,
            SettingProblem::Key(_) => Self::Keys,
        }
    }

    /// A Save refused for a problem on a page out of sight would look like a Save that did nothing.
    pub fn after_refusal(self, problems: &[SettingProblem]) -> Self {
        let pages_with_problems: Vec<Self> = problems.iter().map(Self::of).collect();
        if pages_with_problems.contains(&self) {
            return self;
        }
        Self::ALL
            .into_iter()
            .find(|page| pages_with_problems.contains(page))
            .unwrap_or(self)
    }
}
