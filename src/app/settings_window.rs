use crate::app_folders::AppFolders;

use gpui::{
    AppContext, ClipboardItem, Context, Entity, FocusHandle, InteractiveElement, IntoElement,
    MouseButton, MouseDownEvent, ParentElement, Render, Styled, Subscription, Window, div,
};

use super::components::palette::{Appearance, Colors};
use super::components::settings_panel::{Placement, SettingsEvent, SettingsPanel};
use super::layouts::one_step::CopySelection;
use super::prose_selection::ProseSelections;

/// The root of the app's one window while no PR is open: Settings and nothing else. Settings closing,
/// by Save or Close, closes the window, as there is nothing behind it to go back to; whether the app
/// ends with it is `AppWindow::closed`'s to say.
pub struct SettingsWindow {
    panel: Entity<SettingsPanel>,
    focus: FocusHandle,
    /// Held, not detached: once the window holds a PR, the panel closing must no longer close it.
    _panel_events: Subscription,
    _appearance: Subscription,
}

impl SettingsWindow {
    pub fn new(folders: AppFolders, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        let panel =
            cx.new(|cx| SettingsPanel::new(folders, focus.clone(), Placement::WholeWindow, cx));
        panel.update(cx, |panel, cx| panel.open(window, cx));
        let panel_events = cx.subscribe_in(&panel, window, |_, _, event, window, _| match event {
            SettingsEvent::Saved | SettingsEvent::Closed => window.remove_window(),
        });
        let appearance = cx.observe_window_appearance(window, |_, window, cx| {
            Appearance::follow(window, cx);
            cx.notify();
        });
        Self {
            panel,
            focus,
            _panel_events: panel_events,
            _appearance: appearance,
        }
    }

    /// An `open` that would have taken this window over failed; the reviewer would see nothing otherwise.
    pub fn build_failed(&mut self, reason: String, cx: &mut Context<Self>) {
        self.panel
            .update(cx, |panel, cx| panel.show_notice(reason.into(), cx));
    }

    /// The panel, with whatever is being edited, for the PR that takes the window over.
    pub fn panel(&self) -> Entity<SettingsPanel> {
        self.panel.clone()
    }
}

impl Render for SettingsWindow {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .key_context(ProseSelections::KEY_CONTEXT)
            .track_focus(&self.focus)
            .on_action(|_: &CopySelection, _, cx| {
                if let Some(text) = ProseSelections::text(cx) {
                    cx.write_to_clipboard(ClipboardItem::new_string(text));
                }
            })
            .capture_any_mouse_down(|event: &MouseDownEvent, window, cx| {
                if event.button == MouseButton::Left && ProseSelections::clear_unless_dragging(cx) {
                    window.refresh();
                }
            })
            .size_full()
            .bg(Colors::active(cx).background)
            .child(self.panel.clone())
    }
}
