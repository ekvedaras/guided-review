//! When the Definition card of a hovered name is open: after the pointer rests on a name, while
//! it is on the name or the card, and not after Escape or a press. Timers are the app's: each
//! change that a timer may undo hands out a ticket, and a ticket that is not the latest does nothing.

use super::name_desk::{HoveredName, NameUnderPointer};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HoverTicket(u32);

#[derive(Debug, Default)]
pub struct NameHover {
    pointer: Option<NameUnderPointer>,
    open: Option<HoveredName>,
    card_hovered: bool,
    latest: u32,
}

impl NameHover {
    pub fn shown(&self) -> Option<&HoveredName> {
        self.open.as_ref()
    }

    /// A ticket for the rest to wait out, unless the pointer is on this name already.
    pub fn pointer_on(&mut self, under: NameUnderPointer) -> Option<HoverTicket> {
        if self.pointer.as_ref() == Some(&under) {
            return None;
        }
        let ticket = self.next_ticket();
        let already_open = self.open.as_ref() == Some(&under.name);
        if !already_open && !self.card_hovered {
            self.open = None;
        }
        self.pointer = Some(under);
        (!already_open).then_some(ticket)
    }

    /// A ticket for the grace before the card closes, if one is open.
    pub fn pointer_off(&mut self) -> Option<HoverTicket> {
        self.pointer.take()?;
        self.close_later()
    }

    pub fn card_entered(&mut self) {
        self.next_ticket();
        self.card_hovered = true;
    }

    pub fn card_left(&mut self) -> Option<HoverTicket> {
        self.card_hovered = false;
        self.close_later()
    }

    /// The name to look up, when the pointer has rested on it.
    pub fn rested(&mut self, ticket: HoverTicket) -> Option<HoveredName> {
        if ticket.0 != self.latest {
            return None;
        }
        let name = self.pointer.as_ref()?.name.clone();
        self.open = Some(name.clone());
        Some(name)
    }

    pub fn grace_over(&mut self, ticket: HoverTicket) {
        if ticket.0 == self.latest && self.pointer.is_none() && !self.card_hovered {
            self.open = None;
        }
    }

    /// Escape, a press on the code, or the page moving away: closed until the pointer is on another name.
    pub fn dismiss(&mut self) {
        self.next_ticket();
        self.open = None;
        self.card_hovered = false;
    }

    fn close_later(&mut self) -> Option<HoverTicket> {
        let ticket = self.next_ticket();
        self.open.is_some().then_some(ticket)
    }

    fn next_ticket(&mut self) -> HoverTicket {
        self.latest = self.latest.wrapping_add(1);
        HoverTicket(self.latest)
    }
}
