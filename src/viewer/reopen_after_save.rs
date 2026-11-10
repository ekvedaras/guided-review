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
        let waits = match waits.split_last() {
            Some((last, [])) => last.clone(),
            Some((last, rest)) => format!("{} and {last}", rest.join(", ")),
            None => String::new(),
        };
        format!("Saved. The PR opens again once {waits}.")
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
