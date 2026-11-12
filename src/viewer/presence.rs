//! Whether an overlay is on screen, and whether it is on its way out. Every overlay (the palette,
//! the PR switcher, Settings, the shortcuts sheet, the Ask panel) keeps one, so they close the same
//! way: the overlay stays drawn while it leaves, takes no input meanwhile, and is dropped once its
//! leaving move has played.
//!
//! The move is one timeline from 0 (gone) to 1 (fully in). Opening travels it forward and leaving
//! backward, so a leave is the entrance played in reverse; switching direction midway continues
//! from the place the timeline had reached and takes only the time that is left.

use std::time::{Duration, Instant};

/// Whether a leaving move may play. The app says `Reduced` when the reader asked for no motion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Motion {
    Full,
    Reduced,
}

/// What closing an overlay asks of whoever draws it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Closing {
    /// Nothing to play: the overlay is gone, or was not open.
    AtOnce,
    /// The overlay stays drawn for `lasts`, then `end_leaving` with this ticket drops it.
    Leaving { ticket: Ticket, lasts: Duration },
}

/// Names one leave, so a leave that was reversed by a reopening cannot end the newer opening.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ticket(usize);

/// A stretch of the timeline to play: from where it is to where it goes, in the time that is left.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stretch {
    pub from: f32,
    pub to: f32,
    pub lasts: Duration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    Closed,
    Open,
    Leaving,
}

/// What an overlay is drawn as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shown {
    Entering,
    Leaving,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Presence {
    stage: Stage,
    /// Counts the openings and leaves, so each plays its move from the start under its own id.
    turns: usize,
    /// How long the whole timeline takes.
    total: Duration,
    travel: Stretch,
    /// When `travel` began; `None` before the first opening.
    started: Option<Instant>,
}

impl Presence {
    pub fn lasting(total: Duration) -> Self {
        Self {
            stage: Stage::Closed,
            turns: 0,
            total,
            travel: Stretch {
                from: 0.,
                to: 0.,
                lasts: Duration::ZERO,
            },
            started: None,
        }
    }

    /// Where the timeline is at `now`: 0 gone, 1 fully in.
    pub fn position(&self, now: Instant) -> f32 {
        let Some(started) = self.started else {
            return self.travel.to;
        };
        let lasts = self.travel.lasts.as_secs_f32();
        let done = if lasts <= 0. {
            1.
        } else {
            (now.saturating_duration_since(started).as_secs_f32() / lasts).min(1.)
        };
        self.travel.from + (self.travel.to - self.travel.from) * done
    }

    fn go_to(&mut self, to: f32, now: Instant) {
        let from = self.position(now);
        self.travel = Stretch {
            from,
            to,
            lasts: self.total.mul_f32((to - from).abs()),
        };
        self.started = Some(now);
        self.turns += 1;
    }

    /// Opens it, or reverses a leave under way from where it had got to. Opening what is open
    /// changes nothing.
    pub fn open(&mut self, now: Instant) {
        if self.stage != Stage::Open {
            self.go_to(1., now);
            self.stage = Stage::Open;
        }
    }

    pub fn close(&mut self, motion: Motion, now: Instant) -> Closing {
        if self.stage != Stage::Open {
            return Closing::AtOnce;
        }
        match motion {
            Motion::Reduced => {
                self.stage = Stage::Closed;
                self.travel = Stretch {
                    from: 0.,
                    to: 0.,
                    lasts: Duration::ZERO,
                };
                Closing::AtOnce
            }
            Motion::Full => {
                self.go_to(0., now);
                self.stage = Stage::Leaving;
                Closing::Leaving {
                    ticket: Ticket(self.turns),
                    lasts: self.travel.lasts,
                }
            }
        }
    }

    /// Drops the overlay when the leave `ticket` names is still the one under way.
    pub fn end_leaving(&mut self, ticket: Ticket) {
        if self.stage == Stage::Leaving && ticket.0 == self.turns {
            self.stage = Stage::Closed;
        }
    }

    /// Open and taking input: false while it leaves.
    pub fn is_open(&self) -> bool {
        self.stage == Stage::Open
    }

    /// Drawn at all: open, or leaving.
    pub fn shown(&self) -> Option<Shown> {
        match self.stage {
            Stage::Closed => None,
            Stage::Open => Some(Shown::Entering),
            Stage::Leaving => Some(Shown::Leaving),
        }
    }

    /// The stretch of timeline to play now.
    pub fn travel(&self) -> Stretch {
        self.travel
    }

    /// Changes with every opening and every leave: the part of a moving element's id that makes
    /// its move play again.
    pub fn turn(&self) -> usize {
        self.turns
    }
}
