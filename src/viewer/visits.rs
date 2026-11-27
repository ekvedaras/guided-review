//! The places the reviewer was at, kept like a browser's history so Back and Forward return to them.

use super::line_location::LineLocation;
use super::screen::Screen;

/// Where the reviewer was scrolled on a screen: the code at the top of the view, which survives rows
/// that came or went since, and the row it was at, which is the best guess when that code is gone.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReadingSpot {
    anchor: Option<LineLocation>,
    row_hint: usize,
}

impl ReadingSpot {
    pub fn new(anchor: Option<LineLocation>, row_hint: usize) -> Self {
        Self { anchor, row_hint }
    }

    pub fn anchor(&self) -> Option<&LineLocation> {
        self.anchor.as_ref()
    }

    pub fn row_hint(&self) -> usize {
        self.row_hint
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Visit {
    pub screen: Screen,
    pub spot: ReadingSpot,
}

/// The visits Back returns to, and those Forward returns to after a Back.
#[derive(Debug, Default)]
pub(super) struct Visits {
    back: Vec<Visit>,
    forward: Vec<Visit>,
}

impl Visits {
    /// The reviewer moved on from `left`. Like a browser, a new move drops what Forward held.
    pub fn left(&mut self, left: Visit) {
        self.back.push(left);
        self.forward.clear();
    }

    pub fn has_back(&self) -> bool {
        !self.back.is_empty()
    }

    pub fn has_forward(&self) -> bool {
        !self.forward.is_empty()
    }

    /// The visit Back returns to, the latest whose screen `is_shown`; `here` becomes what Forward returns to.
    pub fn back(&mut self, here: &Visit, is_shown: impl Fn(&Screen) -> bool) -> Option<Visit> {
        let target = take_shown(&mut self.back, is_shown)?;
        self.forward.push(here.clone());
        Some(target)
    }

    pub fn forward(&mut self, here: &Visit, is_shown: impl Fn(&Screen) -> bool) -> Option<Visit> {
        let target = take_shown(&mut self.forward, is_shown)?;
        self.back.push(here.clone());
        Some(target)
    }
}

/// A re-pinned tour may no longer have a screen the history remembers; those are passed over.
fn take_shown(stack: &mut Vec<Visit>, is_shown: impl Fn(&Screen) -> bool) -> Option<Visit> {
    while let Some(visit) = stack.pop() {
        if is_shown(&visit.screen) {
            return Some(visit);
        }
    }
    None
}
