//! The Definition card over a name in a diff line: the pointer's rest opens it, the pointer on the
//! name or the card keeps it, Escape, a press on the code or the page moving close it. When it
//! opens is the viewer's `NameHover`; this draws it and runs its timers.

use std::rc::Rc;
use std::time::Duration;

use gpui::{
    AnyElement, Context, InteractiveElement, IntoElement, ParentElement, Pixels, Point,
    StatefulInteractiveElement, Styled, div,
};

use crate::app::code_links::open_code;
use crate::app::components::diff_view::NamePointer;
use crate::app::components::name_card::{OnNameTarget, name_card};
use crate::app::components::overlay::popover_at;
use crate::app::components::palette::Colors;
use crate::viewer::{HoverTicket, NameTarget, Screen};

use super::OneStepLayout;

/// How long the pointer rests on a name before its card opens, as a tooltip waits.
const REST: Duration = Duration::from_millis(500);
/// Between the pointer leaving the name and the card closing, long enough to cross to the card.
const GRACE: Duration = Duration::from_millis(150);

/// Where the open card is, and what the page looked like then: a card does not follow a page that
/// moved under it.
pub(super) struct NameCardPlace {
    at: Point<Pixels>,
    screen: Screen,
    scrolled_to: (usize, Pixels),
}

impl OneStepLayout {
    pub(super) fn on_name_pointer(&mut self, pointer: NamePointer, cx: &mut Context<Self>) {
        match pointer {
            NamePointer::On { under, card_at } => {
                let shown_before = self.name_hover.shown().cloned();
                if let Some(ticket) = self.name_hover.pointer_on(under) {
                    self.name_card_waits_at = card_at;
                    self.wait(REST, ticket, Self::name_rested, cx);
                }
                if shown_before.as_ref() != self.name_hover.shown() {
                    cx.notify();
                }
            }
            NamePointer::Off => {
                if let Some(ticket) = self.name_hover.pointer_off() {
                    self.wait(GRACE, ticket, Self::name_grace_over, cx);
                }
            }
        }
    }

    /// Opens the card at once at `at`, for a snapshot scene.
    #[cfg(feature = "snapshots")]
    pub(super) fn show_staged_name_card(
        &mut self,
        under: crate::viewer::NameUnderPointer,
        at: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        if let Some(ticket) = self.name_hover.pointer_on(under) {
            self.name_hover.rested(ticket);
        }
        self.name_card_place = Some(NameCardPlace {
            at,
            screen: self.session.read(cx).review().screen().clone(),
            scrolled_to: self.scrolled_to(),
        });
    }

    pub(super) fn dismiss_name_card(&mut self, cx: &mut Context<Self>) {
        if self.name_hover.shown().is_some() {
            cx.notify();
        }
        self.name_hover.dismiss();
        self.name_card_place = None;
    }

    fn wait(
        &self,
        time: Duration,
        ticket: HoverTicket,
        then: fn(&mut Self, HoverTicket, &mut Context<Self>),
        cx: &mut Context<Self>,
    ) {
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(time).await;
            this.update(cx, |this, cx| then(this, ticket, cx)).ok();
        })
        .detach();
    }

    fn name_rested(&mut self, ticket: HoverTicket, cx: &mut Context<Self>) {
        let Some(name) = self.name_hover.rested(ticket) else {
            return;
        };
        self.name_card_place = Some(NameCardPlace {
            at: self.name_card_waits_at,
            screen: self.session.read(cx).review().screen().clone(),
            scrolled_to: self.scrolled_to(),
        });
        self.session
            .update(cx, |session, cx| session.look_up_name(&name, cx));
        cx.notify();
    }

    fn name_grace_over(&mut self, ticket: HoverTicket, cx: &mut Context<Self>) {
        self.name_hover.grace_over(ticket);
        if self.name_hover.shown().is_none() {
            self.name_card_place = None;
            cx.notify();
        }
    }

    fn scrolled_to(&self) -> (usize, Pixels) {
        let top = self.screen_list.logical_scroll_top();
        (top.item_ix, top.offset_in_item)
    }

    fn open_name_target(&mut self, target: &NameTarget, cx: &mut Context<Self>) {
        match target {
            NameTarget::Tour(target) => self.change_review(cx, |review| review.jump_to(target)),
            NameTarget::Code(link) => open_code(link, cx),
        }
        self.dismiss_name_card(cx);
        cx.notify();
    }

    pub(super) fn close_name_card_by_key(
        &mut self,
        event: &gpui::KeyDownEvent,
        cx: &mut Context<Self>,
    ) {
        if event.keystroke.key == "escape" {
            self.dismiss_name_card(cx);
            cx.stop_propagation();
        }
    }

    /// The card of the name the pointer rests on, drawn over the page.
    pub(super) fn name_card_layer(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let name = self.name_hover.shown()?.clone();
        let place = self.name_card_place.as_ref()?;
        let at = place.at;
        let page_moved = place.screen != *self.session.read(cx).review().screen()
            || place.scrolled_to != self.scrolled_to();
        if page_moved {
            self.dismiss_name_card(cx);
            return None;
        }
        let card = self.session.read(cx).review().definition_card(&name)?;
        let layout = cx.weak_entity();
        let on_open: OnNameTarget = Rc::new(move |target, _, cx| {
            let target = target.clone();
            layout
                .update(cx, |this, cx| this.open_name_target(&target, cx))
                .ok();
        });
        let colors = Colors::active(cx);
        Some(
            popover_at(
                at,
                div()
                    .id("name-card")
                    .pt_1()
                    .on_hover(cx.listener(|this, is_hovered: &bool, _, cx| {
                        if *is_hovered {
                            this.name_hover.card_entered();
                        } else if let Some(ticket) = this.name_hover.card_left() {
                            this.wait(GRACE, ticket, Self::name_grace_over, cx);
                        }
                    }))
                    .child(name_card(&colors, &card, &on_open)),
            )
            .into_any_element(),
        )
    }
}
