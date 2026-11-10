/// The work running now that a reopen would kill, as the window counts it: Ask turns answer in threads
/// the Ask panel owns, and hand-offs and Asks handed to the agent are typed in by tasks of the session.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct InFlight {
    pub asks_answering: u16,
    pub asks_handed_to_agent: u16,
    pub delivering_hand_offs: bool,
}

impl InFlight {
    fn is_idle(self) -> bool {
        self == Self::default()
    }

    fn waiting_line(self) -> String {
        let mut waits = Vec::new();
        match self.asks_answering {
            0 => {}
            1 => waits.push("the Ask finishes answering".to_owned()),
            count => waits.push(format!("{count} Asks finish answering")),
        }
        match self.asks_handed_to_agent {
            0 => {}
            1 => waits.push("the Ask is handed to your agent".to_owned()),
            count => waits.push(format!("{count} Asks are handed to your agent")),
        }
        if self.delivering_hand_offs {
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
}

impl ReopenAfterSave {
    pub fn saved(&mut self, in_flight: InFlight) -> Reopen {
        if in_flight.is_idle() {
            self.waiting = None;
            return Reopen::Now;
        }
        self.waiting = Some(in_flight.waiting_line());
        Reopen::Waiting
    }

    /// Called whenever the work in flight changes.
    pub fn in_flight_is(&mut self, in_flight: InFlight) -> Reopen {
        if self.waiting.is_none() {
            return Reopen::NotAsked;
        }
        self.saved(in_flight)
    }

    pub fn waiting_line(&self) -> Option<&str> {
        self.waiting.as_deref()
    }
}
