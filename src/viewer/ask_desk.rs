use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use crate::pull_request::Sha;

use super::agent_event::AgentEvent;
use super::agent_pool::{AgentPool, Opening};
use super::ask::{Ask, AskId, Asks, Place};
use super::ask_target::AskTarget;
use super::ports::{AgentLaunch, TurnStopper};
use super::review::Screen;

/// The live AI processes of Asks, each answering on its own thread. The desk decides which run;
/// the shell starts, feeds and stops them, and reports what each says as an `AskProcessEvent`.
pub trait AskProcesses {
    fn start(&mut self, ask: AskId, launch: AgentLaunch);
    fn stop(&mut self, ask: AskId);
    fn is_running(&self, ask: AskId) -> bool;
    /// Fails when the Ask has no process, or its process ended before it took the message.
    fn deliver(&mut self, ask: AskId, message: String) -> Result<(), NotDelivered>;
    /// `None` until the process runs.
    fn turn_stopper(&self, ask: AskId) -> Option<Arc<dyn TurnStopper>>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NotDelivered;

/// What an Ask's process says, as the shell hears it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AskProcessEvent {
    Line(String),
    TurnEnded,
    /// The process failed to start or died; it is gone.
    Failed(String),
}

/// Why the last question did not go out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AskNotice {
    AllAnswering,
}

impl AskNotice {
    pub fn message(self) -> &'static str {
        match self {
            Self::AllAnswering => "Three Asks are answering now: wait for one, or stop it",
        }
    }
}

/// What became of a typed question.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Question {
    /// It is in the Ask now, so the box empties.
    Taken,
    /// Nothing to send, a turn still answers, or every process answers: the box keeps it.
    Kept,
}

pub const QUESTION_NOT_SENT: &str = "The AI stopped before it got the question, and three other Asks are answering: ask again when one is done";

const HANDING_TO_AGENT: &str = "Handing to your agent...";

/// Every Ask of the PR and the AI processes answering them: which place the Ask panel is open on,
/// which Ask a question goes to, which processes run, what each process's news does to its Ask,
/// and what became of an Ask handed to the reviewer's own agent.
pub struct AskDesk {
    asks: Asks,
    pool: AgentPool,
    /// Without a local clone the AI has no code to read, so Ask is off.
    clone: Option<PathBuf>,
    /// What the Ask panel is open on; `None` while it is closed.
    target: Option<AskTarget>,
    /// The Ask the next question goes to. Closing the panel keeps it, so opening the same place
    /// again goes on with its draft.
    selected: Option<AskId>,
    /// Asks a re-pin changed the code under, as of the review they were last checked against.
    code_changed: HashSet<AskId>,
    notice: Option<AskNotice>,
    /// What became of each Ask handed to the tour agent, or that it is being handed.
    hand_offs: HashMap<AskId, Arc<str>>,
}

impl AskDesk {
    pub fn new(asks: Asks, clone: Option<PathBuf>) -> Self {
        Self {
            asks,
            pool: AgentPool::default(),
            clone,
            target: None,
            selected: None,
            code_changed: HashSet::new(),
            notice: None,
            hand_offs: HashMap::new(),
        }
    }

    pub fn clone_folder(&self) -> Option<&PathBuf> {
        self.clone.as_ref()
    }

    pub fn all(&self) -> &[Ask] {
        self.asks.all()
    }

    pub fn get(&self, id: AskId) -> Option<&Ask> {
        self.asks.get(id)
    }

    /// Asks about this place, newest first. Empty drafts are left out.
    pub fn at(&self, place: &Place) -> Vec<&Ask> {
        self.asks.at(place)
    }

    /// What the marker on a place shows.
    pub fn count_at(&self, place: &Place) -> usize {
        self.asks.count_at(place)
    }

    /// The selected lines with Asks on a screen, which get a marker under their last row.
    pub fn asked_lines_on(&self, screen: &Screen) -> Vec<Place> {
        let step = match screen {
            Screen::Step(id) => Some(id),
            Screen::SafetyNet => None,
            Screen::Summary => return Vec::new(),
        };
        let mut places: Vec<Place> = Vec::new();
        for ask in self.asks.all() {
            let Place::Lines { step: of, .. } = &ask.place else {
                continue;
            };
            if of.as_ref() == step && !ask.messages.is_empty() && !places.contains(&ask.place) {
                places.push(ask.place.clone());
            }
        }
        places
    }

    /// What the Ask panel is open on; `None` while it is closed.
    pub fn target(&self) -> Option<&AskTarget> {
        self.target.as_ref()
    }

    pub fn is_open(&self) -> bool {
        self.target.is_some()
    }

    pub fn selected(&self) -> Option<AskId> {
        self.selected
    }

    pub fn is_answering(&self, id: AskId) -> bool {
        self.pool.is_answering(id)
    }

    pub fn answering(&self) -> u16 {
        self.pool.answering()
    }

    pub fn code_changed_since(&self, id: AskId) -> bool {
        self.code_changed.contains(&id)
    }

