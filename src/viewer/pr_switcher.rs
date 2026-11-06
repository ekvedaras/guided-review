use std::path::Path;

use super::active_pr::ActivePr;
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
    is_refreshing: bool,
    /// Whether tour agents can be reached, so a PR with no tour can be written; the reason when not.
    agent_link: Result<(), String>,
}

enum LoadState {
    Loading,
    Loaded { teams_left_out: Vec<String> },
    Failed(String),
}

pub struct SwitcherRow {
    pub pull_request: RelevantPullRequest,
    pub tour: TourFact,
    /// The PR's tour agent answered while another PR was shown.
    pub has_unseen_reply: bool,
    haystack: String,
}

/// What the PR has to read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TourFact {
    Ready,
    Missing,
    /// A tour agent the app started is writing it.
    Writing,
    /// The writing ended without a tour.
    WritingFailed,
}

/// What picking a row does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pick {
    /// Switch to the PR's tour.
    Show(PullRequestUrl),
    /// Show the writing of the PR's tour on the loading screen, starting it when none is on.
    Write(PullRequestUrl),
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NotPickable {
    #[error(
        "this pull request has no tour, and writing one needs herdr (`hc`) with its server running: {0}"
    )]
    NeedsHerdr(String),
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
            is_refreshing: true,
            agent_link: Ok(()),
        }
    }
}

impl PrSwitcher {
    /// A read of the list starts; the rows listed so far stay until its answer.
    pub fn refreshing(&mut self) {
        self.is_refreshing = true;
    }

    pub fn is_refreshing(&self) -> bool {
        self.is_refreshing
    }

    /// Takes the answer of the background read. A failed read keeps the rows listed before and says why.
    pub fn loaded(&mut self, answer: Result<RelevantPullRequests, GitHubError>, home: &Path) {
        self.is_refreshing = false;
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
                self.state = LoadState::Failed(format!(
                    "Could not list your pull requests from GitHub: {error}"
                ));
            }
        }
        self.apply_filter();
    }

    /// Looks for tours again, as one may have been written since the list loaded, and takes what the
    /// app knows beyond the disk: tours being written and agent replies not seen.
    pub fn check_tours(&mut self, home: &Path, active: &ActivePr) {
        for row in &mut self.rows {
            let pr = &row.pull_request.url;
            row.tour = active
                .tour_fact_of(pr)
                .unwrap_or_else(|| TourFact::of(pr, home));
            row.has_unseen_reply = active.has_unseen_reply(pr);
        }
    }

    pub fn agent_link(&mut self, link: Result<(), String>) {
        self.agent_link = link;
    }

    /// What the row says of the PR's tour.
    pub fn tour_label(&self, row: &SwitcherRow) -> &'static str {
        match row.tour {
            TourFact::Ready => "tour",
            TourFact::Missing if self.agent_link.is_err() => "needs herdr",
            TourFact::Missing => "no tour yet",
            TourFact::Writing => "writing",
            TourFact::WritingFailed => "writing failed",
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

    /// What picking the `nth` visible row does.
    pub fn pick(&self, nth: usize) -> Result<Pick, NotPickable> {
        let row = self.visible().nth(nth).ok_or(NotPickable::NoSuchRow)?;
        let pr = row.pull_request.url.clone();
        match (row.tour, &self.agent_link) {
            (TourFact::Ready, _) => Ok(Pick::Show(pr)),
            (TourFact::Missing, Err(reason)) => Err(NotPickable::NeedsHerdr(reason.clone())),
            (TourFact::Missing | TourFact::Writing | TourFact::WritingFailed, _) => {
                Ok(Pick::Write(pr))
            }
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
            has_unseen_reply: false,
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
