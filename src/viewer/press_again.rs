//! A key that changes the PR on GitHub acts only on the same press again soon after: the first press
//! says what the second one does, so a stray key cannot merge or un-draft a PR.

use std::time::{Duration, Instant};

use crate::keys::Command;

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

/// One change a key asks of the PR: the first press and what it asked for, waiting for the same
/// press again, then the confirmed change until GitHub answers.
#[derive(Debug)]
pub(super) struct PressAgain<T> {
    asked: Option<(T, Instant)>,
    to_send: Option<T>,
}

impl<T> Default for PressAgain<T> {
    fn default() -> Self {
        Self {
            asked: None,
            to_send: None,
        }
    }
}

impl<T: Copy + PartialEq> PressAgain<T> {
    /// A press of `command`, which offers the change GitHub would take or says why it would not.
    /// It sends the change when it is the same press as the first, soon enough; otherwise it
    /// becomes the first press and says what the same press again `does`.
    pub(super) fn press(
        &mut self,
        command: Command,
        offered: Result<T, String>,
        now: Instant,
        does: impl FnOnce(T) -> String,
    ) -> PressStep {
        let wanted = match offered {
            Ok(wanted) => wanted,
            Err(refusal) => return PressStep::Refused(refusal),
        };
        let confirmed = self.asked.is_some_and(|(asked, at)| {
            asked == wanted && now.saturating_duration_since(at) <= CONFIRM_WITHIN
        });
        self.asked = (!confirmed).then_some((wanted, now));
        if confirmed {
            self.to_send = Some(wanted);
            return PressStep::Sent;
        }
        PressStep::ask(command, &does(wanted))
    }

    /// What the confirmed press wants GitHub to be told.
    pub(super) fn to_send(&self) -> Option<T> {
        self.to_send
    }

    /// A snapshot scene shows the change as sent, without sending it.
    #[cfg(feature = "snapshots")]
    pub(super) fn stage_to_send(&mut self, change: Option<T>) {
        self.to_send = change;
    }

    /// GitHub answered `change`, so it is no longer to send.
    pub(super) fn landed(&mut self, change: T) {
        if self.to_send == Some(change) {
            self.to_send = None;
        }
    }
}
