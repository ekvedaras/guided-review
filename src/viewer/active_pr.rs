use super::pr_switcher::TourFact;
use super::tour_agent::{AgentStatus, TerminalId};
use super::tour_writing::TourWriting;
use crate::pull_request::PullRequestUrl;

/// Which PR the window shows, and what an incoming open request does to it. A PR that is built takes
/// the screen only when it is the newest request, so the one on screen stays until then. It also keeps
/// the tours being written, the one the loading screen waits for, and the PRs with an agent reply the
/// reviewer has not seen.
#[derive(Default)]
pub struct ActivePr {
    shown: Option<PullRequestUrl>,
    building: Option<(BuildTicket, PullRequestUrl)>,
    issued: u64,
    writings: Vec<(PullRequestUrl, TourWriting)>,
    /// The PR whose writing the loading screen shows.
    waiting_for: Option<PullRequestUrl>,
    unseen_replies: Vec<PullRequestUrl>,
}

/// Names one build, so its result can be told from the result of an older one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuildTicket(u64);

/// Who asked to open a PR.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenOrigin {
    /// The reviewer, or an agent they started: the window goes where they ask.
    Reviewer,
    /// A tour agent the app started. It writes or fixes tours on its own time, so its open never pulls
    /// the window away from what the reviewer reads.
    AppStartedAgent,
}

pub enum OpenDecision {
    /// The PR is on screen: read its tour again.
    Reload,
    /// Build the PR's review in the background, then hand the result to `finished`.
    Build(BuildTicket),
    /// Not shown now: the switcher marks the PR ready instead.
    KeepInBackground,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteDecision {
    /// Start a tour agent to write the tour, then hand the result to `writer_started`.
    StartWriter,
    /// A writing is on already; the loading screen shows it.
    Watch,
}

pub enum Finished<Built> {
    /// The PR now shown is the built one; swap the screen to it and drop the old one.
    Switch(Built),
    /// The build failed; what is on screen stays.
    Stay { reason: String },
    /// A newer request came while this one was building.
    Superseded,
}

impl ActivePr {
    pub fn shown(&self) -> Option<&PullRequestUrl> {
        self.shown.as_ref()
    }

    /// An open of a PR being written is its writer saying the tour is checked, which ends the writing.
    pub fn request(&mut self, pr: PullRequestUrl, origin: OpenOrigin) -> OpenDecision {
        let was_written = self.writings.iter().any(|(writing, _)| writing == &pr);
        self.writings.retain(|(writing, _)| writing != &pr);
        if self.shown.as_ref() == Some(&pr) {
            self.issued += 1;
            self.building = None;
            return OpenDecision::Reload;
        }
        let is_awaited = self.waiting_for.as_ref() == Some(&pr);
        let is_agents_own = was_written || origin == OpenOrigin::AppStartedAgent;
        if is_agents_own && !is_awaited {
            return OpenDecision::KeepInBackground;
        }
        self.waiting_for = None;
        self.issued += 1;
        let ticket = BuildTicket(self.issued);
        self.building = Some((ticket, pr));
        OpenDecision::Build(ticket)
    }

    pub fn finished<Built>(
        &mut self,
        ticket: BuildTicket,
        built: anyhow::Result<Built>,
    ) -> Finished<Built> {
        let Some((current, pr)) = self.building.take() else {
            return Finished::Superseded;
        };
        if current != ticket {
            self.building = Some((current, pr));
            return Finished::Superseded;
        }
        match built {
            Ok(built) => {
                self.unseen_replies.retain(|unseen| unseen != &pr);
                self.shown = Some(pr);
                Finished::Switch(built)
            }
            Err(error) => Finished::Stay {
                reason: format!("could not open {pr}: {error:#}"),
            },
        }
    }

    /// The reviewer picked a PR with no tour: the loading screen shows its writing, which starts unless
    /// one is on already. A build still running for another PR is superseded.
    pub fn write(&mut self, pr: PullRequestUrl) -> WriteDecision {
        self.building = None;
        self.waiting_for = Some(pr.clone());
        if self.writing_of(&pr).is_some() {
            return WriteDecision::Watch;
        }
        self.writings.push((pr, TourWriting::starting()));
        WriteDecision::StartWriter
    }

    /// Starts the PR's writing over, as after a failure.
    pub fn try_again(&mut self, pr: &PullRequestUrl) {
        self.writings.retain(|(writing, _)| writing != pr);
        self.write(pr.clone());
    }

    pub fn writer_started(&mut self, pr: &PullRequestUrl, started: anyhow::Result<TerminalId>) {
        if let Some(writing) = self.writing_mut(pr) {
            writing.started(started);
        }
    }

    pub fn writer_status(&mut self, pr: &PullRequestUrl, status: AgentStatus) {
        if let Some(writing) = self.writing_mut(pr) {
            writing.status_is(status);
        }
    }

    pub fn writer_screen(&mut self, pr: &PullRequestUrl, lines: Vec<String>) {
        if let Some(writing) = self.writing_mut(pr) {
            writing.screen_is(lines);
        }
    }

    /// Herdr no longer follows the writer: its terminal ended.
    pub fn writer_gone(&mut self, pr: &PullRequestUrl) {
        if let Some(writing) = self.writing_mut(pr) {
            writing.gone();
        }
    }

    /// The reviewer leaves the loading screen for the PR shown behind it; the writing goes on.
    pub fn stop_waiting(&mut self) {
        self.waiting_for = None;
    }

    /// The writing the loading screen shows.
    pub fn loading(&self) -> Option<(&PullRequestUrl, &TourWriting)> {
        let pr = self.waiting_for.as_ref()?;
        Some((pr, self.writing_of(pr)?))
    }

    pub fn writing_of(&self, pr: &PullRequestUrl) -> Option<&TourWriting> {
        self.writings
            .iter()
            .find(|(writing, _)| writing == pr)
            .map(|(_, writing)| writing)
    }

    /// What the switcher says of the PR's tour beyond what the disk says, while it is being written.
    pub fn tour_fact_of(&self, pr: &PullRequestUrl) -> Option<TourFact> {
        self.writing_of(pr).map(|writing| {
            if writing.has_failed() {
                TourFact::WritingFailed
            } else {
                TourFact::Writing
            }
        })
    }

    /// The tour agent answered a hand-off of the PR. Unless the PR is on screen, the switcher marks it.
    pub fn agent_replied(&mut self, pr: &PullRequestUrl) {
        if self.shown.as_ref() != Some(pr) && !self.has_unseen_reply(pr) {
            self.unseen_replies.push(pr.clone());
        }
    }

    pub fn has_unseen_reply(&self, pr: &PullRequestUrl) -> bool {
        self.unseen_replies.contains(pr)
    }

    fn writing_mut(&mut self, pr: &PullRequestUrl) -> Option<&mut TourWriting> {
        self.writings
            .iter_mut()
            .find(|(writing, _)| writing == pr)
            .map(|(_, writing)| writing)
    }
}
