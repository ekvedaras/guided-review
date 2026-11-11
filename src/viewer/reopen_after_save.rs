use std::time::{Duration, Instant};

/// The work running now that a reopen would kill, as the window counts it: Ask turns answer in threads
/// the Ask panel owns, and hand-offs and Asks handed to the agent are typed in by tasks of the session.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct InFlight {
    pub asks_answering: u16,
    pub delivering_hand_offs: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Pending {
    in_flight: InFlight,
    asks_handed_to_agent: u16,
}

impl Pending {
    fn is_idle(self) -> bool {
        self.in_flight == InFlight::default() && self.asks_handed_to_agent == 0
    }

    fn waiting_line(self) -> String {
        format!("Saved. The PR opens again once {}.", self.waits())
    }

    /// What has to end first, as one clause: "the Ask finishes answering and …".
    fn waits(self) -> String {
        let mut waits = Vec::new();
        match self.in_flight.asks_answering {
            0 => {}
            1 => waits.push("the Ask finishes answering".to_owned()),
            count => waits.push(format!("{count} Asks finish answering")),
        }
        match self.asks_handed_to_agent {
            0 => {}
            1 => waits.push("the Ask is handed to your agent".to_owned()),
            count => waits.push(format!("{count} Asks are handed to your agent")),
        }
        if self.in_flight.delivering_hand_offs {
            waits.push("the hand-offs are delivered to the tour agent".to_owned());
        }
        match waits.split_last() {
            Some((last, [])) => last.clone(),
            Some((last, rest)) => format!("{} and {last}", rest.join(", ")),
            None => String::new(),
        }
    }
}

/// What to do about the reopen a save asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reopen {
    /// Reopen the PR on screen now.
    Now,
    /// A reopen waits for work in flight; `waiting_line` says which.
    Waiting,
    /// No save asked for a reopen.
    NotAsked,
}

/// The reopen that a save of the settings asks for, so the new config applies. Reopening drops the
/// window's session, and with it any Ask answering or hand-off being typed in, so the reopen waits
/// until that work ends. However often the reviewer saves meanwhile, it is one reopen.
#[derive(Debug, Default)]
pub struct ReopenAfterSave {
    /// The line for the Settings panel while the reopen waits; kept so a render only borrows it.
    waiting: Option<String>,
    asks_handed_to_agent: u16,
}

impl ReopenAfterSave {
    pub fn saved(&mut self, in_flight: InFlight) -> Reopen {
        let pending = Pending {
            in_flight,
            asks_handed_to_agent: self.asks_handed_to_agent,
        };
        if pending.is_idle() {
            self.waiting = None;
            return Reopen::Now;
        }
        self.waiting = Some(pending.waiting_line());
        Reopen::Waiting
    }

    /// Called whenever the work in flight that the window counts changes.
    pub fn work_in_flight_changed(&mut self, in_flight: InFlight) -> Reopen {
        if self.waiting.is_none() {
            return Reopen::NotAsked;
        }
        self.saved(in_flight)
    }

    /// An Ask is being typed into the reviewer's own agent.
    pub fn ask_handed_to_agent(&mut self) {
        self.asks_handed_to_agent += 1;
    }

    pub fn ask_hand_off_ended(&mut self, in_flight: InFlight) -> Reopen {
        self.asks_handed_to_agent = self.asks_handed_to_agent.saturating_sub(1);
        self.work_in_flight_changed(in_flight)
    }

    pub fn waiting_line(&self) -> Option<&str> {
        self.waiting.as_deref()
    }
}

/// How soon a second restart must follow the warning to count as a yes.
const CONFIRM_WITHIN: Duration = Duration::from_secs(10);

/// A restart quits the app at once, so with work running it first warns, and only a second ask soon
/// after restarts.
#[derive(Debug, Default)]
pub struct RestartGuard {
    warned_at: Option<Instant>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RestartStep {
    Restart,
    /// What would be lost, and that asking again restarts anyway.
    Warn(String),
}

impl RestartGuard {
    pub fn ask(&mut self, in_flight: InFlight, now: Instant) -> RestartStep {
        let was_warned = self
            .warned_at
            .is_some_and(|warned| now.saturating_duration_since(warned) <= CONFIRM_WITHIN);
        if in_flight == InFlight::default() || was_warned {
            self.warned_at = None;
            return RestartStep::Restart;
        }
        self.warned_at = Some(now);
        let running = Pending {
            in_flight,
            asks_handed_to_agent: 0,
        };
        RestartStep::Warn(format!(
            "A restart now does not wait until {}. Pick Restart again to restart anyway.",
            running.waits()
        ))
    }
}
