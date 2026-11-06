use std::path::Path;

use super::ports::GitHubError;
use super::relevant_prs::{RelevantPullRequest, RelevantPullRequests};
use crate::pull_request::PullRequestUrl;
use crate::tour::Tour;

/// The list behind the PR title: the reviewer's relevant PRs, what each has, and what was typed
/// to narrow it. It loads in the background, so it starts out loading.
pub struct PrSwitcher {
    rows: Vec<SwitcherRow>,
    /// Indexes into `rows` that match the filter, in list order.
    visible: Vec<usize>,
    filter: String,
    state: LoadState,
}

enum LoadState {
    Loading,
    Loaded { teams_left_out: Vec<String> },
    Failed(String),
}

pub struct SwitcherRow {
    pub pull_request: RelevantPullRequest,
    pub tour: TourFact,
    haystack: String,
}

/// What the PR has to read, as far as the disk says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TourFact {
    Ready,
    Missing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum NotPickable {
    #[error("there is no tour for this pull request yet")]
    NoTourYet,
    #[error("there is no such row")]
    NoSuchRow,
}

impl Default for PrSwitcher {
    fn default() -> Self {
        Self {
            rows: Vec::new(),
            visible: Vec::new(),
            filter: String::new(),
            state: LoadState::Loading,
        }
    }
}

impl PrSwitcher {
    /// Takes the answer of the background read. A failed read empties the list and says why.
    pub fn loaded(&mut self, answer: Result<RelevantPullRequests, GitHubError>, home: &Path) {
        match answer {
            Ok(relevant) => {
                self.rows = relevant
                    .pull_requests
                    .into_iter()
                    .map(|pull_request| SwitcherRow::new(pull_request, home))
                    .collect();
                self.state = LoadState::Loaded {
                    teams_left_out: relevant.teams_left_out,
                };
            }
            Err(error) => {
                self.rows.clear();
                self.state = LoadState::Failed(format!(
                    "Could not list your pull requests from GitHub: {error}"
                ));
            }
        }
        self.apply_filter();
    }

    /// Looks for tours again, as one may have been written since the list loaded.
    pub fn check_tours(&mut self, home: &Path) {
        for row in &mut self.rows {
            row.tour = TourFact::of(&row.pull_request.url, home);
        }
    }

    pub fn filter(&mut self, text: &str) {
        text.clone_into(&mut self.filter);
        self.apply_filter();
    }

    pub fn is_loading(&self) -> bool {
        matches!(self.state, LoadState::Loading)
    }

    /// Something the reviewer should know about the list: GitHub could not be reached, or some
    /// teams were not searched.
    pub fn notice(&self) -> Option<String> {
        match &self.state {
            LoadState::Loading => None,
            LoadState::Failed(reason) => Some(reason.clone()),
            LoadState::Loaded { teams_left_out } if teams_left_out.is_empty() => None,
            LoadState::Loaded { teams_left_out } => Some(format!(
                "Review requests to these teams are not listed, only 10 teams are searched: {}",
                teams_left_out.join(", ")
            )),
        }
    }

    /// The rows the filter lets through, newest update first.
    pub fn visible(&self) -> impl Iterator<Item = &SwitcherRow> {
        self.visible
            .iter()
            .filter_map(|&index| self.rows.get(index))
    }

    /// The PR to switch to for the `nth` visible row.
    pub fn pick(&self, nth: usize) -> Result<PullRequestUrl, NotPickable> {
        let row = self.visible().nth(nth).ok_or(NotPickable::NoSuchRow)?;
        match row.tour {
            TourFact::Ready => Ok(row.pull_request.url.clone()),
            TourFact::Missing => Err(NotPickable::NoTourYet),
        }
    }

    fn apply_filter(&mut self) {
        let lowered = self.filter.to_lowercase();
        let words: Vec<&str> = lowered.split_whitespace().collect();
        self.visible = self
            .rows
            .iter()
            .enumerate()
            .filter(|(_, row)| words.iter().all(|word| row.haystack.contains(word)))
            .map(|(index, _)| index)
            .collect();
    }
}

impl SwitcherRow {
    fn new(pull_request: RelevantPullRequest, home: &Path) -> Self {
        let haystack = format!(
            "{} {} {}",
            pull_request.url.short(),
            pull_request.title,
            pull_request.author
        )
        .to_lowercase();
        Self {
            tour: TourFact::of(&pull_request.url, home),
            pull_request,
            haystack,
        }
    }
}

impl TourFact {
    fn of(pr: &PullRequestUrl, home: &Path) -> Self {
        if Tour::path_for(home, pr).exists() {
            Self::Ready
        } else {
            Self::Missing
        }
    }
}
