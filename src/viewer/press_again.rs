//! A key that changes the PR on GitHub acts only on the same press again soon after: the first press
//! says what the second one does, so a stray key cannot merge or un-draft a PR.

use std::time::{Duration, Instant};

use super::shortcuts::Command;

/// How soon the second press must follow the first to go ahead.
pub const CONFIRM_WITHIN: Duration = Duration::from_secs(10);

/// What a press of such a key did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PressStep {
    /// The PR's status is not known, so there is nothing to change.
    Nothing,
    /// GitHub would refuse; why, for a toast.
    Refused(String),
    /// The first press: what the same press again does.
    Ask(String),
    /// The second press: the change is on its way to GitHub.
    Sent,
}

impl PressStep {
    /// The toast of a first press of `command`.
    pub(super) fn ask(command: Command, does: &str) -> Self {
        let key = command
            .default_chords()
            .first()
            .map_or_else(String::new, |chord| chord.spelled());
        Self::Ask(format!(
            "Press {key} again within {} s to {does}.",
            CONFIRM_WITHIN.as_secs()
        ))
    }
}

/// The first press of a key and what it asked for, waiting for the same press again.
#[derive(Debug)]
pub struct PressAgain<T> {
    asked: Option<(T, Instant)>,
}

impl<T> Default for PressAgain<T> {
    fn default() -> Self {
        Self { asked: None }
    }
}

impl<T: Copy + PartialEq> PressAgain<T> {
    /// Whether this press confirms the first one: the same change, soon enough. When it does not,
    /// it becomes the first press.
    pub fn confirms(&mut self, wanted: T, now: Instant) -> bool {
        let confirmed = self.asked.is_some_and(|(asked, at)| {
            asked == wanted && now.saturating_duration_since(at) <= CONFIRM_WITHIN
        });
        self.asked = (!confirmed).then_some((wanted, now));
        confirmed
    }
}
