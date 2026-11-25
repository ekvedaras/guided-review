use super::app_state::{AppState, AppStateFile};
use super::guided_tour::GuidedTourProgress;
use super::hand_offs::{AgentReplied, HandOffId};
use super::tour_agent::OpenOrigin;
use super::tour_agent::{AgentStatus, TerminalId, TourAgentPort};
use super::tour_writing::TourWriting;
use crate::review_subject::ReviewSubject;
use crate::tour::ToursOnDisk;

/// Which PR the window shows, and what an incoming open request does to it. A PR that is built takes
/// the screen only when it is the newest request, so the one on screen stays until then. It also keeps
/// the tours being written, the one the loading screen waits for, and the PRs with an agent reply the
/// reviewer has not seen.
#[derive(Default)]
pub struct ActivePr {
    shown: Option<ReviewSubject>,
    building: Option<Build>,
    replies_for_reopened: Vec<(HandOffId, AgentReplied)>,
    issued: u64,
    /// The PR the reviewer picked or a build is making ready, which the window does not show yet.
    switching: Option<ReviewSubject>,
    writings: Vec<(ReviewSubject, TourWriting)>,
    /// The PR whose writing the loading screen shows.
    waiting_for: Option<ReviewSubject>,
    unseen_replies: Vec<ReviewSubject>,
    remembered_in: Option<AppStateFile>,
}

struct Build {
    ticket: BuildTicket,
    pr: ReviewSubject,
    /// Agent replies that came while the PR on screen reopened.
    held_replies: Vec<(HandOffId, AgentReplied)>,
    /// The build opens a tour the loading screen waited for, so a failure shows there.
    after_writing: bool,
}

/// Names one build, so its result can be told from the result of an older one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuildTicket(u64);

/// What an open request of a PR comes to.
pub enum OpenOutcome {
    /// The PR has a tour, or none and no writing: the build makes its basic tour when none is on disk.
    Tour(OpenDecision),
    /// A tour of the PR is being written and nothing is on disk to read yet: the loading screen shows it.
    Writing(WriteDecision),
}

