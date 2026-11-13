use std::path::Path;

use super::active_pr::ActivePr;
use super::ports::GitHubError;
use super::relevant_prs::{RelevantPullRequest, RelevantPullRequests};
use super::remembered_prs::RememberedPullRequestsFile;
use crate::pull_request::PullRequestUrl;
use crate::tour::Tour;

/// The list behind the PR title: the reviewer's relevant PRs, what each has, and what was typed
/// to narrow it. It loads in the background, so it starts out loading.
pub struct PrSwitcher {
    rows: Vec<SwitcherRow>,
    /// Indexes into `rows` that match the filter, in list order.
    visible: Vec<usize>,
    filter: String,
    /// Nth visible row.
    selected: usize,
    /// Nth visible row at the top of the rows shown.
    first_shown: usize,
    state: LoadState,
    is_refreshing: bool,
    /// Whether tour agents can be reached, so a PR with no tour can be written; the reason when not.
    agent_link: Result<(), String>,
    /// The row under the pointer, counted from the top of the rows on screen, as the pointer stays
    /// where it is when the wheel moves the rows.
    hovered_screen_row: Option<usize>,
    memory: Option<RememberedPullRequestsFile>,
    /// The list from last time could not be read, or this one could not be kept for next time.
    memory_problem: Option<String>,
}

/// Where the reviewer is in the list, kept across a new list from GitHub so rows do not jump under
/// the pointer: the selected PR stays selected, and the hovered PR, or the selected one when none
/// is, stays on its row of the screen.
struct Place {
    selected: Option<PullRequestUrl>,
    /// The PR to keep on screen, and its row counted from the top of the screen.
    pinned: Option<(PullRequestUrl, usize)>,
    first_shown: usize,
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
    /// The PR under review.
    pub is_active: bool,
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
            selected: 0,
            first_shown: 0,
            state: LoadState::Loading,
            is_refreshing: true,
            agent_link: Ok(()),
            hovered_screen_row: None,
            memory: None,
            memory_problem: None,
        }
    }
}

impl PrSwitcher {
    pub const ROWS_SHOWN: usize = 10;

    /// Starts on the list GitHub gave last time, when there is one, and keeps each new one for next time.
    pub fn remembering_in(memory: RememberedPullRequestsFile, home: &Path) -> Self {
        let mut switcher = Self::default();
        match memory.load() {
            Ok(remembered) if remembered.is_empty() => {}
            Ok(remembered) => {
                switcher.rows = remembered
                    .into_iter()
                    .map(|pull_request| SwitcherRow::new(pull_request, home))
                    .collect();
                switcher.state = LoadState::Loaded {
                    teams_left_out: Vec::new(),
                };
                switcher.apply_filter();
            }
            Err(error) => {
                switcher.memory_problem =
                    Some(format!("Could not show the list from last time: {error:#}"));
            }
        }
        switcher.memory = Some(memory);
        switcher
    }

    /// The pointer is over the `nth` visible row.
    pub fn hover(&mut self, nth: usize) {
        self.hovered_screen_row = nth.checked_sub(self.first_shown);
    }

