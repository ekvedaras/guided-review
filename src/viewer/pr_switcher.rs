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
    is_finding_local: bool,
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
    /// The repo, or the folder of a local review with no repo, that the row is listed under.
    pub group: String,
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

/// A line of the switcher's screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScreenLine {
    /// The repo, or the folder of a local review with no repo, whose rows follow, and how many rows
    /// it has in the whole list.
    Heading { name: String, rows: usize },
    /// The nth visible row.
    Row(usize),
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
            is_finding_local: true,
            hovered_screen_row: None,
            memory: None,
            memory_problem: None,
        }
    }
}

impl PrSwitcher {
    /// The screen's height in lines: a row takes two, a repo's name one. Eight rows of one repo fill it.
    pub const SCREEN_LINES: usize = 17;
    pub const ROW_LINES: usize = 2;
    pub const HEADING_LINES: usize = 1;

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

    /// The visible row under the pointer.
    pub fn hovered(&self) -> Option<usize> {
        self.hovered_screen_row.map(|row| self.first_shown + row)
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
        self.is_finding_local = true;
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
            .filter(|&row| row < self.rows_on_screen());
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
        let last_first = self.last_first_shown();
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
        self.is_finding_local = false;
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

    /// What the row says of the PR's tour, when it is more than the basic tour most rows have.
    pub fn tour_label(&self, row: &SwitcherRow) -> Option<&'static str> {
        match (row.tour.writing, row.tour.on_disk) {
            (WritingFact::Writing, _) => Some("writing"),
            (WritingFact::Failed, _) => Some("writing failed"),
            (WritingFact::None, ToursOnDisk::None | ToursOnDisk::Basic) => None,
            (WritingFact::None, ToursOnDisk::Guided) => Some("guided"),
            (WritingFact::None, ToursOnDisk::BasicWithGuided) => Some("guided ready"),
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

    pub fn select_last(&mut self) {
        self.select(self.visible.len().saturating_sub(1));
    }

    pub fn select_previous(&mut self) {
        self.select(self.selected.saturating_sub(1));
    }

    /// Scrolls only as far as the row needs to be on screen, and not at all when it is.
    pub fn select(&mut self, nth: usize) {
        self.selected = nth;
        if nth < self.first_shown {
            self.first_shown = nth;
        } else {
            while nth >= self.first_shown + self.rows_on_screen() {
                self.first_shown += 1;
            }
        }
    }

    /// The first row of the screen once the scrollbar is dragged there.
    pub fn scroll_to(&mut self, first: usize) {
        self.first_shown = first.min(self.last_first_shown());
    }

    /// Whether the local folders are being read for the local rows.
    pub fn is_finding_local(&self) -> bool {
        self.is_finding_local
    }

    /// The wheel moves the rows, not the selection.
    pub fn scroll_by(&mut self, rows: isize) {
        let last_first = self.last_first_shown();
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

    /// What is on screen, top to bottom: the rows from `first_shown`, each repo's name above its first one.
    /// Below the whole rows, what starts before the screen's foot follows too, cut off by it, so the
    /// screen never ends on a gap while more rows are below.
    pub fn screen(&self) -> Vec<ScreenLine> {
        let groups = self.visible_groups();
        let mut lines = Vec::new();
        let mut used = 0;
        let whole = self.first_shown + self.rows_on_screen();
        for nth in self.first_shown..groups.len() {
            if let Some(group) = Self::opened_group(&groups, nth, self.first_shown) {
                if nth >= whole && used >= Self::SCREEN_LINES {
                    break;
                }
                lines.push(ScreenLine::Heading {
                    name: group.to_owned(),
                    rows: groups.iter().filter(|other| **other == group).count(),
                });
                used += Self::HEADING_LINES;
            }
            if nth >= whole && used >= Self::SCREEN_LINES {
                break;
            }
            lines.push(ScreenLine::Row(nth));
            used += Self::ROW_LINES;
            if nth >= whole {
                break;
            }
        }
        lines
    }

    /// How many rows are on screen from `first_shown`, as each repo's name takes room too.
    pub fn rows_on_screen(&self) -> usize {
        self.rows_fitting_from(self.first_shown, &self.visible_groups())
    }

    fn rows_fitting_from(&self, first: usize, groups: &[&str]) -> usize {
        let mut lines = 0;
        let mut fitting = 0;
        for nth in first..groups.len() {
            let heading = if Self::opened_group(groups, nth, first).is_some() {
                Self::HEADING_LINES
            } else {
                0
            };
            lines += heading + Self::ROW_LINES;
            if lines > Self::SCREEN_LINES {
                break;
            }
            fitting += 1;
        }
        fitting
    }

    fn visible_groups(&self) -> Vec<&str> {
        self.visible().map(|row| row.group.as_str()).collect()
    }

    /// The group the `nth` row starts on screen. The top row names its repo too, so a scrolled screen
    /// still says whose rows they are.
    fn opened_group<'a>(groups: &[&'a str], nth: usize, first: usize) -> Option<&'a str> {
        let group = *groups.get(nth)?;
        let above = nth.checked_sub(1).and_then(|above| groups.get(above));
        (nth == first || above != Some(&group)).then_some(group)
    }

    /// The last `first_shown` that still fills the screen: from it, the last row is on screen.
    pub fn last_first_shown(&self) -> usize {
        let groups = self.visible_groups();
        let total = groups.len();
        (0..total)
            .find(|&first| first + self.rows_fitting_from(first, &groups) >= total)
            .unwrap_or(0)
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

    /// Puts each repo's rows together, the repos in the order of their newest row, rows in list order.
    fn group_visible(&mut self) {
        let mut groups: Vec<&str> = Vec::new();
        for &index in &self.visible {
            if let Some(row) = self.row_at(index)
                && !groups.contains(&row.group.as_str())
            {
                groups.push(&row.group);
            }
        }
        let rank_of = |index: usize| {
            self.row_at(index)
                .and_then(|row| groups.iter().position(|group| *group == row.group))
        };
        let mut ranked: Vec<(Option<usize>, usize)> = self
            .visible
            .iter()
            .map(|&index| (rank_of(index), index))
            .collect();
        ranked.sort_by_key(|&(rank, _)| rank);
        self.visible = ranked.into_iter().map(|(_, index)| index).collect();
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
        self.group_visible();
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
            pull_request.author.login
        );
        let group = pull_request.url.repo.to_string();
        Self::new(
            ReviewSubject::from(pull_request.url.clone()),
            RowKind::PullRequest(pull_request),
            &haystack,
            group,
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
        let group = local.change_set.repo().map_or_else(
            || local.change_set.folder().name().into_owned(),
            ToString::to_string,
        );
        Self::new(subject, RowKind::Local(local), &haystack, group, folders)
    }

    fn new(
        subject: ReviewSubject,
        kind: RowKind,
        haystack: &str,
        group: String,
        folders: &AppFolders,
    ) -> Self {
        Self {
            group,
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
