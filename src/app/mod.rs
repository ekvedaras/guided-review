//! The GPUI app. Shared components know nothing of the layout; a layout assembles them
//! around a `viewer::Review`, so a new layout later is one new file in `layouts`.

mod components;
mod layouts;

use std::sync::Arc;

use gpui::{App, AppContext, Bounds, Focusable, KeyBinding, WindowBounds, WindowOptions, px, size};
use gpui_platform::application;

use crate::code_source::CodeSource;
use crate::viewer::Review;
use components::palette::{Appearance, Palette};
use layouts::one_step::{GoBack, GoForward, OneStepLayout};

/// Blocks until the window is closed.
pub fn run(review: Review, source: Arc<dyn CodeSource>) {
    application().run(move |cx: &mut App| {
        cx.bind_keys([
            KeyBinding::new("right", GoForward, Some(OneStepLayout::KEY_CONTEXT)),
            KeyBinding::new("j", GoForward, Some(OneStepLayout::KEY_CONTEXT)),
            KeyBinding::new("left", GoBack, Some(OneStepLayout::KEY_CONTEXT)),
            KeyBinding::new("k", GoBack, Some(OneStepLayout::KEY_CONTEXT)),
        ]);
        let bounds = Bounds::centered(None, size(px(1200.), px(800.)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            ..Default::default()
        };
        cx.open_window(options, |window, cx| {
            Palette::set_active(Appearance::of_window(window.appearance()));
            let layout = cx.new(|cx| OneStepLayout::new(review, source, cx));
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
