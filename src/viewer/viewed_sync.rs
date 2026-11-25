use std::collections::{BTreeMap, BTreeSet};

use crate::coverage;
use crate::file_path::FilePath;
use crate::pieces::ChangedFile;
use crate::pull_request::{PullRequestUrl, Sha};
use crate::tour::GuidedTour;

use super::ports::{CodeHost, HostError, Viewed, ViewedFilesPort};
use super::review_state::ReviewState;
use super::screen::ReadSubject;

/// Keeps the host's "viewed" marks and the read ticks in step. Ticks keep working when it is off.
#[derive(Default)]
pub(super) struct ViewedSync {
    /// Why sync is switched off for this session; the app shows it as a banner.
    off_because: Option<String>,
    /// The host has no viewed marks (a local review's has none): ticks stay ticks.
    without_host_marks: bool,
}

impl ViewedSync {
    pub(super) fn without_host_marks() -> Self {
        Self {
            off_because: None,
            without_host_marks: true,
        }
    }

    pub(super) fn has_host_marks(&self) -> bool {
        !self.without_host_marks
    }

    fn wishes_nothing(&self) -> bool {
        self.without_host_marks || self.off_because.is_some()
    }

    pub(super) fn banner(&self) -> Option<&str> {
        self.off_because.as_deref()
    }

    pub(super) fn not_synced(&self, state: &ReviewState) -> bool {
        !self.wishes_nothing() && !state.unsent_viewed().is_empty()
    }

    pub(super) fn wish_after_tick(
        &self,
        state: &mut ReviewState,
        coverage: &Coverage,
        subject: &ReadSubject,
    ) {
        if self.wishes_nothing() {
            return;
        }
        let fully_ticked: Vec<&FilePath> = coverage
            .files_of(subject)
            .iter()
            .filter(|file| coverage.is_fully_ticked(file, |subject| state.is_read(subject)))
            .collect();
        for file in fully_ticked {
            state.want_viewed(file, Viewed::Yes);
        }
    }

    pub(super) fn wish_after_untick(
        &self,
        state: &mut ReviewState,
        coverage: &Coverage,
        subject: &ReadSubject,
    ) {
        if self.wishes_nothing() {
            return;
        }
        for file in coverage.files_of(subject) {
            state.want_viewed(file, Viewed::No);
        }
    }

    /// Everything the network part of a sync needs, owned, so it can run on a background thread.
    /// `None` when the host has no marks to sync with.
    pub(super) fn request(
        &self,
        state: &ReviewState,
        coverage: &Coverage,
        pr: &PullRequestUrl,
        tour_head: &Sha,
    ) -> Option<SyncRequest> {
        if self.without_host_marks {
            return None;
        }
        let baseline_wishes = state
            .host_baseline_due()
            .then(|| baseline_wishes(state, coverage));
        Some(SyncRequest {
            pr: pr.clone(),
            tour_head: tour_head.clone(),
            unsent: state.unsent_viewed().clone(),
            baseline_wishes,
            is_off: self.off_because.is_some(),
        })
    }

    /// Takes in what GitHub said. Returns what it unticked, and the error of the network part.
    pub(super) fn take_in(
        &self,
        state: &mut ReviewState,
        coverage: &Coverage,
        tour_head: &Sha,
        outcome: SyncOutcome,
    ) -> (Vec<ReadSubject>, Option<HostError>) {
        outcome.land(state, coverage, tour_head)
    }

    /// GitHub saying viewed sync is unavailable switches it off for the session instead of failing.
    pub(super) fn settle(&mut self, error: HostError) -> anyhow::Result<()> {
        match error {
            HostError::NoAccess(no_access) => {
                self.off_because = Some(no_access.into_reason());
                Ok(())
            }
            HostError::Other(error) => Err(error),
            typed => Err(typed.into()),
        }
    }

    pub(super) fn is_off(&self) -> bool {
        self.off_because.is_some()
    }
}

/// After a re-pin: viewed for every file whose steps are all still ticked, and the queued
/// "not viewed" edits for the rest. Queued "viewed" edits are dropped: the ticks say it again
/// where it is still true.
fn baseline_wishes(state: &ReviewState, coverage: &Coverage) -> BTreeMap<FilePath, Viewed> {
    let is_fully_ticked =
        |path: &FilePath| coverage.is_fully_ticked(path, |subject| state.is_read(subject));
    let still_viewed = coverage
        .held_files()
        .filter(|path| is_fully_ticked(path))
        .map(|path| (path.clone(), Viewed::Yes));
    let still_not_viewed = state
        .unsent_viewed()
        .iter()
        .filter(|(path, viewed)| **viewed == Viewed::No && !is_fully_ticked(path))
        .map(|(path, _)| (path.clone(), Viewed::No));
    still_viewed.chain(still_not_viewed).collect()
}