pub enum OpenDecision {
    /// The PR is on screen: read its tour again.
    Reload,
    /// Build the PR's review in the background, then hand the result to `finished`.
    Build(BuildTicket),
    /// Not shown now: the switcher marks the PR ready instead.
    KeepInBackground,
    /// The basic tour on screen has its guided tour written and checked: the review says so and
    /// nothing is built, reloaded or moved.
    GuidedReady,
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
                .filter_map(|key| key.parse().ok())
                .map(|subject| (subject, TourWriting::starting()))
                .collect(),
            unseen_replies: saved
                .unseen_replies
                .iter()
                .filter_map(|key| key.parse().ok())
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
    ) -> Vec<(ReviewSubject, TerminalId)> {
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
        let keys =
            |subjects: Vec<&ReviewSubject>| subjects.into_iter().map(ToString::to_string).collect();
        let state = AppState {
            writings: keys(self.writings.iter().map(|(subject, _)| subject).collect()),
            unseen_replies: keys(self.unseen_replies.iter().collect()),
        };
        if let Err(error) = file.save(&state) {
            eprintln!("could not keep the app state: {error:#}");
        }
    }

    pub fn shown(&self) -> Option<&ReviewSubject> {
        self.shown.as_ref()
    }

    /// Dropping the review on screen goes through its open session, which then writes nothing more.
    pub fn is_shown(&self, subject: &ReviewSubject) -> bool {
        self.shown.as_ref() == Some(subject)
    }

    pub fn is_building(&self) -> bool {
        self.building.is_some()
    }

    /// The PR the window is about to show: picked, or being built.
    pub fn switching_to(&self) -> Option<&ReviewSubject> {
        self.switching.as_ref()
    }

    /// The reviewer picked a PR to switch to. This is known before the app's open request is answered,
    /// so the window can say so at once.
    pub fn picked(&mut self, pr: ReviewSubject) {
        if self.shown.as_ref() != Some(&pr) {
            self.switching = Some(pr);
        }
    }

    /// The one way in for an open request, whoever sent it: a PR with a tour, or with none and no writing,
    /// is requested, as its basic tour is made by the build; a PR whose tour is being written with nothing
    /// to read yet shows that writing.
    pub fn open(
        &mut self,
        pr: ReviewSubject,
        origin: OpenOrigin,
        on_disk: ToursOnDisk,
    ) -> OpenOutcome {
        if on_disk == ToursOnDisk::None && self.writing_of(&pr).is_some() {
            OpenOutcome::Writing(self.write(pr))
        } else if on_disk == ToursOnDisk::BasicWithGuided
            && self.shown.as_ref() == Some(&pr)
            && !self.is_reopening(&pr)
        {
            self.forget_writing(&pr);
            OpenOutcome::Tour(OpenDecision::GuidedReady)
        } else {
            OpenOutcome::Tour(self.request(pr, origin))
        }
    }

    /// The review's files are gone: it is shown no more, and nothing of it is remembered. True when it
    /// was the one on screen, whose window then shows something else.
    pub fn dropped(&mut self, subject: &ReviewSubject) -> bool {
        self.moved_away(subject);
        let was_shown = self.shown.as_ref() == Some(subject);
        if was_shown {
            self.shown = None;
        }
        was_shown
    }

    /// The review moved to another, whose files took its place: nothing of it is remembered, yet it
    /// stays on screen until the one it moved to opens, so the window never shows the switcher between.
    pub fn moved_away(&mut self, subject: &ReviewSubject) {
        self.forget_writing(subject);
        if self.has_unseen_reply(subject) {
            self.unseen_replies.retain(|unseen| unseen != subject);
            self.remember();
        }
    }

    /// True when a writing of the PR was on.
    fn forget_writing(&mut self, pr: &ReviewSubject) -> bool {
        let was_written = self.writings.iter().any(|(writing, _)| writing == pr);
        self.writings.retain(|(writing, _)| writing != pr);
        if was_written {
            self.remember();
        }
        was_written
    }

    /// What the review of the PR starts out knowing of its guided tour. A guided file on disk with no
    /// writing on means ready, so the banner outlives a restart, a re-pin and a tour written by `/tour`
    /// from the reviewer's own session; one mid-writing does not, as it is not checked yet.
    pub fn guided_tour_progress_of(
        &self,
        pr: &ReviewSubject,
        on_disk: ToursOnDisk,
    ) -> GuidedTourProgress {
        match (self.writing_of(pr), on_disk) {
            (Some(writing), _) => GuidedTourProgress::of_writing(writing),
            (None, ToursOnDisk::BasicWithGuided) => GuidedTourProgress::Ready,
            (None, _) => GuidedTourProgress::NotAsked,
        }
    }

    /// An open of a PR being written is its writer saying the tour is checked, which ends the writing.
    pub fn request(&mut self, pr: ReviewSubject, origin: OpenOrigin) -> OpenDecision {
        let was_written = self.forget_writing(&pr);
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
        let ticket = self.build(pr);
        if let Some(build) = &mut self.building {
            build.after_writing = is_awaited && was_written;
        }
        OpenDecision::Build(ticket)
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
            pr,
            held_replies,
            after_writing,
            ..
        } = build;
        match built {
            Ok(built) => {
                self.unseen_replies.retain(|unseen| unseen != &pr);
                self.remember();
                self.shown = Some(pr);
                self.replies_for_reopened = held_replies;
                Finished::Switch(built)
            }
            Err(error) => {
                let reason = format!("could not open {pr}: {error:#}");
                if after_writing {
                    self.writings.push((
                        pr.clone(),
                        TourWriting::failed_to_open(&format!("{error:#}")),
                    ));
                    self.waiting_for = Some(pr);
                    self.remember();
                }
                Finished::Stay { reason }
            }
        }
    }

    /// Builds the PR on screen again, so what `open_review` reads anew, such as the config, applies.
    /// The window says it is opening, as for a switch, and `finished` takes the result.
    /// A build of another PR is left alone: it reads the config anyway, and the reviewer asked for it.
    pub fn reopen(&mut self) -> Option<(BuildTicket, ReviewSubject)> {
        let pr = self.shown.clone()?;
        if self.building.is_some() && !self.is_reopening(&pr) {
            return None;
        }
        Some((self.build(pr.clone()), pr))
    }

    /// The reply is applied where it lands now: to the session on show, or to the state file by the
    /// command. A build of the PR may have read the state file before that, and its first save would
    /// write the stale hand-off back, so the reply is kept to apply again in the new window. Applying a
    /// reply twice is harmless; it only sets the hand-off's state.
    pub fn hold_reply_while_building(
        &mut self,
        pr: &ReviewSubject,
        hand_off: HandOffId,
        replied: &AgentReplied,
    ) {
        if !self.builds(pr) {
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

    fn is_reopening(&self, pr: &ReviewSubject) -> bool {
        self.shown.as_ref() == Some(pr) && self.builds(pr)
    }

    fn builds(&self, pr: &ReviewSubject) -> bool {
        self.building.as_ref().is_some_and(|build| &build.pr == pr)
    }

    /// A build of the PR that is being built already takes over the replies held for it.
    fn build(&mut self, pr: ReviewSubject) -> BuildTicket {
        let held_replies = if self.builds(&pr) {
            self.building
                .take()
                .map_or_default(|build| build.held_replies)
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
            after_writing: false,
        });
        ticket
    }

    /// The reviewer picked a PR with no tour: the loading screen shows its writing, which starts unless
    /// one is on already. A build still running for another PR is superseded.
    pub fn write(&mut self, pr: ReviewSubject) -> WriteDecision {
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

    /// The reviewer asked to write the guided tour of a basic review: the writing is recorded with no
    /// loading screen, and the review goes on reading. Refused while a writing of the PR is on.
    pub fn write_in_background(&mut self, pr: ReviewSubject) -> WriteDecision {
        if self
            .writing_of(&pr)
            .is_some_and(|writing| !writing.has_failed())
        {
            return WriteDecision::Watch;
        }
        self.writings.retain(|(writing, _)| writing != &pr);
        self.writings.push((pr, TourWriting::starting()));
        self.remember();
        WriteDecision::StartWriter
    }

    /// Starts the PR's writing over, as after a failure.
    pub fn try_again(&mut self, pr: &ReviewSubject) {
        self.writings.retain(|(writing, _)| writing != pr);
        self.write(pr.clone());
    }

    pub fn writer_started(&mut self, pr: &ReviewSubject, started: anyhow::Result<TerminalId>) {
        if let Some(writing) = self.writing_mut(pr) {
            writing.started(started);
        }
    }

    pub fn writer_status(&mut self, pr: &ReviewSubject, status: AgentStatus) {
        if let Some(writing) = self.writing_mut(pr) {
            writing.status_is(status);
        }
    }

    pub fn writer_screen(&mut self, pr: &ReviewSubject, lines: Vec<String>) {
        if let Some(writing) = self.writing_mut(pr) {
            writing.screen_is(lines);
        }
    }

    /// Herdr no longer follows the writer: its terminal ended.
    pub fn writer_gone(&mut self, pr: &ReviewSubject) {
        if let Some(writing) = self.writing_mut(pr) {
            writing.gone();
        }
    }

    /// The reviewer leaves the loading screen for the PR shown behind it; the writing goes on.
    pub fn stop_waiting(&mut self) {
        self.waiting_for = None;
    }

    /// The writing the loading screen shows.
    pub fn loading(&self) -> Option<(&ReviewSubject, &TourWriting)> {
        let pr = self.waiting_for.as_ref()?;
        Some((pr, self.writing_of(pr)?))
    }

    pub fn writing_of(&self, pr: &ReviewSubject) -> Option<&TourWriting> {
        self.writings
            .iter()
            .find(|(writing, _)| writing == pr)
            .map(|(_, writing)| writing)
    }

    /// What the switcher says of the PR's tour beyond what the disk says, while it is being written.
    pub fn writing_fact_of(&self, pr: &ReviewSubject) -> WritingFact {
        match self.writing_of(pr) {
            None => WritingFact::None,
            Some(writing) if writing.has_failed() => WritingFact::Failed,
            Some(_) => WritingFact::Writing,
        }
    }

    /// The tour agent answered a hand-off of the PR. Unless the PR is on screen, the switcher marks it.
    pub fn agent_replied(&mut self, pr: &ReviewSubject) {
        if self.shown.as_ref() != Some(pr) && !self.has_unseen_reply(pr) {
            self.unseen_replies.push(pr.clone());
            self.remember();
        }
    }

    pub fn has_unseen_reply(&self, pr: &ReviewSubject) -> bool {
        self.unseen_replies.contains(pr)
    }

    fn writing_mut(&mut self, pr: &ReviewSubject) -> Option<&mut TourWriting> {
        self.writings
            .iter_mut()
            .find(|(writing, _)| writing == pr)
            .map(|(_, writing)| writing)
    }
}

/// A tour agent the app started writing a guided tour of the PR.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WritingFact {
    None,
    Writing,
    /// The writing ended without a tour.
    Failed,
}
