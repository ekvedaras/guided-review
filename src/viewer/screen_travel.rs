use super::review::{Review, Screen};

/// Which way along the tour the reader went to reach the screen on show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Travel {
    Forward,
    Back,
}

/// Counts the screens the reader arrived at, so a screen's entrance plays once per arrival.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Arrival(u32);

impl Arrival {
    pub fn count(self) -> u32 {
        self.0
    }
}

/// Notes each change of screen and which way it went, so the new screen can come in from that side.
#[derive(Debug, Default)]
pub struct ScreenTravel {
    shown: Option<Screen>,
    arrival: Arrival,
    heading: Option<Travel>,
}

impl ScreenTravel {
    /// Called whenever the review may have changed; only a new screen counts as an arrival.
    pub fn arrive(&mut self, review: &Review) {
        let screen = review.screen();
        if self.shown.as_ref() == Some(screen) {
            return;
        }
        let from = self
            .shown
            .as_ref()
            .and_then(|shown| review.reading_position(shown));
        let to = review.reading_position(screen);
        self.heading = Some(match (from, to) {
            (Some(from), Some(to)) if to < from => Travel::Back,
            _ => Travel::Forward,
        });
        self.arrival = Arrival(self.arrival.0.wrapping_add(1));
        self.shown = Some(screen.clone());
    }

    pub fn arrival(&self) -> Arrival {
        self.arrival
    }

    pub fn heading(&self) -> Travel {
        self.heading.unwrap_or(Travel::Forward)
    }
}
