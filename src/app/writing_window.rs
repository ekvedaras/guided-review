use gpui::{
    AppContext, Context, Entity, InteractiveElement, IntoElement, ParentElement, Render, Styled,
    Subscription, Window, div,
};

use super::components::loading_screen::LoadingScreen;
use super::components::palette::{Appearance, Colors};
use crate::viewer::ActivePr;

/// The root of the app's one window while no PR is open and a tour is being written for the one asked
/// for: the loading screen and nothing else. The review takes the window over when the tour is written.
pub struct WritingWindow {
    loading: Entity<LoadingScreen>,
    _appearance: Subscription,
}

impl WritingWindow {
    pub fn new(active: &Entity<ActivePr>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let loading = cx.new(|cx| LoadingScreen::new(active, cx));
        let appearance = cx.observe_window_appearance(window, |_, window, cx| {
            cx.set_global(Appearance::of(window));
            cx.notify();
        });
        Self {
            loading,
            _appearance: appearance,
        }
    }
}

impl Render for WritingWindow {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("writing-window")
            .relative()
            .size_full()
            .bg(Colors::active(cx).background)
            .child(self.loading.clone())
    }
}
