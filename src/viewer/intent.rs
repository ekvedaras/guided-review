//! What the reader asks of the review's window, by key, button or palette, and what that does.
//!
//! The window reports what it alone knows (an overlay open, a reopen under way, the end of the
//! screen on show) and carries out the `Effect`; the rules that pick the effect are here, once.

use super::review::{ReviewCore, Screen};
use crate::tour::StepId;

/// One thing the reader asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Intent {
    /// The forward key: on, ticking the step it leaves only once its end was seen.
    NextScreen,
    /// The footer's main button: ticks the step it leaves, then goes on.
    MarkReadAndGoOn,
}

/// What the window does for an intent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    Change(ReviewChange),
}

/// A change of the review an intent makes, applied with `ReviewCore::apply`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReviewChange {
    GoOn { ticking: Option<StepId> },
}

/// What only the window knows when an intent comes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WindowMoment {}

impl ReviewCore {
    pub fn decide(&self, intent: Intent, _moment: WindowMoment) -> Effect {
        match intent {
            Intent::NextScreen => Effect::Change(ReviewChange::GoOn {
                ticking: self
                    .unread_step_on_screen()
                    .filter(|_| self.has_seen_end_of_screen()),
            }),
            Intent::MarkReadAndGoOn => Effect::Change(ReviewChange::GoOn {
                ticking: self.unread_step_on_screen(),
            }),
        }
    }

    /// A tick that cannot be saved is one of the review's problems; the change goes on regardless.
    pub fn apply(&mut self, change: ReviewChange) -> anyhow::Result<()> {
        match change {
            ReviewChange::GoOn { ticking } => {
                let ticked = ticking.map_or(Ok(()), |id| self.tick(&id));
                self.go_on();
                ticked
            }
        }
    }

    fn unread_step_on_screen(&self) -> Option<StepId> {
        match self.screen() {
            Screen::Step(id) if !self.is_ticked(id) => Some(id.clone()),
            Screen::Summary | Screen::Step(_) | Screen::SafetyNet => None,
        }
    }
}
