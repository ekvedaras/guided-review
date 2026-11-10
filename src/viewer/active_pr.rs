use super::app_state::{AppState, AppStateFile};
use super::hand_offs::{AgentReplied, HandOffId};
use super::pr_switcher::TourFact;
use super::tour_agent::{AgentStatus, TerminalId, TourAgentPort};
use super::tour_writing::TourWriting;
use crate::pull_request::PullRequestUrl;

/// Which PR the window shows, and what an incoming open request does to it. A PR that is built takes
/// the screen only when it is the newest request, so the one on screen stays until then. It also keeps
/// the tours being written, the one the loading screen waits for, and the PRs with an agent reply the
/// reviewer has not seen.
#[derive(Default)]
pub struct ActivePr {
    shown: Option<PullRequestUrl>,
    building: Option<Build>,
    replies_for_reopened: Vec<(HandOffId, AgentReplied)>,
    issued: u64,
    /// The PR the reviewer picked or a build is making ready, which the window does not show yet.
    switching: Option<PullRequestUrl>,
    writings: Vec<(PullRequestUrl, TourWriting)>,
    /// The PR whose writing the loading screen shows.
    waiting_for: Option<PullRequestUrl>,
    unseen_replies: Vec<PullRequestUrl>,
    remembered_in: Option<AppStateFile>,
}

struct Build {
    ticket: BuildTicket,
    pr: PullRequestUrl,
    /// Agent replies that came while the PR on screen reopened.
    held_replies: Vec<(HandOffId, AgentReplied)>,
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
    /// Starts from what the file kept, and keeps the file up to date from here on. A file that cannot be
    /// read is for the app's log: the app starts with nothing remembered.
    pub fn remembering_in(file: AppStateFile) -> Self {
        let saved = file.load().unwrap_or_else(|error| {
            eprintln!("could not read the app state: {error:#}");
            AppState::default()
        });
        Self {
            writings: saved
                .writings
                .iter()
                .filter_map(|url| url.parse().ok())
                .map(|pr| (pr, TourWriting::starting()))
                .collect(),
            unseen_replies: saved
                .unseen_replies
                .iter()
                .filter_map(|url| url.parse().ok())
                .collect(),
            remembered_in: Some(file),
            ..Self::default()
        }
    }

    /// After a restart, each remembered writing looks for its terminal by the PR's herdr label, never by
    /// a stored id. Returns the writings to follow again; one whose terminal is gone, or that herdr
    /// cannot be asked about, has failed with the reason.
    pub fn resume_writings(
        &mut self,
        port: &dyn TourAgentPort,
    ) -> Vec<(PullRequestUrl, TerminalId)> {
        let mut to_follow = Vec::new();
        for (pr, writing) in &mut self.writings {
            match port.terminals_of(pr) {
                Ok(terminals) => match terminals.into_iter().next() {
                    Some(terminal) => {
                        let status = port.status(&terminal).ok();
                        writing.resumed(terminal.clone(), status);
                        to_follow.push((pr.clone(), terminal));
                    }
                    None => writing.lost_in_restart(
                        "The app restarted while this tour was being written, and the tour agent's terminal is gone."
                            .to_owned(),
                    ),
                },
                Err(error) => writing.lost_in_restart(format!(
                    "The app restarted while this tour was being written, and herdr could not be asked for the tour agent's terminal: {error:#}"
                )),
            }
        }
        to_follow
    }

    fn remember(&self) {
        let Some(file) = &self.remembered_in else {
            return;
        };
        let urls = |prs: Vec<&PullRequestUrl>| prs.into_iter().map(ToString::to_string).collect();
        let state = AppState {
            writings: urls(self.writings.iter().map(|(pr, _)| pr).collect()),
            unseen_replies: urls(self.unseen_replies.iter().collect()),
        };
        if let Err(error) = file.save(&state) {
            eprintln!("could not keep the app state: {error:#}");
        }
    }

    pub fn shown(&self) -> Option<&PullRequestUrl> {
        self.shown.as_ref()
    }

    /// The PR the window is about to show: picked, or being built.
    pub fn switching_to(&self) -> Option<&PullRequestUrl> {
        self.switching.as_ref()
    }

    /// The reviewer picked a PR to switch to. This is known before the app's open request is answered,
    /// so the window can say so at once.
    pub fn picked(&mut self, pr: PullRequestUrl) {
        if self.shown.as_ref() != Some(&pr) {
            self.switching = Some(pr);
        }
    }

