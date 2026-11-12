//! Whether an overlay is on screen, and whether it is on its way out. Every overlay (the palette,
//! the PR switcher, Settings, the shortcuts sheet, the Ask panel) keeps one, so they close the same
//! way: the overlay stays drawn while it leaves, takes no input meanwhile, and is dropped once its
//! leaving move has played.

/// Whether a leaving move may play. The app says `Reduced` when the reader asked for no motion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Motion {
    Full,
    Reduced,
}

/// What closing an overlay asks of whoever draws it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Closing {
    /// Nothing to play: the overlay is gone, or was not open.
    AtOnce,
    /// The overlay stays drawn for `LEAVE`, then `end_leaving` with this ticket drops it.
    Leaving(Ticket),
}

/// Names one leave, so a leave that was reversed by a reopening cannot end the newer opening.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ticket(usize);

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Presence {
    stage: Stage,
    /// Counts the openings and leaves, so each plays its move from the start under its own id.
    turns: usize,
}

impl Default for Presence {
    fn default() -> Self {
        Self {
            stage: Stage::Closed,
            turns: 0,
        }
    }
}

impl Presence {
    /// Opens it, or reverses a leave under way. Opening what is open changes nothing.
    pub fn open(&mut self) {
        if self.stage != Stage::Open {
            self.stage = Stage::Open;
            self.turns += 1;
        }
    }

    pub fn close(&mut self, motion: Motion) -> Closing {
        if self.stage != Stage::Open {
            return Closing::AtOnce;
        }
        match motion {
            Motion::Reduced => {
                self.stage = Stage::Closed;
                Closing::AtOnce
            }
            Motion::Full => {
                self.stage = Stage::Leaving;
                self.turns += 1;
                Closing::Leaving(Ticket(self.turns))
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

    /// Changes with every opening and every leave: the part of a moving element's id that makes
    /// its move play again.
    pub fn turn(&self) -> usize {
        self.turns
    }
}
