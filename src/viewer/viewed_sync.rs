use std::collections::{BTreeMap, BTreeSet};

use crate::coverage;
use crate::file_path::FilePath;
use crate::pieces::ChangedFile;
use crate::pull_request::{PullRequestUrl, Sha};
use crate::tour::Tour;

use super::ports::{PullRequestTipsPort, ViewedFilesPort, ViewedState, ViewedSyncUnavailable};
use super::review::StepId;
use super::review_state::ReviewState;

/// Keeps GitHub's "viewed" marks and the read ticks in step. Ticks keep working when it is off.
#[derive(Default)]
pub(super) struct ViewedSync {
    /// Why sync is switched off for this session; the app shows it as a banner.
    off_because: Option<String>,
}

impl ViewedSync {
    pub(super) fn banner(&self) -> Option<&str> {
        self.off_because.as_deref()
    }

    pub(super) fn not_synced(&self, state: &ReviewState) -> bool {
        self.off_because.is_none() && !state.unsent_viewed().is_empty()
    }

    pub(super) fn wish_after_tick(
        &self,
        state: &mut ReviewState,
        coverage: &Coverage,
        step: &StepId,
    ) {
        if self.off_because.is_some() {
            return;
        }
        let fully_ticked: Vec<&FilePath> = coverage
            .files_of(step)
            .iter()
            .filter(|file| coverage.is_fully_ticked(file, |step| is_ticked(state, step)))
            .collect();
        for file in fully_ticked {
            state.want_viewed(file, true);
        }
    }

    pub(super) fn wish_after_untick(
        &self,
        state: &mut ReviewState,
        coverage: &Coverage,
        step: &StepId,
    ) {
        if self.off_because.is_some() {
            return;
        }
        for file in coverage.files_of(step) {
            state.want_viewed(file, false);
        }
    }

    /// Everything the network part of a sync needs, owned, so it can run on a background thread.
    pub(super) fn request(
        &self,
        state: &ReviewState,
        coverage: &Coverage,
        pr: &PullRequestUrl,
        tour_head: &Sha,
    ) -> SyncRequest {
        let baseline_wishes = state
            .github_baseline_due()
            .then(|| baseline_wishes(state, coverage));
        SyncRequest {
            pr: pr.clone(),
            tour_head: tour_head.clone(),
            unsent: state.unsent_viewed().clone(),
            baseline_wishes,
            is_off: self.off_because.is_some(),
        }
    }

    /// Takes in what GitHub said. Returns the steps it unticked, and the error of the network part.
    pub(super) fn take_in(
        &self,
        state: &mut ReviewState,
        coverage: &Coverage,
        tour_head: &Sha,
        outcome: SyncOutcome,
    ) -> (Vec<StepId>, Option<anyhow::Error>) {
        let mut unticked = Vec::new();
        match (&outcome.baseline_wishes, &outcome.on_github) {
            (Some(wishes), Some(on_github)) => {
                state.take_github_baseline(on_github, wishes);
                for (path, viewed) in &outcome.sent {
                    state.sent_viewed(path, *viewed);
                }
            }
            (Some(_), None) => {}
            (None, on_github) => {
                for (path, viewed) in &outcome.sent {
                    state.sent_viewed(path, *viewed);
                }
                if let Some(on_github) = on_github {
                    unticked = pull_viewed(state, coverage, tour_head, on_github);
                }
            }
        }
        (unticked, outcome.error)
    }

    /// GitHub saying viewed sync is unavailable switches it off for the session instead of failing.
    pub(super) fn settle(&mut self, result: anyhow::Result<()>) -> anyhow::Result<()> {
        match result {
            Err(error) => match error.downcast_ref::<ViewedSyncUnavailable>() {
                Some(unavailable) => {
                    self.off_because = Some(unavailable.to_string());
                    Ok(())
                }
                None => Err(error),
            },
            ok => ok,
        }
    }

    pub(super) fn is_off(&self) -> bool {
        self.off_because.is_some()
    }
}

fn is_ticked(state: &ReviewState, step: &StepId) -> bool {
    state.read_tick(step).is_some()
}