    /// An open of a PR being written is its writer saying the tour is checked, which ends the writing.
    pub fn request(&mut self, pr: PullRequestUrl, origin: OpenOrigin) -> OpenDecision {
        let was_written = self.writings.iter().any(|(writing, _)| writing == &pr);
        self.writings.retain(|(writing, _)| writing != &pr);
        if was_written {
            self.remember();
        }
        if self.is_reopening(&pr) {
            return OpenDecision::Build(self.build(pr));
        }
        if self.shown.as_ref() == Some(&pr) {
            self.issued += 1;
            self.building = None;
            self.switching = None;
            return OpenDecision::Reload;
        }
        let is_awaited = self.waiting_for.as_ref() == Some(&pr);
        let is_agents_own = was_written || origin == OpenOrigin::AppStartedAgent;
        if is_agents_own && !is_awaited {
            return OpenDecision::KeepInBackground;
        }
        self.waiting_for = None;
        OpenDecision::Build(self.build(pr))
    }

    pub fn finished<Built>(
        &mut self,
        ticket: BuildTicket,
        built: anyhow::Result<Built>,
    ) -> Finished<Built> {
        let Some(build) = self.building.take() else {
            return Finished::Superseded;
        };
        if build.ticket != ticket {
            self.building = Some(build);
            return Finished::Superseded;
        }
        self.switching = None;
        let Build {
            pr, held_replies, ..
        } = build;
        match built {
            Ok(built) => {
                self.unseen_replies.retain(|unseen| unseen != &pr);
                self.remember();
                self.shown = Some(pr);
                self.replies_for_reopened = held_replies;
                Finished::Switch(built)
            }
            Err(error) => Finished::Stay {
                reason: format!("could not open {pr}: {error:#}"),
            },
        }
    }

    /// Builds the PR on screen again, so what `open_review` reads anew, such as the config, applies.
    /// The window says it is opening, as for a switch, and `finished` takes the result.
    /// A build of another PR is left alone: it reads the config anyway, and the reviewer asked for it.
    pub fn reopen(&mut self) -> Option<(BuildTicket, PullRequestUrl)> {
        let pr = self.shown.clone()?;
        if self.building.is_some() && !self.is_reopening(&pr) {
            return None;
        }
        Some((self.build(pr.clone()), pr))
    }

    /// The window still applies the reply to the session it shows. The reopen's build may have read the
    /// state file before that session saved it, so the reply is kept to apply again in the new window.
    /// Applying a reply twice is harmless; it only sets the hand-off's state.
    pub fn hold_reply_for_reopen(
        &mut self,
        pr: &PullRequestUrl,
        hand_off: HandOffId,
        replied: &AgentReplied,
    ) {
        if !self.is_reopening(pr) {
            return;
        }
        if let Some(build) = &mut self.building {
            build.held_replies.push((hand_off, replied.clone()));
        }
    }

    /// The replies held while the PR on screen reopened, for its new window, after `finished` gave
    /// `Switch`. A reopen that failed or lost to a newer request drops them, as the old window has them.
    pub fn take_replies_for_reopened(&mut self) -> Vec<(HandOffId, AgentReplied)> {
        std::mem::take(&mut self.replies_for_reopened)
    }

    fn is_reopening(&self, pr: &PullRequestUrl) -> bool {
        self.shown.as_ref() == Some(pr)
            && self.building.as_ref().is_some_and(|build| &build.pr == pr)
    }

    /// A build of the PR that is reopening takes over the replies held for it.
    fn build(&mut self, pr: PullRequestUrl) -> BuildTicket {
        let held_replies = if self.is_reopening(&pr) {
            self.building
                .take()
                .map(|build| build.held_replies)
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        self.issued += 1;
        let ticket = BuildTicket(self.issued);
        self.switching = Some(pr.clone());
        self.building = Some(Build {
            ticket,
            pr,
            held_replies,
        });
        ticket
    }

    /// The reviewer picked a PR with no tour: the loading screen shows its writing, which starts unless
    /// one is on already. A build still running for another PR is superseded.
    pub fn write(&mut self, pr: PullRequestUrl) -> WriteDecision {
        self.building = None;
        self.switching = None;
        self.waiting_for = Some(pr.clone());
        if self.writing_of(&pr).is_some() {
            return WriteDecision::Watch;
        }
        self.writings.push((pr, TourWriting::starting()));
        self.remember();
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
            self.remember();
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
