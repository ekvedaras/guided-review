use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::Review;
use crate::viewer::ai_review::AiReview;
use crate::viewer::ai_review_run::AiReviewRun;
use crate::viewer::fix_requests::FixState;
use crate::viewer::guided_tour::GuidedTourProgress;
use crate::viewer::pr_reviewers::StanceAge;
use crate::viewer::stations::{Station, StationHealth, StationKind, Stations, waited};
use crate::viewer::tour_agent::{AgentStatus, TourAgentState};
use crate::viewer::tour_writing::WritingStage;

use StationHealth::{Done, Failed, NotStarted, Waiting, Working};

fn epoch_seconds(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

fn counted(count: usize, one: &str, many: &str) -> String {
    match count {
        1 => format!("1 {one}"),
        count => format!("{count} {many}"),
    }
}

impl Review {
    /// How each station of the review stands, for the sidebar's stepper.
    pub fn stations(&self, tour_agent: &TourAgentState, now: SystemTime) -> Stations {
        let mut stations = vec![
            self.tour_station(tour_agent),
            self.ai_review_station(),
            self.fixes_station(),
            self.your_review_station(),
        ];
        if self.subject.pull_request().is_some() {
            stations.push(self.github_station(now));
        }
        Stations::new(stations).with_problems(&self.problems())
    }

    /// Notes when viewed marks started to wait for GitHub, so the sidebar can say for how long.
    pub fn time_viewed_wait(&mut self, now: SystemTime) -> anyhow::Result<()> {
        let since = if self.not_synced() {
            self.state
                .viewed_waiting_since()
                .or_else(|| Some(epoch_seconds(now)))
        } else {
            None
        };
        if since != self.state.viewed_waiting_since() {
            self.state
                .record(|state| state.set_viewed_waiting_since(since))?;
        }
        Ok(())
    }

    fn tour_station(&self, agent: &TourAgentState) -> Station {
        let station = |health, state: &str| Station::new(StationKind::Tour, health, state);
        if self.is_basic() {
            return match &self.guided_tour {
                GuidedTourProgress::NotAsked => station(NotStarted, "not written"),
                GuidedTourProgress::Writing(writing) => match (&writing.stage, writing.status) {
                    (_, Some(AgentStatus::WaitingForYou)) => {
                        station(Waiting, "agent waits for you")
                    }
                    (WritingStage::Starting, _) => station(Working, "starting"),
                    _ => station(Working, "being written"),
                },
                GuidedTourProgress::Ready => station(Waiting, "ready to switch to"),
                GuidedTourProgress::Failed(_) => station(Failed, "not written"),
            };
        }
        match agent {
            TourAgentState::LinkOff(_) => station(Failed, "agent link off"),
            TourAgentState::Choose(_) => station(Waiting, "pick an agent"),
            TourAgentState::Agent { status, .. } => match status {
                AgentStatus::Working => station(Working, "agent working"),
                AgentStatus::WaitingForYou => station(Waiting, "agent waits for you"),
                AgentStatus::Failed => station(Failed, "agent failed"),
                AgentStatus::Idle => station(Done, "written · agent idle"),
                AgentStatus::Unreported => station(Done, "written"),
            },
            TourAgentState::Unknown | TourAgentState::None => station(Done, "written"),
        }
    }

    fn ai_review_station(&self) -> Station {
        let station = |health, state: &str| Station::new(StationKind::AiReview, health, state);
        match &self.ai_review_run {
            AiReviewRun::Starting => station(Working, "starting"),
            AiReviewRun::Running(running) => match running.status {
                Some(AgentStatus::WaitingForYou) => station(Waiting, "agent waits for you"),
                _ => station(Working, "reviewing"),
            },
            AiReviewRun::Failed { .. } => station(Failed, "run failed"),
            AiReviewRun::NotRun => match &self.ai_review {
                AiReview::NotRun => station(NotStarted, "not run"),
                AiReview::Unreadable(_) => station(Failed, "findings not read"),
                AiReview::Findings(_) => match self.ai_review.counts().total() {
                    0 => station(Done, "no findings"),
                    count => station(Done, &counted(count, "finding", "findings")),
                },
            },
        }
    }

    fn fixes_station(&self) -> Station {
        let station = |health, state: String| Station::new(StationKind::Fixes, health, state);
        let states: Vec<&FixState> = self.state.fix_requests().latest_states().collect();
        let count =
            |wanted: fn(&FixState) -> bool| states.iter().filter(|state| wanted(state)).count();
        let asked = count(|state| matches!(state, FixState::Asked));
        let to_send = self.fixes_to_send();
        let fixing = count(|state| state.is_in_progress() && !matches!(state, FixState::Asked));
        let fixed = count(|state| matches!(state, FixState::Fixed { .. }));
        if asked > 0 {
            station(
                Waiting,
                counted(asked, "question for you", "questions for you"),
            )
        } else if to_send > 0 {
            station(Waiting, format!("{to_send} to send"))
        } else if fixing > 0 {
            station(Working, format!("{fixing} being fixed"))
        } else if fixed > 0 {
            station(Done, format!("{fixed} fixed"))
        } else {
            station(NotStarted, "none asked".to_owned())
        }
    }

    fn your_review_station(&self) -> Station {
        let station = |health, state: String| Station::new(StationKind::YourReview, health, state);
        let progress = self.progress();
        let is_everything_read = progress.total > 0 && progress.read == progress.total;
        if let Some(queued) = self.finishes_with_send_tasks() {
            return match (queued, is_everything_read) {
                (0, _) => station(NotStarted, "no tasks".to_owned()),
                (queued, true) => {
                    station(Waiting, counted(queued, "task to send", "tasks to send"))
                }
                (queued, false) => {
                    station(NotStarted, counted(queued, "task queued", "tasks queued"))
                }
            };
        }
        match self.your_review() {
            Some(yours) if yours.is_marked_done => station(Done, "marked done".to_owned()),
            Some(yours) if yours.age == StanceAge::BeforeNewCommits => {
                station(Waiting, "new commits since".to_owned())
            }
            Some(yours) => station(Done, yours.title().to_owned()),
            None if !self.can_finish_review() => station(NotStarted, "nothing to send".to_owned()),
            None if is_everything_read => station(Waiting, "ready to send".to_owned()),
            None => station(NotStarted, "not sent yet".to_owned()),
        }
    }

    fn github_station(&self, now: SystemTime) -> Station {
        let station = |health, state: String| Station::new(StationKind::GitHub, health, state);
        if self.sync_banner().is_some() {
            return station(NotStarted, "viewed marks off".to_owned());
        }
        if !self.not_synced() {
            return station(Done, "viewed marks in sync".to_owned());
        }
        let marks = counted(
            self.state.unsent_viewed().len(),
            "viewed mark waiting",
            "viewed marks waiting",
        );
        let time = self
            .state
            .viewed_waiting_since()
            .map_or(Duration::ZERO, |since| {
                Duration::from_secs(epoch_seconds(now).saturating_sub(since))
            });
        station(Waiting, format!("{marks} · {}", waited(time)))
    }
}