/// After a re-pin: viewed for every file whose steps are all still ticked, and the queued
/// "not viewed" edits for the rest. Queued "viewed" edits are dropped: the ticks say it again
/// where it is still true.
fn baseline_wishes(state: &ReviewState, coverage: &Coverage) -> BTreeMap<FilePath, bool> {
    let is_fully_ticked =
        |path: &FilePath| coverage.is_fully_ticked(path, |step| is_ticked(state, step));
    let still_viewed = coverage
        .files_by_step
        .values()
        .flatten()
        .filter(|path| is_fully_ticked(path))
        .map(|path| (path.clone(), true));
    let still_not_viewed = state
        .unsent_viewed()
        .iter()
        .filter(|(path, viewed)| !**viewed && !is_fully_ticked(path))
        .map(|(path, _)| (path.clone(), false));
    still_viewed.chain(still_not_viewed).collect()
}

/// Reacts only to files whose state changed since the last sync, and never to files with an
/// unsent local change: those win. Returns the steps it unticked.
fn pull_viewed(
    state: &mut ReviewState,
    coverage: &Coverage,
    tour_head: &Sha,
    on_github: &BTreeMap<FilePath, bool>,
) -> Vec<StepId> {
    let unsent = state.unsent_viewed().clone();
    let is_viewed = |path: &FilePath| {
        unsent
            .get(path)
            .or_else(|| on_github.get(path))
            .copied()
            .unwrap_or(false)
    };
    let changed: Vec<(&FilePath, bool)> = on_github
        .iter()
        .filter(|(path, now)| {
            !unsent.contains_key(*path) && state.was_viewed_on_github(path) != **now
        })
        .map(|(path, now)| (path, *now))
        .collect();

    let mut unticked = Vec::new();
    for (path, _) in changed.iter().filter(|(_, now)| !now) {
        for step in coverage.steps_holding(path) {
            state.untick(step);
            unticked.push(step.clone());
        }
    }
    for (path, _) in changed.iter().filter(|(_, now)| *now) {
        for step in coverage.steps_holding(path) {
            let files = coverage.files_of(step);
            if !is_ticked(state, step) && files.iter().all(&is_viewed) {
                state.tick(step, tour_head);
            }
        }
    }
    for (path, now) in on_github {
        if !unsent.contains_key(path) {
            state.saw_on_github(path, *now);
        }
    }
    unticked
}

/// Which files each step holds, and which files have changes no step holds.
#[derive(Default)]
pub(super) struct Coverage {
    files_by_step: BTreeMap<StepId, BTreeSet<FilePath>>,
    partly_unheld: BTreeSet<FilePath>,
}

impl Coverage {
    /// A step holds a file when it shows any lines of it; a file is partly unheld when the
    /// safety net still has lines of it.
    pub(super) fn of(tour: &Tour, files: &[ChangedFile]) -> Self {
        let mut coverage = Self::default();
        for step in tour.steps() {
            let paths = step
                .pieces
                .iter()
                .flat_map(|pointer| coverage::shown_parts(pointer, tour, files))
                .map(|part| part.piece.id.path().clone());
            coverage
                .files_by_step
                .entry(step.id.clone())
                .or_default()
                .extend(paths);
        }
        coverage.partly_unheld = coverage::safety_net(tour, files)
            .into_iter()
            .map(|part| part.piece.id.path().clone())
            .collect();
        coverage
    }

    fn files_of(&self, step: &StepId) -> &BTreeSet<FilePath> {
        static NONE: BTreeSet<FilePath> = BTreeSet::new();
        self.files_by_step.get(step).unwrap_or(&NONE)
    }

    fn steps_holding<'a>(&'a self, path: &'a FilePath) -> impl Iterator<Item = &'a StepId> {
        self.files_by_step
            .iter()
            .filter(move |(_, files)| files.contains(path))
            .map(|(step, _)| step)
    }

    /// A file counts as read only when no change of it is left outside the steps.
    fn is_fully_ticked(&self, path: &FilePath, is_ticked: impl Fn(&StepId) -> bool) -> bool {
        !self.partly_unheld.contains(path) && self.steps_holding(path).all(is_ticked)
    }
}