/// Reacts only to files whose state changed since the last sync, and never to files with an
/// unsent local change: those win. Returns what it unticked.
fn pull_viewed(
    state: &mut ReviewState,
    coverage: &Coverage,
    tour_head: &Sha,
    on_host: &BTreeMap<FilePath, Viewed>,
) -> Vec<ReadSubject> {
    let unsent = state.unsent_viewed().clone();
    let is_viewed = |path: &FilePath| {
        unsent
            .get(path)
            .or_else(|| on_host.get(path))
            .copied()
            .unwrap_or(Viewed::No)
    };
    let changed: Vec<(&FilePath, Viewed)> = on_host
        .iter()
        .filter(|(path, now)| {
            !unsent.contains_key(*path) && state.was_viewed_on_host(path) != **now
        })
        .map(|(path, now)| (path, *now))
        .collect();

    let mut unticked = Vec::new();
    for (path, _) in changed.iter().filter(|(_, now)| *now == Viewed::No) {
        for subject in coverage.subjects_holding(path) {
            state.untick_subject(subject);
            unticked.push(subject.clone());
        }
    }
    for (path, _) in changed.iter().filter(|(_, now)| *now == Viewed::Yes) {
        for subject in coverage.subjects_holding(path) {
            let files = coverage.files_of(subject);
            if !state.is_read(subject) && files.iter().all(|path| is_viewed(path) == Viewed::Yes) {
                state.tick_subject(subject, tour_head);
            }
        }
    }
    for (path, now) in on_host {
        if !unsent.contains_key(path) {
            state.saw_on_host(path, *now);
        }
    }
    unticked
}

/// Which files each read subject holds, and which files have changes no step holds. In a basic
/// tour each file is held by itself and nothing is partly unheld.
#[derive(Default)]
pub(super) struct Coverage {
    files_by_subject: BTreeMap<ReadSubject, BTreeSet<FilePath>>,
    partly_unheld: BTreeSet<FilePath>,
}

impl Coverage {
    /// A step holds a file when it shows any lines of it; a file is partly unheld when the
    /// safety net still has lines of it.
    pub(super) fn of(tour: &GuidedTour, files: &[ChangedFile]) -> Self {
        let mut coverage = Self::default();
        for step in tour.steps() {
            let paths = step
                .pieces
                .iter()
                .flat_map(|pointer| coverage::shown_parts(pointer, tour, files))
                .map(|part| part.piece.id.path().clone());
            coverage
                .files_by_subject
                .entry(ReadSubject::Step(step.id.clone()))
                .or_default()
                .extend(paths);
        }
        coverage.partly_unheld = coverage::safety_net(tour, files)
            .into_iter()
            .map(|part| part.piece.id.path().clone())
            .collect();
        coverage
    }

    /// A basic tour holds every changed file by itself.
    pub(super) fn of_files<'a>(paths: impl IntoIterator<Item = &'a FilePath>) -> Self {
        Self {
            files_by_subject: paths
                .into_iter()
                .map(|path| {
                    (
                        ReadSubject::File(path.clone()),
                        BTreeSet::from([path.clone()]),
                    )
                })
                .collect(),
            partly_unheld: BTreeSet::new(),
        }
    }

    /// Every file a read subject holds, once per subject that holds it.
    fn held_files(&self) -> impl Iterator<Item = &FilePath> {
        self.files_by_subject.values().flatten()
    }

    fn files_of(&self, subject: &ReadSubject) -> &BTreeSet<FilePath> {
        static NONE: BTreeSet<FilePath> = BTreeSet::new();
        self.files_by_subject.get(subject).unwrap_or(&NONE)
    }

    fn subjects_holding<'a>(&'a self, path: &'a FilePath) -> impl Iterator<Item = &'a ReadSubject> {
        self.files_by_subject
            .iter()
            .filter(move |(_, files)| files.contains(path))
            .map(|(subject, _)| subject)
    }

    /// A file counts as read only when no change of it is left outside the steps.
    fn is_fully_ticked(&self, path: &FilePath, is_read: impl Fn(&ReadSubject) -> bool) -> bool {
        !self.partly_unheld.contains(path) && self.subjects_holding(path).all(is_read)
    }
}

pub struct SyncRequest {
    pr: PullRequestUrl,
    tour_head: Sha,
    unsent: BTreeMap<FilePath, Viewed>,
    /// Set for the first sync after a re-pin; it replaces `unsent`.
    baseline_wishes: Option<BTreeMap<FilePath, Viewed>>,
    is_off: bool,
}

/// What the network part of a sync learned and did; the review takes it in with `apply_sync`.
pub struct SyncOutcome {
    tour_head: Sha,
    sent: Vec<(FilePath, Viewed)>,
    on_host: Option<BTreeMap<FilePath, Viewed>>,
    baseline_wishes: Option<BTreeMap<FilePath, Viewed>>,
    error: Option<HostError>,
}

impl SyncOutcome {
    pub(super) fn is_for(&self, tour_head: &Sha) -> bool {
        self.tour_head == *tour_head
    }

    fn nothing(tour_head: &Sha) -> Self {
        Self {
            tour_head: tour_head.clone(),
            sent: Vec::new(),
            on_host: None,
            baseline_wishes: None,
            error: None,
        }
    }

