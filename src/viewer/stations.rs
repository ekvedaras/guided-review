//! The stations of a review: the work around reading it (the tour, the AI review, the fixes, the
//! reviewer's own review, the host), each with how it stands, for the sidebar's stepper.

use std::time::Duration;

use super::problems::{Problem, ProblemKind};
use crate::pull_request::HostSite;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StationKind {
    Tour,
    AiReview,
    Fixes,
    YourReview,
    Host,
}

impl StationKind {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Tour => "Tour",
            Self::AiReview => "AI review",
            Self::Fixes => "Fixes",
            Self::YourReview => "Your review",
            Self::Host => "Code host",
        }
    }

    /// The name under the station's icon, where the row has room for one short word each.
    pub const fn short_name(self) -> &'static str {
        match self {
            Self::AiReview => "AI",
            Self::YourReview => "You",
            other => other.name(),
        }
    }
}

/// How a station stands, from worst to best for the reviewer's attention.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum StationHealth {
    Failed,
    /// Something waits: for the reviewer, or to go through.
    Waiting,
    Working,
    Done,
    NotStarted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Station {
    pub kind: StationKind,
    /// What the card and the tooltip call it: the PR's host's own name for the host station.
    pub name: &'static str,
    /// The name under the station's icon.
    pub short_name: &'static str,
    pub health: StationHealth,
    /// A few words on how it stands, as "3 findings" or "new commits".
    pub state: String,
}

impl Station {
    pub fn new(kind: StationKind, health: StationHealth, state: impl Into<String>) -> Self {
        Self {
            kind,
            name: kind.name(),
            short_name: kind.short_name(),
            health,
            state: state.into(),
        }
    }

    /// The station of the PR's host, named after it.
    pub fn of_host(host: HostSite, health: StationHealth, state: impl Into<String>) -> Self {
        Self {
            name: host.name(),
            short_name: host.name(),
            ..Self::new(StationKind::Host, health, state)
        }
    }

    pub fn needs_you(&self) -> bool {
        matches!(self.health, StationHealth::Failed | StationHealth::Waiting)
    }
}

/// The stations in the order the work goes: a local review has no host station.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stations {
    stations: Vec<Station>,
}

impl Stations {
    pub fn new(stations: Vec<Station>) -> Self {
        Self { stations }
    }

    pub fn all(&self) -> &[Station] {
        &self.stations
    }

    pub fn get(&self, kind: StationKind) -> Option<&Station> {
        self.stations.iter().find(|station| station.kind == kind)
    }

    /// The station whose card is open: the one the reviewer picked, else the first that failed, then
    /// the first that waits, then the first at work, else the AI review.
    pub fn open(&self, picked: Option<StationKind>) -> StationKind {
        if let Some(picked) = picked.filter(|picked| self.get(*picked).is_some()) {
            return picked;
        }
        self.stations
            .iter()
            .filter(|station| station.health <= StationHealth::Working)
            .min_by_key(|station| station.health)
            .map_or(StationKind::AiReview, |station| station.kind)
    }

    /// The stations besides the open one that failed or wait, so none hides behind its icon.
    pub fn also_needing_you(&self, open: StationKind) -> impl Iterator<Item = &Station> {
        self.stations
            .iter()
            .filter(move |station| station.kind != open && station.needs_you())
    }

    /// A snapshot scene draws a station as it would stand, with nothing asked of the review.
    #[cfg(feature = "snapshots")]
    pub fn stage(&mut self, staged: Station) {
        if let Some(station) = self
            .stations
            .iter_mut()
            .find(|station| station.kind == staged.kind)
        {
            *station = staged;
        }
    }

    /// The station whose card names a problem; `None` puts it above the stepper, as it is about
    /// this Mac. A host problem of a review with no host station goes there too.
    pub fn home_of(&self, problem: &Problem) -> Option<StationKind> {
        let home = match problem.kind() {
            ProblemKind::ThreadsNotRead
            | ProblemKind::HostNotAnswering
            | ProblemKind::NotRead(_)
            | ProblemKind::ViewedNotSynced
            | ProblemKind::HandOffNotSent
            | ProblemKind::ReviewingMarkNotChanged
            | ProblemKind::ReadinessNotChanged
            | ProblemKind::MergingNotChanged => StationKind::Host,
            ProblemKind::TourNotRefreshed
            | ProblemKind::TourNotReloaded
            | ProblemKind::ShowAgentFailed
            | ProblemKind::UseAgentFailed => StationKind::Tour,
            _ => return None,
        };
        self.get(home).map(|station| station.kind)
    }

    /// A station with problems of its own failed, whatever it said before. One problem is named;
    /// more are counted.
    pub fn with_problems(mut self, problems: &[Problem]) -> Self {
        let homed: Vec<(StationKind, ProblemKind)> = problems
            .iter()
            .filter_map(|problem| Some((self.home_of(problem)?, problem.kind())))
            .collect();
        for station in &mut self.stations {
            let own: Vec<ProblemKind> = homed
                .iter()
                .filter(|(home, _)| *home == station.kind)
                .map(|(_, kind)| *kind)
                .collect();
            station.state = match own.as_slice() {
                [] => continue,
                [kind] => short_state(*kind).to_owned(),
                many => format!("{} problems", many.len()),
            };
            station.health = StationHealth::Failed;
        }
        self
    }
}

/// A station's state when one problem is its own, in a few words.
const fn short_state(kind: ProblemKind) -> &'static str {
    match kind {
        ProblemKind::ThreadsNotRead => "threads not read",
        ProblemKind::HostNotAnswering => "not answering",
        ProblemKind::NotRead(_) => "read failed",
        ProblemKind::ViewedNotSynced => "viewed marks not synced",
        ProblemKind::HandOffNotSent => "comment not handed off",
        ProblemKind::ReviewingMarkNotChanged => "👀 not changed",
        ProblemKind::ReadinessNotChanged => "draft state not changed",
        ProblemKind::MergingNotChanged => "merge not changed",
        ProblemKind::TourNotRefreshed => "not refreshed",
        ProblemKind::TourNotReloaded => "not reloaded",
        ProblemKind::ShowAgentFailed => "agent not shown",
        ProblemKind::UseAgentFailed => "agent not used",
        _ => "1 problem",
    }
}

/// How long viewed marks have waited to reach the host, said in the sidebar.
pub fn waited(time: Duration) -> String {
    match time.as_secs() {
        0..60 => "under a minute".to_owned(),
        seconds @ 60..3600 => format!("{} min", seconds / 60),
        seconds => format!("{} h", seconds / 3600),
    }
}