    /// The pointer left the `nth` visible row. Rows report leaving and entering in no set order, so
    /// leaving a row the pointer is no longer over changes nothing.
    pub fn unhover(&mut self, nth: usize) {
        if self.hovered_screen_row == nth.checked_sub(self.first_shown) {
            self.hovered_screen_row = None;
        }
    }

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
                if let Some(memory) = &self.memory {
                    self.memory_problem = memory.save(&relevant.pull_requests).err().map(|error| {
                        format!("Could not keep this list for the next start: {error:#}")
                    });
                }
                let place = self.place();
                self.rows = relevant
                    .pull_requests
                    .into_iter()
                    .map(|pull_request| SwitcherRow::new(pull_request, home))
                    .collect();
                self.state = LoadState::Loaded {
                    teams_left_out: relevant.teams_left_out,
                };
                self.apply_filter();
                self.keep(place);
            }
            Err(error) => {
                self.state = LoadState::Failed(format!(
                    "Could not list your pull requests from GitHub: {error}"
                ));
                self.apply_filter();
            }
        }
    }

    fn place(&self) -> Place {
        let url_of = |nth: usize| {
            self.visible()
                .nth(nth)
                .map(|row| row.pull_request.url.clone())
        };
        let selected_screen_row = self
            .selected
            .checked_sub(self.first_shown)
            .filter(|&row| row < Self::ROWS_SHOWN);
        let pinned_screen_row = self.hovered_screen_row.or(selected_screen_row);
        Place {
            selected: url_of(self.selected),
            pinned: pinned_screen_row.and_then(|screen_row| {
                url_of(self.first_shown + screen_row).map(|url| (url, screen_row))
            }),
            first_shown: self.first_shown,
        }
    }

    /// A pinned PR that left the list leaves the rows where they were.
    fn keep(&mut self, place: Place) {
        let nth_of =
            |url: &PullRequestUrl| self.visible().position(|row| &row.pull_request.url == url);
        let selected = place.selected.and_then(|url| nth_of(&url));
        let pinned_first_shown = place
            .pinned
            .and_then(|(url, screen_row)| nth_of(&url).map(|nth| nth.saturating_sub(screen_row)));
        if let Some(nth) = selected {
            self.selected = nth;
        }
        let last_first = self.visible.len().saturating_sub(Self::ROWS_SHOWN);
        self.first_shown = pinned_first_shown
            .unwrap_or(place.first_shown)
            .min(last_first);
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
            row.is_active = active.shown() == Some(pr);
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
        self.selected = 0;
        self.first_shown = 0;
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    /// The nth visible row at the top of the `ROWS_SHOWN` rows on screen.
    pub fn first_shown(&self) -> usize {
        self.first_shown
    }

    pub fn select_next(&mut self) {
        self.select((self.selected + 1).min(self.visible.len().saturating_sub(1)));
    }

    pub fn select_previous(&mut self) {
        self.select(self.selected.saturating_sub(1));
    }

    /// Scrolls only as far as the row needs to be on screen, and not at all when it is.
    pub fn select(&mut self, nth: usize) {
        self.selected = nth;
        if nth < self.first_shown {
            self.first_shown = nth;
        } else if nth >= self.first_shown + Self::ROWS_SHOWN {
            self.first_shown = nth + 1 - Self::ROWS_SHOWN;
        }
    }

    /// The wheel moves the rows, not the selection.
    pub fn scroll_by(&mut self, rows: isize) {
        let last_first = self.visible.len().saturating_sub(Self::ROWS_SHOWN);
        self.first_shown = self.first_shown.saturating_add_signed(rows).min(last_first);
    }

    /// Puts the selection on the PR under review, or the first row, scrolling from the top.
    pub fn select_active(&mut self) {
        self.first_shown = 0;
        let active = self.visible().position(|row| row.is_active);
        self.select(active.unwrap_or(0));
    }

    pub fn is_loading(&self) -> bool {
        matches!(self.state, LoadState::Loading)
    }

    /// Something the reviewer should know about the list: GitHub could not be reached, or some
    /// teams were not searched.
    pub fn notice(&self) -> Option<String> {
        match &self.state {
            LoadState::Loading => self.memory_problem.clone(),
            LoadState::Failed(reason) => Some(reason.clone()),
            LoadState::Loaded { teams_left_out } if teams_left_out.is_empty() => {
                self.memory_problem.clone()
            }
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
        let last = self.visible.len().saturating_sub(1);
        self.select(self.selected.min(last));
        self.scroll_by(0);
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
            is_active: false,
            pull_request,
            haystack,
        }
    }
}

impl TourFact {
    /// What the disk says of the PR's tour.
    pub fn of(pr: &PullRequestUrl, home: &Path) -> Self {
        if Tour::path_for(home, pr).exists() {
            Self::Ready
        } else {
            Self::Missing
        }
    }
}
