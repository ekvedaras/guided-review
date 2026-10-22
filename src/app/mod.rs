//! The GPUI app. Shared components know nothing of the layout; a layout assembles them
//! around a `viewer::Review`, so a new layout later is one new file in `layouts`.

pub mod components;
mod layouts;

use std::sync::Arc;

use gpui::{App, AppContext, Bounds, Focusable, KeyBinding, WindowBounds, WindowOptions, px, size};
use gpui_platform::application;

use crate::code_source::CodeSource;
use crate::ide::Ide;
use crate::viewer::{GitHubPort, Review, TourUpdater};
use components::ask_panel::AskPanel;
pub use components::ask_panel::AskSetup;
use components::comment_composer::CommentComposer;
use components::jump_palette::JumpPalette;
use components::palette::{Appearance, Palette};
use layouts::one_step::{GoBack, GoForward, OneStepLayout, ToggleJump};

/// Blocks until the window is closed.
pub fn run(
    review: Review,
    source: Arc<dyn CodeSource>,
    ask: AskSetup,
    ide: Option<Ide>,
    github: Arc<dyn GitHubPort>,
    updater: TourUpdater,
) {
    application().run(move |cx: &mut App| {
        let typing_is_not_navigation = format!(
            "{} && !{} && !{} && !{}",
            OneStepLayout::KEY_CONTEXT,
            AskPanel::KEY_CONTEXT,
            JumpPalette::KEY_CONTEXT,
            CommentComposer::KEY_CONTEXT
        );
        cx.bind_keys([
            KeyBinding::new("right", GoForward, Some(&typing_is_not_navigation)),
            KeyBinding::new("j", GoForward, Some(&typing_is_not_navigation)),
            KeyBinding::new("left", GoBack, Some(&typing_is_not_navigation)),
            KeyBinding::new("k", GoBack, Some(&typing_is_not_navigation)),
            KeyBinding::new("cmd-k", ToggleJump, Some(OneStepLayout::KEY_CONTEXT)),
        ]);
        let bounds = Bounds::centered(None, size(px(1200.), px(800.)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            ..Default::default()
        };
        cx.open_window(options, |window, cx| {
            Palette::set_active(Appearance::of_window(window.appearance()));
            let updater = Arc::new(updater);
            let layout = cx.new(|cx| {
                OneStepLayout::new(review, source, ask, ide, github, updater, window, cx)
            });
            layout.update(cx, |_, cx| {
                cx.observe_window_appearance(window, |_, window, cx| {
                    Palette::set_active(Appearance::of_window(window.appearance()));
                    cx.notify();
                })
                .detach();
            });
            layout.focus_handle(cx).focus(window, cx);
            layout
        })
        .expect("could not open the app window");
        cx.on_window_closed(|cx, _| cx.quit()).detach();
        cx.activate(true);
    });
}