    fn failed(tour_head: &Sha, error: HostError) -> Self {
        Self {
            error: Some(error),
            ..Self::nothing(tour_head)
        }
    }

    /// The queued marks were pushed, then GitHub's marks read back.
    fn pushed(
        tour_head: &Sha,
        (sent, push_error): Pushed,
        read: Result<BTreeMap<FilePath, Viewed>, HostError>,
    ) -> Self {
        let (on_host, read_error) = match read {
            Ok(on_host) => (Some(on_host), None),
            Err(error) => (None, Some(error)),
        };
        Self {
            sent,
            on_host,
            error: push_error.or(read_error),
            ..Self::nothing(tour_head)
        }
    }

    /// After a re-pin: GitHub's marks were read first, then the wishes they did not match pushed.
    fn baseline_taken(
        tour_head: &Sha,
        wishes: BTreeMap<FilePath, Viewed>,
        on_host: BTreeMap<FilePath, Viewed>,
        (sent, error): Pushed,
    ) -> Self {
        Self {
            sent,
            on_host: Some(on_host),
            baseline_wishes: Some(wishes),
            error,
            ..Self::nothing(tour_head)
        }
    }

    /// Records what was sent and what GitHub says. Returns what it unticked, and the error of the
    /// network part.
    fn land(
        self,
        state: &mut ReviewState,
        coverage: &Coverage,
        tour_head: &Sha,
    ) -> (Vec<ReadSubject>, Option<HostError>) {
        let mut unticked = Vec::new();
        match (&self.baseline_wishes, &self.on_host) {
            (Some(wishes), Some(on_host)) => {
                state.take_host_baseline(on_host, wishes);
                for (path, viewed) in &self.sent {
                    state.sent_viewed(path, *viewed);
                }
            }
            (Some(_), None) => {}
            (None, on_host) => {
                for (path, viewed) in &self.sent {
                    state.sent_viewed(path, *viewed);
                }
                if let Some(on_host) = on_host {
                    unticked = pull_viewed(state, coverage, tour_head, on_host);
                }
            }
        }
        (unticked, self.error)
    }
}

/// What a push sent, and the failure it stopped at.
type Pushed = (Vec<(FilePath, Viewed)>, Option<HostError>);

impl SyncRequest {
    /// No sync while sync is off, or while the tour is behind the PR head.
    pub fn run(self, host: &dyn CodeHost) -> SyncOutcome {
        let pr = &self.pr;
        if self.is_off {
            return SyncOutcome::nothing(&self.tour_head);
        }
        let Some(marks) = host.viewed_marks() else {
            return SyncOutcome::failed(&self.tour_head, HostError::not_offered("viewed marks"));
        };
        match host.pull_request_tips(pr) {
            Ok(tips) if tips.head == self.tour_head => {}
            Ok(_) => return SyncOutcome::nothing(&self.tour_head),
            Err(error) => return SyncOutcome::failed(&self.tour_head, error),
        }
        if let Some(wishes) = self.baseline_wishes {
            Self::take_baseline(marks, pr, &self.tour_head, wishes)
        } else {
            let pushed = push(marks, pr, &self.unsent);
            SyncOutcome::pushed(&self.tour_head, pushed, read_viewed(marks, pr))
        }
    }

    /// GitHub is read first, so only wishes it does not already match are pushed.
    fn take_baseline(
        host: &dyn ViewedFilesPort,
        pr: &PullRequestUrl,
        tour_head: &Sha,
        wishes: BTreeMap<FilePath, Viewed>,
    ) -> SyncOutcome {
        let on_host = match read_viewed(host, pr) {
            Ok(on_host) => on_host,
            Err(error) => return SyncOutcome::failed(tour_head, error),
        };
        let differing: BTreeMap<FilePath, Viewed> = wishes
            .iter()
            .filter(|(path, viewed)| on_host.get(*path).copied().unwrap_or(Viewed::No) != **viewed)
            .map(|(path, viewed)| (path.clone(), *viewed))
            .collect();
        let pushed = push(host, pr, &differing);
        SyncOutcome::baseline_taken(tour_head, wishes, on_host, pushed)
    }
}

/// Stops at the first failure; what was not sent stays queued.
fn push(
    host: &dyn ViewedFilesPort,
    pr: &PullRequestUrl,
    wishes: &BTreeMap<FilePath, Viewed>,
) -> Pushed {
    let mut sent = Vec::new();
    for (path, viewed) in wishes {
        if let Err(error) = host.set_file_viewed(pr, path, *viewed) {
            return (sent, Some(error));
        }
        sent.push((path.clone(), *viewed));
    }
    (sent, None)
}

fn read_viewed(
    host: &dyn ViewedFilesPort,
    pr: &PullRequestUrl,
) -> Result<BTreeMap<FilePath, Viewed>, HostError> {
    Ok(host
        .viewed_files(pr)?
        .into_iter()
        .map(|file| (file.path, file.state.viewed()))
        .collect())
}