pub struct SyncRequest {
    pr: PullRequestUrl,
    tour_head: Sha,
    unsent: BTreeMap<FilePath, bool>,
    /// Set for the first sync after a re-pin; it replaces `unsent`.
    baseline_wishes: Option<BTreeMap<FilePath, bool>>,
    is_off: bool,
}

/// What the network part of a sync learned and did; the review takes it in with `apply_sync`.
pub struct SyncOutcome {
    tour_head: Sha,
    sent: Vec<(FilePath, bool)>,
    on_github: Option<BTreeMap<FilePath, bool>>,
    baseline_wishes: Option<BTreeMap<FilePath, bool>>,
    error: Option<anyhow::Error>,
}

impl SyncOutcome {
    pub(super) fn is_for(&self, tour_head: &Sha) -> bool {
        self.tour_head == *tour_head
    }

    fn nothing(tour_head: &Sha) -> Self {
        Self {
            tour_head: tour_head.clone(),
            sent: Vec::new(),
            on_github: None,
            baseline_wishes: None,
            error: None,
        }
    }
}

impl SyncRequest {
    /// No sync while sync is off, or while the tour is behind the PR head.
    pub fn run(
        self,
        github: &(impl PullRequestTipsPort + ViewedFilesPort + ?Sized),
    ) -> SyncOutcome {
        let pr = &self.pr;
        if self.is_off {
            return SyncOutcome::nothing(&self.tour_head);
        }
        match github.pull_request_tips(pr) {
            Ok(tips) if tips.head == self.tour_head => {}
            Ok(_) => return SyncOutcome::nothing(&self.tour_head),
            Err(error) => {
                return SyncOutcome {
                    error: Some(error),
                    ..SyncOutcome::nothing(&self.tour_head)
                };
            }
        }
        if let Some(wishes) = self.baseline_wishes {
            Self::take_baseline(github, pr, &self.tour_head, wishes)
        } else {
            let mut outcome = SyncOutcome::nothing(&self.tour_head);
            outcome.error = push(github, pr, &self.unsent, &mut outcome.sent).err();
            match read_viewed(github, pr) {
                Ok(on_github) => outcome.on_github = Some(on_github),
                Err(error) => outcome.error = outcome.error.or(Some(error)),
            }
            outcome
        }
    }

    /// GitHub is read first, so only wishes it does not already match are pushed.
    fn take_baseline(
        github: &(impl PullRequestTipsPort + ViewedFilesPort + ?Sized),
        pr: &PullRequestUrl,
        tour_head: &Sha,
        wishes: BTreeMap<FilePath, bool>,
    ) -> SyncOutcome {
        let on_github = match read_viewed(github, pr) {
            Ok(on_github) => on_github,
            Err(error) => {
                return SyncOutcome {
                    error: Some(error),
                    ..SyncOutcome::nothing(tour_head)
                };
            }
        };
        let differing: BTreeMap<FilePath, bool> = wishes
            .iter()
            .filter(|(path, viewed)| on_github.get(*path).copied().unwrap_or(false) != **viewed)
            .map(|(path, viewed)| (path.clone(), *viewed))
            .collect();
        let mut outcome = SyncOutcome::nothing(tour_head);
        outcome.error = push(github, pr, &differing, &mut outcome.sent).err();
        outcome.on_github = Some(on_github);
        outcome.baseline_wishes = Some(wishes);
        outcome
    }
}

/// Stops at the first failure; what was not sent stays queued.
fn push(
    github: &(impl ViewedFilesPort + ?Sized),
    pr: &PullRequestUrl,
    wishes: &BTreeMap<FilePath, bool>,
    sent: &mut Vec<(FilePath, bool)>,
) -> anyhow::Result<()> {
    for (path, viewed) in wishes {
        github.set_file_viewed(pr, path, *viewed)?;
        sent.push((path.clone(), *viewed));
    }
    Ok(())
}

fn read_viewed(
    github: &(impl ViewedFilesPort + ?Sized),
    pr: &PullRequestUrl,
) -> anyhow::Result<BTreeMap<FilePath, bool>> {
    Ok(github
        .viewed_files(pr)?
        .into_iter()
        .map(|file| (file.path, file.state == ViewedState::Viewed))
        .collect())
}
