use crate::app_folders::AppFolders;
use crate::review_subject::ReviewSubject;

use super::active_pr::ActivePr;
use super::local_row::LocalRow;
use super::ports::GitHubError;
use super::relevant_prs::{RelevantPullRequest, RelevantPullRequests};
use super::remembered_prs::RememberedPullRequestsFile;
use crate::tour::ToursOnDisk;

/// The list behind the PR title: the reviewer's relevant PRs, what each has, and what was typed
/// to narrow it. It loads in the background, so it starts out loading.
pub struct PrSwitcher {
    /// What GitHub listed, newest update first.
    pull_requests: Vec<SwitcherRow>,
    /// The local change sets found on disk, after the PRs.
    local: Vec<SwitcherRow>,
    /// Indexes into the PRs followed by the local rows that match the filter, in list order.
    visible: Vec<usize>,
    filter: String,
    /// Nth visible row.
    selected: usize,
    /// Nth visible row at the top of the rows shown.
    first_shown: usize,
    state: LoadState,
    is_refreshing: bool,
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
    selected: Option<ReviewSubject>,
    /// The row to keep on screen, and its place counted from the top of the screen.
    pinned: Option<(ReviewSubject, usize)>,
    first_shown: usize,
}

enum LoadState {
    Loading,
    Loaded { teams_left_out: Vec<String> },
    Failed(String),
}

pub struct SwitcherRow {
    pub subject: ReviewSubject,
    pub kind: RowKind,
    pub tour: TourFact,
    /// The PR's tour agent answered while another PR was shown.
    pub has_unseen_reply: bool,
    /// The PR under review.
    pub is_active: bool,
    haystack: String,
}

/// What a row is: a PR GitHub listed, or a local change set found on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RowKind {
    PullRequest(RelevantPullRequest),
    Local(LocalRow),
}

/// What the PR has to read: the tours on disk, and apart from them what the app is writing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TourFact {
    pub on_disk: ToursOnDisk,
    pub writing: WritingFact,
}

/// A tour agent the app started writing a guided tour of the PR.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WritingFact {
    None,
    Writing,
    /// The writing ended without a tour.
    Failed,
}

/// What picking a row does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pick {
    /// Switch to the PR's tour.
    Show(ReviewSubject),
    /// Show the writing of the PR's tour on the loading screen.
    Write(ReviewSubject),
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NotPickable {
    #[error("there is no such row")]
    NoSuchRow,
}

impl Default for PrSwitcher {
    fn default() -> Self {
        Self {
            pull_requests: Vec::new(),
            local: Vec::new(),
            visible: Vec::new(),
            filter: String::new(),
            selected: 0,
            first_shown: 0,
            state: LoadState::Loading,
            is_refreshing: true,
            hovered_screen_row: None,
            memory: None,
            memory_problem: None,
        }
    }
}

impl PrSwitcher {
    pub const ROWS_SHOWN: usize = 10;