    pub fn notice(&self) -> Option<AskNotice> {
        self.notice
    }

    pub fn hand_off(&self, id: AskId) -> Option<&Arc<str>> {
        self.hand_offs.get(&id)
    }

    pub(super) fn code_changed_is(&mut self, changed: HashSet<AskId>) {
        self.code_changed = changed;
    }

    /// Shows the Asks of a place and gets the AI ready: its process starts now, not at the first
    /// question. An empty draft about the same place is asked again instead of a new one.
    pub(super) fn open(
        &mut self,
        target: AskTarget,
        head: &Sha,
        now: Instant,
        processes: &mut dyn AskProcesses,
    ) {
        self.notice = None;
        if let AskTarget::Unavailable { .. } = target {
            self.selected = None;
            self.target = Some(target);
            return;
        }
        let reused_draft = self.selected.filter(|id| {
            self.asks
                .get(*id)
                .is_some_and(|ask| &ask.place == target.place() && ask.messages.is_empty())
        });
        let selected =
            reused_draft.unwrap_or_else(|| self.asks.start(target.place().clone(), head));
        self.selected = Some(selected);
        self.target = Some(target);
        self.ensure_process(selected, now, processes);
    }

    pub fn close(&mut self) {
        self.target = None;
    }

    /// The next question goes to this Ask, which gets its process ready.
    pub fn follow_up(&mut self, id: AskId, now: Instant, processes: &mut dyn AskProcesses) {
        self.selected = Some(id);
        self.notice = None;
        self.ensure_process(id, now, processes);
    }

    /// Sends the question to the selected Ask's process. A process that died before it took the
    /// question is started once more; when that cannot be, the Ask says so.
    pub fn send(
        &mut self,
        question: &str,
        now: Instant,
        processes: &mut dyn AskProcesses,
    ) -> Question {
        let question = question.trim();
        let (Some(id), Some(AskTarget::Ready { context, .. })) = (self.selected, &self.target)
        else {
            return Question::Kept;
        };
        let context = context.clone();
        if question.is_empty() || self.pool.is_answering(id) {
            return Question::Kept;
        }
        if !self.ensure_process(id, now, processes) {
            self.notice = Some(AskNotice::AllAnswering);
            return Question::Kept;
        }
        self.notice = None;
        let message = self.asks.begin_turn(id, question, &context);
        if processes.deliver(id, message.clone()).is_err() {
            processes.stop(id);
            self.pool.ended(id);
            let resent =
                self.ensure_process(id, now, processes) && processes.deliver(id, message).is_ok();
            if !resent {
                self.asks
                    .apply(id, AgentEvent::Error(QUESTION_NOT_SENT.to_owned()));
                return Question::Taken;
            }
        }
        self.pool.turn_started(id, now);
        Question::Taken
    }

    /// The answer so far stays, marked as stopped; the process keeps the session.
    pub fn stop_turn(&mut self, id: AskId, processes: &dyn AskProcesses) {
        if let Some(stopper) = processes.turn_stopper(id) {
            self.asks.stop_turn(id, stopper.as_ref());
        }
    }

    pub fn on_process_event(
        &mut self,
        id: AskId,
        event: AskProcessEvent,
        now: Instant,
        processes: &mut dyn AskProcesses,
    ) {
        match event {
            AskProcessEvent::Line(line) => self.asks.apply_line(id, &line),
            AskProcessEvent::TurnEnded => self.pool.turn_ended(id, now),
            AskProcessEvent::Failed(reason) => {
                self.pool.ended(id);
                processes.stop(id);
                self.asks.apply(id, AgentEvent::Error(reason));
            }
        }
    }

    /// Stops the processes nobody used for the idle limit.
    pub fn stop_idle(&mut self, now: Instant, processes: &mut dyn AskProcesses) {
        for id in self.pool.stop_idle(now) {
            processes.stop(id);
        }
    }

    pub fn hand_off_started(&mut self, id: AskId) {
        self.hand_offs.insert(id, HANDING_TO_AGENT.into());
    }

    /// The word on an Ask handed, or tried to hand, to the tour agent.
    pub fn hand_off_ended(&mut self, id: AskId, line: &str) {
        self.hand_offs.insert(id, line.into());
    }

    /// Gets the Ask a live process, stopping the one used longest ago when three are live.
    /// False when the place cannot be asked about, or all three are answering.
    fn ensure_process(
        &mut self,
        id: AskId,
        now: Instant,
        processes: &mut dyn AskProcesses,
    ) -> bool {
        let Some(AskTarget::Ready { launch, .. }) = &self.target else {
            return false;
        };
        let launch = self.asks.launch_for(id, launch.clone());
        match self.pool.open(id, now) {
            Opening::Live if processes.is_running(id) => return true,
            Opening::Live => {}
            Opening::Start { stop } => {
                if let Some(stopped) = stop {
                    processes.stop(stopped);
                }
            }
            Opening::AllAnswering => return false,
        }
        processes.start(id, launch);
        true
    }
}
