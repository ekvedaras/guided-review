use std::path::PathBuf;

use gpui::{
    AppContext, Context, Entity, FocusHandle, InteractiveElement, IntoElement, ParentElement,
    Render, Styled, Subscription, Window, div,
};

use super::components::palette::{Appearance, Colors};
use super::components::settings_panel::{Placement, SettingsEvent, SettingsPanel};

/// The root of the app's one window while no PR is open: Settings and nothing else. Settings closing,
/// by Save or Close, closes the window, as there is nothing behind it to go back to.
pub struct SettingsWindow {
    panel: Entity<SettingsPanel>,
    focus: FocusHandle,
    /// Held, not detached: once the window holds a PR, the panel closing must no longer close it.
    _panel_events: Subscription,
    _appearance: Subscription,
}

impl SettingsWindow {
    pub fn new(home: PathBuf, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        let panel =
            cx.new(|cx| SettingsPanel::new(home, focus.clone(), Placement::WholeWindow, cx));
        panel.update(cx, |panel, cx| panel.open(window, cx));
        let panel_events = cx.subscribe_in(&panel, window, |_, _, event, window, _| match event {
            SettingsEvent::Saved | SettingsEvent::Closed => window.remove_window(),
        });
        let appearance = cx.observe_window_appearance(window, |_, window, cx| {
            cx.set_global(Appearance::of_window(window.appearance()));
            cx.notify();
        });
        Self {
            panel,
            focus,
            _panel_events: panel_events,
            _appearance: appearance,
        }
    }

    /// The panel, with whatever is being edited, for the PR that takes the window over.
    pub fn panel(&self) -> Entity<SettingsPanel> {
        self.panel.clone()
    }
}

impl Render for SettingsWindow {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .track_focus(&self.focus)
            .size_full()
            .bg(Colors::active(cx).background)
            .child(self.panel.clone())
    }
}
