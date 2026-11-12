//! A GitHub outage: GitHub does not answer reads that are fine, so what the screen shows may be
//! old. The reviewer is told once, whatever number of reads failed, and the reads are tried again
//! by themselves, further apart the longer the outage lasts.

use std::collections::BTreeMap;
use std::fmt;
use std::time::Duration;

use super::problems::Read;
use super::work_agenda::WorkKind;

/// Why GitHub is not answering. A refused token or a missing page is not one: those say something
/// the reviewer can put right, and trying again changes nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutageCause {
    ServerError(u16),
    TimedOut,
    NoConnection,
}

impl fmt::Display for OutageCause {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::ServerError(status) => write!(formatter, "{status}"),
            Self::TimedOut => formatter.write_str("timed out"),
            Self::NoConnection => formatter.write_str("no connection"),
        }
    }
}

/// What the screen may show old while GitHub does not answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Affected {
    NewCommits,
    Description,
    Comments,
    ViewedMarks,
    PendingReview,
    Authorship,
    LastReview,
}

impl Affected {
    /// The work that reads it again.
    pub(super) fn retried_by(self) -> WorkKind {
        match self {
            Self::NewCommits => WorkKind::CheckPrHead,
            Self::Description => WorkKind::PullPrDescription,
            Self::Comments => WorkKind::PullThreads,
            Self::ViewedMarks => WorkKind::SyncViewed,
            Self::PendingReview | Self::Authorship => WorkKind::PullPendingReview,
            Self::LastReview => WorkKind::PullLastReview,
        }
    }
}

impl From<Read> for Affected {
    fn from(read: Read) -> Self {
        match read {
            Read::PrHead => Self::NewCommits,
            Read::PrDescription => Self::Description,
            Read::PendingReview => Self::PendingReview,
            Read::Authorship => Self::Authorship,
            Read::LastReview => Self::LastReview,
        }
    }
}

impl fmt::Display for Affected {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str(match self {
            Self::NewCommits => "new commits",
            Self::Description => "the PR description",
            Self::Comments => "comments",
            Self::ViewedMarks => "viewed marks",
            Self::PendingReview => "the pending review",
            Self::Authorship => "who wrote the PR",
            Self::LastReview => "your last review",
        })
    }
}

/// The wait before a read that failed `failures` times in a row is tried again: 30 s, 1 min, 2 min,
/// and so on up to 5 min.
pub fn backoff(failures: u32) -> Duration {
    const FIRST: Duration = Duration::from_secs(30);
    const LONGEST: Duration = Duration::from_secs(300);
    let doublings = failures.saturating_sub(1).min(4);
    (FIRST * 2u32.pow(doublings)).min(LONGEST)
}

/// An outage as it stands: its latest cause and, for each read it keeps from going through, how
/// many times in a row that read failed. It is over when none is left.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outage {
    cause: OutageCause,
    failures: BTreeMap<Affected, u32>,
}

impl Outage {
    pub(super) fn new(cause: OutageCause) -> Self {
        Self {
            cause,
            failures: BTreeMap::new(),
        }
    }

    /// Returns the wait before the read is tried again.
    pub(super) fn failed(&mut self, cause: OutageCause, affected: Affected) -> Duration {
        self.cause = cause;
        let failures = self.failures.entry(affected).or_default();
        *failures += 1;
        backoff(*failures)
    }

    /// True when it was the last read the outage kept from going through.
    pub(super) fn answered(&mut self, affected: Affected) -> bool {
        self.failures.remove(&affected);
        self.failures.is_empty()
    }

    pub fn cause(&self) -> OutageCause {
        self.cause
    }

    pub fn affected(&self) -> impl Iterator<Item = Affected> + '_ {
        self.failures.keys().copied()
    }

    /// The soonest a read is tried again.
    pub fn retry_in(&self) -> Duration {
        self.failures
            .values()
            .map(|failures| backoff(*failures))
            .min()
            .unwrap_or_else(|| backoff(1))
    }
}

/// "30 s", "1 min".
pub(super) fn waiting(duration: Duration) -> String {
    let seconds = duration.as_secs();
    if seconds < 60 {
        format!("{seconds} s")
    } else {
        format!("{} min", seconds / 60)
    }
}
