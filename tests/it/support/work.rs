//! Runs a review's due work the way the app's review session does, in line instead of off the UI thread.

use std::sync::Arc;
use std::time::Instant;

use guided_review::code_source::CodeSource;
use guided_review::viewer::{AfterLanding, GitHubPort, Review, TourAgents, WorkKind, WorkPorts};

use super::fake_tour_agent::FakeTourAgent;

/// Runs what is due at `now`, lands each outcome and asks again, until nothing is due at `now`.
/// Returns each work run, in the order it ran, with what the app was asked to do after it.
pub fn drive_until_idle(
    review: &mut Review,
    ports: &WorkPorts<'_>,
    now: Instant,
) -> Vec<(WorkKind, AfterLanding)> {
    let mut ran = Vec::new();
    for _ in 0..100 {
        let due = review.due_work(now);
        if due.is_empty() {
            return ran;
        }
        for work in due {
            let kind = work.kind();
            let outcome = work.run(ports);
            ran.push((kind, review.land(outcome)));
        }
    }
    panic!("work kept falling due: {ran:?}");
}

/// For work that hands nothing to a tour agent.
pub fn drive_without_agents(
    review: &mut Review,
    github: &dyn GitHubPort,
    source: &dyn CodeSource,
    now: Instant,
) -> Vec<WorkKind> {
    let agents = TourAgents::new(Arc::new(FakeTourAgent::new()));
    let ports = WorkPorts {
        github,
        source,
        agents: &agents,
    };
    kinds(&drive_until_idle(review, &ports, now))
}

pub fn kinds(ran: &[(WorkKind, AfterLanding)]) -> Vec<WorkKind> {
    ran.iter().map(|(kind, _)| *kind).collect()
}
