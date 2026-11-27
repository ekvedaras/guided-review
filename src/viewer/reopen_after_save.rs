use std::time::{Duration, Instant};

use crate::keys::Command;

/// The work running now that a reopen would kill, as the window counts it: hand-offs are typed in by
/// tasks of the session.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct InFlight {
    pub delivering_hand_offs: bool,
}

impl InFlight {
    fn is_idle(self) -> bool {
        self == Self::default()
    }

    fn waiting_line(self) -> String {
        format!("Saved. The PR opens again once {}.", self.waits())
    }

    /// What has to end first, as one clause.
    fn waits(self) -> &'static str {
        if self.delivering_hand_offs {
            "the hand-offs are delivered to the tour agent"
        } else {
            ""
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
/// window's session, and with it any hand-off being typed in, so the reopen waits
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

    /// Called whenever the work in flight that the window counts changes.
    pub fn work_in_flight_changed(&mut self, in_flight: InFlight) -> Reopen {
        if self.waiting.is_none() {
            return Reopen::NotAsked;
        }
        self.saved(in_flight)
    }

    pub fn waiting_line(&self) -> Option<&str> {
        self.waiting.as_deref()
    }
}

/// How soon a second restart must follow the warning to count as a yes.
const CONFIRM_WITHIN: Duration = Duration::from_secs(10);

/// A way out of the app that ends what is in flight at once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Departure {
    Restart,
    Quit,
    CloseWindow,
}

/// Leaving the app ends it at once, so with work running the first ask warns, and only the same ask
/// again soon after goes through. Another way out does not confirm it.
#[derive(Debug, Default)]
pub struct RestartGuard {
    warned: Option<(Departure, Instant)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RestartStep {
    /// Nothing stops it.
    Restart,
    /// What would be lost, and that asking again goes ahead anyway.
    Warn(String),
}

impl RestartGuard {
    pub fn ask(&mut self, in_flight: InFlight, now: Instant) -> RestartStep {
        self.ask_to(Departure::Restart, in_flight, now)
    }

    pub fn ask_to(
        &mut self,
        departure: Departure,
        in_flight: InFlight,
        now: Instant,
    ) -> RestartStep {
        let was_warned = self.warned.is_some_and(|(warned_for, at)| {
            warned_for == departure && now.saturating_duration_since(at) <= CONFIRM_WITHIN
        });
        if in_flight.is_idle() || was_warned {
            self.warned = None;
            return RestartStep::Restart;
        }
        self.warned = Some((departure, now));
        let waits = in_flight.waits();
        RestartStep::Warn(match departure {
            Departure::Restart => format!(
                "A restart now does not wait until {waits}. Pick Restart again to restart anyway."
            ),
            Departure::Quit => format!(
                "Quitting now does not wait until {waits}. Press {} again to quit anyway.",
                default_key(Command::Quit)
            ),
            Departure::CloseWindow => format!(
                "Closing the window quits the app, which does not wait until {waits}. Press {} again to close anyway.",
                default_key(Command::CloseWindow)
            ),
        })
    }
}

fn default_key(command: Command) -> String {
    command
        .default_chords()
        .first()
        .map_or_else(String::new, |chord| chord.spelled())
}
