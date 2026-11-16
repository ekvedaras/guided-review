use gpui::{
    AppContext, Context, Entity, FocusHandle, InteractiveElement, IntoElement, ParentElement,
    Render, Styled, Subscription, Window, div,
};

use super::components::palette::{Appearance, Colors};
use super::components::pr_switcher::{
    PrSwitcherView, PrSwitching, SwitcherDismissed, SwitcherPlace,
};
use super::components::switching_veil::SwitchingVeil;
use crate::viewer::ActivePr;

/// The root of the app's one window while no PR is open yet, as after a launch from the Dock: the PR
/// switcher and nothing else. The picked PR's review takes the window over; closing the switcher
/// closes the window, and whether the app ends with it is `AppWindow::closed`'s to say.
pub struct SwitcherWindow {
    switcher: Entity<PrSwitcherView>,
    /// Says which picked PR is opening, which the veil over the switcher names.
    active: Entity<ActivePr>,
    focus: FocusHandle,
    _dismissed: Subscription,
    _appearance: Subscription,
    _opening: Subscription,
}

impl SwitcherWindow {
    pub fn new(switching: PrSwitching, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        let active = switching.active.clone();
        let opening = cx.observe(&active, |_, _, cx| cx.notify());
        let switcher = cx.new(|cx| {
            PrSwitcherView::new(switching, SwitcherPlace::WholeWindow, focus.clone(), cx)
        });
        switcher.update(cx, |switcher, cx| switcher.open(window, cx));
        let dismissed = cx.subscribe_in(&switcher, window, |_, _, SwitcherDismissed, window, _| {
            window.remove_window();
        });
        let appearance = cx.observe_window_appearance(window, |_, window, cx| {
            Appearance::follow(window, cx);
            cx.notify();
        });
        Self {
            switcher,
            active,
            focus,
            _dismissed: dismissed,
            _appearance: appearance,
            _opening: opening,
        }
    }

    /// The picked PR could not be opened; the switcher stays for another pick.
    pub fn build_failed(&mut self, reason: String, cx: &mut Context<Self>) {
        self.switcher
            .update(cx, |switcher, cx| switcher.refuse(reason, cx));
    }
}

impl Render for SwitcherWindow {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let switching_veil = self.active.read(cx).switching_to().map(SwitchingVeil::to);
        div()
            .track_focus(&self.focus)
            .size_full()
            .bg(Colors::active(cx).background)
            .child(self.switcher.clone())
            .children(switching_veil)
    }
}
