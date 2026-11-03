//! The GPUI app. Shared components know nothing of the layout; a layout assembles them
//! around a `viewer::Review`, so a new layout later is one new file in `layouts`.

mod assets;
mod code_links;
mod code_selection;
pub mod components;
mod dock_icon;
mod frame_stats;
mod layouts;
mod page_rows;
mod review_session;
mod sidebar_rows;
#[cfg(feature = "snapshots")]
mod snapshots;

use std::sync::Arc;

use gpui::{
    App, AppContext, Bounds, Focusable, KeyBinding, TitlebarOptions, WindowBounds, WindowOptions,
    point, px, size,
};
use gpui_platform::application;

use crate::code_source::CodeSource;
use crate::ide::{Ide, NoIde};
use crate::viewer::{GitHubPort, Review, TourRefresher, TourRepinner};
use components::ask_panel::AskPanel;
pub use components::ask_panel::AskSetup;
use components::comment_composer::CommentComposer;
use components::jump_palette::JumpPalette;
use components::palette::Appearance;
use components::text_box;
use layouts::one_step::{
    Collaborators, CopySelection, GoBack, GoForward, OneStepLayout, ToggleJump,
};
use review_session::TourWork;

/// Blocks until the window is closed.
pub fn run(
    review: Review,
    source: Arc<dyn CodeSource>,
    ask: AskSetup,
    ide: Result<Ide, NoIde>,
    github: Arc<dyn GitHubPort>,
    repinner: TourRepinner,
    refresher: TourRefresher,
) {
    application()
        .with_assets(assets::Assets)
        .run(move |cx: &mut App| {
            dock_icon::show();
            let typing_is_not_navigation = format!(
                "{} && !{} && !{} && !{} && !{}",
                OneStepLayout::KEY_CONTEXT,
                AskPanel::KEY_CONTEXT,
                JumpPalette::KEY_CONTEXT,
                CommentComposer::KEY_CONTEXT,
                text_box::TYPING_CONTEXT
            );
            text_box::bind_keys(cx);
            cx.bind_keys([
                KeyBinding::new("right", GoForward, Some(&typing_is_not_navigation)),
                KeyBinding::new("j", GoForward, Some(&typing_is_not_navigation)),
                KeyBinding::new("left", GoBack, Some(&typing_is_not_navigation)),
                KeyBinding::new("k", GoBack, Some(&typing_is_not_navigation)),
                KeyBinding::new("cmd-k", ToggleJump, Some(OneStepLayout::KEY_CONTEXT)),
                KeyBinding::new("cmd-c", CopySelection, Some(&typing_is_not_navigation)),
            ]);
            let bounds = Bounds::centered(None, size(px(1200.), px(800.)), cx);
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("Guided review".into()),
                    appears_transparent: true,
                    traffic_light_position: Some(point(px(14.), px(14.))),
                }),
                ..Default::default()
            };
            cx.open_window(options, |window, cx| {
                cx.set_global(Appearance::of_window(window.appearance()));
                let collaborators = Collaborators {
                    github,
                    ide,
                    tour_work: TourWork {
                        repinner: Arc::new(repinner),
                        refresher: Arc::new(refresher),
                    },
                };
                let layout =
                    cx.new(|cx| OneStepLayout::new(review, source, ask, collaborators, window, cx));
                layout.update(cx, |_, cx| {
                    cx.observe_window_appearance(window, |_, window, cx| {
                        cx.set_global(Appearance::of_window(window.appearance()));
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
