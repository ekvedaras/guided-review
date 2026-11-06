//! The GPUI app. Shared components know nothing of the layout; a layout assembles them
//! around a `viewer::Review`, so a new layout later is one new file in `layouts`.

mod active_session;
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

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;
use futures::channel::mpsc::{UnboundedReceiver, unbounded};
use gpui::{
    App, AppContext, AsyncApp, Bounds, Focusable, KeyBinding, TitlebarOptions, WindowBounds,
    WindowHandle, WindowOptions, point, px, size,
};
use gpui_platform::application;

use crate::app_link::{Answer, AppListener, OpenRequest, Request};
use crate::open_review::{OpenedReview, open_review};
use crate::pull_request::PullRequestUrl;
use components::ask_panel::AskPanel;
pub use components::ask_panel::AskSetup;
use components::comment_composer::CommentComposer;
use components::jump_palette::JumpPalette;
use components::palette::Appearance;
use components::text_box;
use layouts::one_step::{
    Collaborators, CopySelection, GoBack, GoForward, OneStepLayout, ToggleJump, ToggleWrapLines,
};
use review_session::TourWork;

type Asked = (Request, std::sync::mpsc::Sender<Answer>);

/// The one running app. It blocks until the window is closed or a quit request comes. The window opens
/// when the first `open` request has built its review.
pub fn serve(listener: AppListener, home: PathBuf) {
    let (requests, asked) = unbounded::<Asked>();
    listener.serve(move |request| {
        let (reply, answer) = std::sync::mpsc::channel();
        if requests.unbounded_send((request, reply)).is_err() {
            return refused("the app is closing");
        }
        answer
            .recv_timeout(Duration::from_secs(10))
            .unwrap_or_else(|_| refused("the app did not answer in time"))
    });
    application()
        .with_assets(assets::Assets)
        .run(move |cx: &mut App| {
            dock_icon::show();
            bind_keys(cx);
            cx.on_window_closed(|cx, _| cx.quit()).detach();
            cx.spawn(async move |cx| answer_requests(asked, home, cx).await)
                .detach();
        });
}

fn refused(reason: &str) -> Answer {
    Answer::Refused {
        reason: reason.to_owned(),
    }
}

struct Shown {
    window: WindowHandle<OneStepLayout>,
    pr: PullRequestUrl,
}

async fn answer_requests(mut asked: UnboundedReceiver<Asked>, home: PathBuf, cx: &mut AsyncApp) {
    let mut shown: Option<Shown> = None;
    while let Some((request, reply)) = asked.next().await {
        match request {
            Request::Quit => {
                reply.send(Answer::Quitting).ok();
                cx.update(|cx| cx.quit());
            }
            Request::Open(open) => {
                let Ok(pr) = open.pr.parse::<PullRequestUrl>() else {
                    reply
                        .send(refused(&format!("{} is not a PR link", open.pr)))
                        .ok();
                    continue;
                };
                match &shown {
                    Some(current) if current.pr == pr => {
                        reply.send(Answer::Reloaded { pr: open.pr }).ok();
                        show_again(current, cx);
                    }
                    Some(current) => {
                        reply
                            .send(refused(&format!(
                                "the app shows {} and cannot switch to {pr} yet",
                                current.pr
                            )))
                            .ok();
                    }
                    None => {
                        reply
                            .send(Answer::Opening {
                                pr: open.pr.clone(),
                            })
                            .ok();
                        shown = Some(first_window(pr, &open, &home, cx).await);
                    }
                }
            }
        }
    }
}

fn show_again(current: &Shown, cx: &mut AsyncApp) {
    cx.update(|cx| {
        current
            .window
            .update(cx, |layout, window, cx| {
                layout.reload_tour(cx);
                window.activate_window();
            })
            .ok();
        cx.activate(true);
    });
}

/// The review is built off the UI thread. When it cannot be built there is no window to keep, so the app
/// ends with the reason in its log.
async fn first_window(
    pr: PullRequestUrl,
    open: &OpenRequest,
    home: &Path,
    cx: &mut AsyncApp,
) -> Shown {
    let (build_pr, folder, home) = (pr.clone(), open.folder.clone(), home.to_path_buf());
    let built = cx
        .background_executor()
        .spawn(async move { open_review(&build_pr, &folder, &home) })
        .await;
    let opened = match built {
        Ok(opened) => opened,
        Err(error) => {
            eprintln!("could not open {pr}: {error:#}");
            std::process::exit(1);
        }
    };
    let window = cx.update(|cx| {
        let window = open_window(opened, cx);
        cx.activate(true);
        window
    });
    match window {
        Ok(window) => Shown { window, pr },
        Err(error) => {
            eprintln!("could not open the app window: {error:#}");
            std::process::exit(1);
        }
    }
}

fn bind_keys(cx: &mut App) {
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
        KeyBinding::new("w", ToggleWrapLines, Some(&typing_is_not_navigation)),
        KeyBinding::new("cmd-k", ToggleJump, Some(OneStepLayout::KEY_CONTEXT)),
        KeyBinding::new("cmd-c", CopySelection, Some(&typing_is_not_navigation)),
    ]);
}

fn open_window(opened: OpenedReview, cx: &mut App) -> anyhow::Result<WindowHandle<OneStepLayout>> {
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
            github: opened.github,
            ide: opened.ide,
            tour_work: TourWork {
                repinner: Arc::new(opened.repinner),
                refresher: Arc::new(opened.refresher),
                reloader: Arc::new(opened.reloader),
            },
        };
        let layout = cx.new(|cx| {
            OneStepLayout::new(
                opened.review,
                opened.source,
                opened.ask,
                collaborators,
                window,
                cx,
            )
        });
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
}