    /// Starts on the list GitHub gave last time, when there is one, and keeps each new one for next time.
    pub fn remembering_in(memory: RememberedPullRequestsFile, folders: &AppFolders) -> Self {
        let mut switcher = Self::default();
        match memory.load() {
            Ok(remembered) if remembered.is_empty() => {}
            Ok(remembered) => {
                switcher.pull_requests = remembered
                    .into_iter()
                    .map(|pull_request| SwitcherRow::of_pull_request(pull_request, folders))
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
    pub fn loaded(
        &mut self,
        answer: Result<RelevantPullRequests, GitHubError>,
        folders: &AppFolders,
    ) {
        self.is_refreshing = false;
        match answer {
            Ok(relevant) => {
                if let Some(memory) = &self.memory {
                    self.memory_problem = memory.save(&relevant.pull_requests).err().map(|error| {
                        format!("Could not keep this list for the next start: {error:#}")
                    });
                }
                let place = self.place();
                self.pull_requests = relevant
                    .pull_requests
                    .into_iter()
                    .map(|pull_request| SwitcherRow::of_pull_request(pull_request, folders))
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
        let subject_of = |nth: usize| self.visible().nth(nth).map(|row| row.subject.clone());
        let selected_screen_row = self
            .selected
            .checked_sub(self.first_shown)
            .filter(|&row| row < Self::ROWS_SHOWN);
        let pinned_screen_row = self.hovered_screen_row.or(selected_screen_row);
        Place {
            selected: subject_of(self.selected),
            pinned: pinned_screen_row.and_then(|screen_row| {
                subject_of(self.first_shown + screen_row).map(|subject| (subject, screen_row))
            }),
            first_shown: self.first_shown,
        }
    }

    /// A pinned row that left the list leaves the rows where they were.
    fn keep(&mut self, place: Place) {
        let nth_of =
            |subject: &ReviewSubject| self.visible().position(|row| &row.subject == subject);
        let selected = place.selected.and_then(|subject| nth_of(&subject));
        let pinned_first_shown = place.pinned.and_then(|(subject, screen_row)| {
            nth_of(&subject).map(|nth| nth.saturating_sub(screen_row))
        });
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
    pub fn check_tours(&mut self, folders: &AppFolders, active: &ActivePr) {
        for row in self.pull_requests.iter_mut().chain(&mut self.local) {
            row.tour = TourFact {
                on_disk: ToursOnDisk::of(folders, &row.subject),
                writing: active.writing_fact_of(&row.subject),
            };
            row.has_unseen_reply = active.has_unseen_reply(&row.subject);
            row.is_active = active.shown() == Some(&row.subject);
        }
    }

    /// Takes the local change sets found off the UI thread, after the PRs: they replace the local
    /// rows listed before, and the PR rows stay as they are.
    pub fn local_found(&mut self, found: Vec<LocalRow>, folders: &AppFolders) {
        let place = self.place();
        self.local = found
            .into_iter()
            .map(|row| SwitcherRow::of_local(row, folders))
            .collect();
        self.apply_filter();
        self.keep(place);
    }

    /// A local review was dropped: its row is no more.
    pub fn dropped(&mut self, subject: &ReviewSubject) {
        let place = self.place();
        self.local.retain(|row| &row.subject != subject);
        self.apply_filter();
        self.keep(place);
    }

    /// The local review to drop when the `nth` visible row offers it, as its branch is gone.
    pub fn drop_offer(&self, nth: usize) -> Option<ReviewSubject> {
        let row = self.visible().nth(nth)?;
        row.local()?
            .offers_drop_review()
            .then(|| row.subject.clone())
    }

    /// What the row says of the PR's tour.
    pub fn tour_label(&self, row: &SwitcherRow) -> &'static str {
        match (row.tour.writing, row.tour.on_disk) {
            (WritingFact::Writing, _) => "writing",
            (WritingFact::Failed, _) => "writing failed",
            (WritingFact::None, ToursOnDisk::None | ToursOnDisk::Basic) => "basic",
            (WritingFact::None, ToursOnDisk::Guided) => "guided",
            (WritingFact::None, ToursOnDisk::BasicWithGuided) => "guided ready",
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
        self.visible.iter().filter_map(|&index| self.row_at(index))
    }

    fn row_at(&self, index: usize) -> Option<&SwitcherRow> {
        self.pull_requests
            .get(index)
            .or_else(|| self.local.get(index.checked_sub(self.pull_requests.len())?))
    }

    /// What picking the `nth` visible row does.
    pub fn pick(&self, nth: usize) -> Result<Pick, NotPickable> {
        let row = self.visible().nth(nth).ok_or(NotPickable::NoSuchRow)?;
        let subject = row.subject.clone();
        if row.tour.is_waited_for() {
            Ok(Pick::Write(subject))
        } else {
            Ok(Pick::Show(subject))
        }
    }

    fn apply_filter(&mut self) {
        let lowered = self.filter.to_lowercase();
        let words: Vec<&str> = lowered.split_whitespace().collect();
        self.visible = self
            .pull_requests
            .iter()
            .chain(&self.local)
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
    fn of_pull_request(pull_request: RelevantPullRequest, folders: &AppFolders) -> Self {
        let haystack = format!(
            "{} {} {}",
            pull_request.url.short(),
            pull_request.title,
            pull_request.author
        );
        Self::new(
            ReviewSubject::from(pull_request.url.clone()),
            RowKind::PullRequest(pull_request),
            &haystack,
            folders,
        )
    }

    fn of_local(local: LocalRow, folders: &AppFolders) -> Self {
        let subject = ReviewSubject::from(local.change_set.clone());
        let repo = local.change_set.repo().map(ToString::to_string);
        let haystack = format!(
            "local {} {} {}",
            subject.short(),
            repo.unwrap_or_default(),
            local.change_set.branch().as_str()
        );
        Self::new(subject, RowKind::Local(local), &haystack, folders)
    }

    fn new(subject: ReviewSubject, kind: RowKind, haystack: &str, folders: &AppFolders) -> Self {
        Self {
            tour: TourFact::on_disk_of(&subject, folders),
            has_unseen_reply: false,
            is_active: false,
            subject,
            kind,
            haystack: haystack.to_lowercase(),
        }
    }

    pub const fn pull_request(&self) -> Option<&RelevantPullRequest> {
        match &self.kind {
            RowKind::PullRequest(pull_request) => Some(pull_request),
            RowKind::Local(_) => None,
        }
    }

    pub const fn local(&self) -> Option<&LocalRow> {
        match &self.kind {
            RowKind::Local(local) => Some(local),
            RowKind::PullRequest(_) => None,
        }
    }
}

impl TourFact {
    /// What the disk says of the subject's tours, with no writing known.
    pub fn on_disk_of(subject: &ReviewSubject, folders: &AppFolders) -> Self {
        Self {
            on_disk: ToursOnDisk::of(folders, subject),
            writing: WritingFact::None,
        }
    }

    /// A tour is being written and there is nothing to read yet, so the loading screen shows it.
    pub fn is_waited_for(self) -> bool {
        self.writing != WritingFact::None && self.on_disk == ToursOnDisk::None
    }
}
