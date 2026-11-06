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

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use gpui::{
    App, AppContext, AsyncApp, Bounds, Context, KeyBinding, TitlebarOptions, Window, WindowBounds,
    WindowHandle, WindowOptions, point, px, size,
};
use gpui_platform::application;

use crate::app_link::{Answer, AppListener, OpenRequest, Request};
use crate::github::GitHub;
use crate::hc_tour_agent::HcTourAgents;
use crate::open_review::{OpenedReview, open_review};
use crate::pull_request::PullRequestUrl;
use crate::viewer::{
    ActivePr, BuildTicket, Finished, HandOffId, OpenDecision, OpenOrigin, ReviewStateFile,
    TourAgents,
};
use components::ask_panel::AskPanel;
pub use components::ask_panel::AskSetup;
use components::comment_composer::CommentComposer;
use components::jump_palette::JumpPalette;
use components::palette::Appearance;
use components::pr_switcher::{PrSwitcherView, PrSwitching};
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
    let requests_to_self = requests.clone();
    listener.serve(move |request| {
        let (reply, answer) = std::sync::mpsc::channel();
        if requests.unbounded_send((request, reply)).is_err() {
            return refused("the app is closing");
        }
        answer
            .recv_timeout(Duration::from_secs(10))
            .unwrap_or_else(|_| refused("the app did not answer in time"))
    });
    let switching = pr_switching(&requests_to_self, &home);
    application()
        .with_assets(assets::Assets)
        .run(move |cx: &mut App| {
            cx.set_global(switching);
            dock_icon::show();
            bind_keys(cx);
            cx.on_window_closed(|cx, _| cx.quit()).detach();
            cx.spawn(async move |cx| answer_requests(asked, home, cx).await)
                .detach();
        });
}

/// Picking a PR in the switcher is an open request the app makes of itself, so it takes the same path
/// as one from the command line. No one waits for its answer; a failed build shows in the window.
fn pr_switching(requests: &UnboundedSender<Asked>, home: &Path) -> PrSwitching {
    let requests = requests.clone();
    let home_folder = home.to_path_buf();
    PrSwitching {
        relevant: Arc::new(GitHub::from_env()),
        home: home.to_path_buf(),
        show: Arc::new(move |pr| {
            // An unreadable state file only loses the remembered clone folder.
            let folder = ReviewStateFile::for_pr(&home_folder, &pr)
                .opened_from()
                .ok()
                .flatten()
                .unwrap_or_else(|| home_folder.clone());
            let (reply, _) = std::sync::mpsc::channel();
            let open = OpenRequest {
                pr: pr.to_string(),
                folder,
                terminal: None,
            };
            if requests
                .unbounded_send((Request::Open(open), reply))
                .is_err()
            {
                eprintln!("could not switch to {pr}: the app is closing");
            }
        }),
    }
}

fn refused(reason: &str) -> Answer {
    Answer::Refused {
        reason: reason.to_owned(),
    }
}

/// What the app shows now. The window is there once the first review is built.
struct Screen {
    active: ActivePr,
    window: Option<WindowHandle<OneStepLayout>>,
}

type SharedScreen = Rc<RefCell<Screen>>;

async fn answer_requests(mut asked: UnboundedReceiver<Asked>, home: PathBuf, cx: &mut AsyncApp) {
    let screen: SharedScreen = Rc::new(RefCell::new(Screen {
        active: ActivePr::default(),
        window: None,
    }));
    while let Some((request, reply)) = asked.next().await {
        match request {
            Request::Quit => {
                reply.send(Answer::Quitting).ok();
                cx.update(|cx| cx.quit());
            }
            Request::AgentReplied { pr, hand_off } => {
                let answer = mark_answered(&screen, pr, hand_off, cx);
                reply.send(answer).ok();
            }
            Request::Open(open) => {
                let Ok(pr) = open.pr.parse::<PullRequestUrl>() else {
                    reply
                        .send(refused(&format!("{} is not a PR link", open.pr)))
                        .ok();
                    continue;
                };
                claim_terminal(&pr, &open, cx);
                let origin = origin_of(&open, cx).await;
                let decision = screen.borrow_mut().active.request(pr.clone(), origin);
                match decision {
                    OpenDecision::KeepInBackground => {
                        reply.send(Answer::KeptInBackground { pr: open.pr }).ok();
                    }
                    OpenDecision::Reload => {
                        reply.send(Answer::Reloaded { pr: open.pr }).ok();
                        let window = screen.borrow().window;
                        if let Some(window) = window {
                            show_again(window, cx);
                        }
                    }
                    OpenDecision::Build(ticket) => {
                        reply
                            .send(Answer::Opening {
                                pr: open.pr.clone(),
                            })
                            .ok();
                        let (screen, home) = (Rc::clone(&screen), home.clone());
                        cx.spawn(async move |cx| {
                            build_and_show(screen, ticket, pr, open, home, cx).await;
                        })
                        .detach();
                    }
                }
            }
        }
    }
}

/// Only the PR in the window is marked here, because its session holds the state it writes back. For
/// any other PR the command marks the state file.
fn mark_answered(
    screen: &SharedScreen,
    pr: String,
    hand_off: HandOffId,
    cx: &mut AsyncApp,
) -> Answer {
    let window = {
        let screen = screen.borrow();
        let shown = screen.active.shown().map(ToString::to_string);
        screen
            .window
            .filter(|_| shown.as_deref() == Some(pr.as_str()))
    };
    let Some(window) = window else {
        return Answer::NotShown { pr };
    };
    let marked = cx.update(|cx| {
        window
            .update(cx, |layout, _, cx| layout.hand_off_answered(hand_off, cx))
            .and_then(|marked| marked)
    });
    match marked {
        Ok(()) => Answer::HandOffAnswered { pr },
        Err(error) => refused(&format!("{error:#}")),
    }
}

/// The terminal an `open` came from becomes the PR's tour agent, whatever the request then does. Slow
/// (it runs `hc`), so it runs off the UI thread; a failure is for the app's log, as the window may stay.
fn claim_terminal(pr: &PullRequestUrl, open: &OpenRequest, cx: &mut AsyncApp) {
    let (pr, terminal) = (pr.clone(), open.terminal.clone());
    cx.background_executor()
        .spawn(async move {
            let agents = TourAgents::new(Arc::new(HcTourAgents::new()));
            if let Err(error) = agents.claim_for_open(&pr, terminal.as_deref()) {
                eprintln!("could not claim the terminal for {pr}: {error:#}");
            }
        })
        .detach();
}

/// Asks herdr whether the open came from a tour agent the app started; off the UI thread, as it runs `hc`.
async fn origin_of(open: &OpenRequest, cx: &mut AsyncApp) -> OpenOrigin {
    let Some(terminal) = open.terminal.clone() else {
        return OpenOrigin::Reviewer;
    };
    cx.background_executor()
        .spawn(async move {
            TourAgents::new(Arc::new(HcTourAgents::new())).origin_of(Some(&terminal))
        })
        .await
}

fn show_again(window: WindowHandle<OneStepLayout>, cx: &mut AsyncApp) {
    cx.update(|cx| {
        window
            .update(cx, |layout, window, cx| {
                layout.reload_tour(cx);
                window.activate_window();
            })
            .ok();
        cx.activate(true);
    });
}

/// The review is built off the UI thread while the PR on screen stays. When the very first build fails
/// there is no window to keep, so the app ends with the reason in its log; later, the reason is shown
/// in the window that stays.
async fn build_and_show(
    screen: SharedScreen,
    ticket: BuildTicket,
    pr: PullRequestUrl,
    open: OpenRequest,
    home: PathBuf,
    cx: &mut AsyncApp,
) {
    let (build_pr, folder) = (pr.clone(), open.folder.clone());
    let built = cx
        .background_executor()
        .spawn(async move { open_review(&build_pr, &folder, &home) })
        .await;
    let finished = screen.borrow_mut().active.finished(ticket, built);
    let window = screen.borrow().window;
    match (finished, window) {
        (Finished::Superseded, _) => {}
        (Finished::Stay { reason }, None) => {
            eprintln!("{reason}");
            std::process::exit(1);
        }
        (Finished::Stay { reason }, Some(window)) => {
            eprintln!("{reason}");
            cx.update(|cx| {
                window
                    .update(cx, |layout, _, cx| layout.switch_failed(reason, cx))
                    .ok();
            });
        }
        (Finished::Switch(opened), window) => {
            let shown = cx.update(|cx| {
                let shown = match window {
                    Some(window) => switch_window(window, opened, &pr, cx).map(|()| window),
                    None => open_window(opened, &pr, cx),
                };
                cx.activate(true);
                shown
            });
            match shown {
                Ok(window) => screen.borrow_mut().window = Some(window),
                Err(error) => {
                    eprintln!("could not show {pr}: {error:#}");
                    std::process::exit(1);
                }
            }
        }
    }
}

/// Typing is saved first. The old root, with its session, Asks and code source, is dropped by the swap.
fn switch_window(
    window: WindowHandle<OneStepLayout>,
    opened: OpenedReview,
    pr: &PullRequestUrl,
    cx: &mut App,
) -> anyhow::Result<()> {
    window.update(cx, |layout, window, cx| {
        layout.flush_typing(cx);
        window.replace_root(cx, |window, cx| new_layout(opened, pr, window, cx));
    })
}

fn bind_keys(cx: &mut App) {
    let typing_is_not_navigation = format!(
        "{} && !{} && !{} && !{} && !{} && !{}",
        OneStepLayout::KEY_CONTEXT,
        AskPanel::KEY_CONTEXT,
        JumpPalette::KEY_CONTEXT,
        PrSwitcherView::KEY_CONTEXT,
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

fn open_window(
    opened: OpenedReview,
    pr: &PullRequestUrl,
    cx: &mut App,
) -> anyhow::Result<WindowHandle<OneStepLayout>> {
    let bounds = Bounds::centered(None, size(px(1200.), px(800.)), cx);
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: Some(TitlebarOptions {
            title: Some(window_title(&opened, pr).into()),
            appears_transparent: true,
            traffic_light_position: Some(point(px(14.), px(14.))),
        }),
        ..Default::default()
    };
    cx.open_window(options, |window, cx| {
        cx.set_global(Appearance::of_window(window.appearance()));
        cx.new(|cx| new_layout(opened, pr, window, cx))
    })
}

fn window_title(opened: &OpenedReview, pr: &PullRequestUrl) -> String {
    format!("{pr}: {}", opened.review.title())
}

fn new_layout(
    opened: OpenedReview,
    pr: &PullRequestUrl,
    window: &mut Window,
    cx: &mut Context<'_, OneStepLayout>,
) -> OneStepLayout {
    window.set_window_title(&window_title(&opened, pr));
    let collaborators = Collaborators {
        github: opened.github,
        ide: opened.ide,
        tour_work: TourWork {
            repinner: Arc::new(opened.repinner),
            refresher: Arc::new(opened.refresher),
            reloader: Arc::new(opened.reloader),
            tour_agents: opened.tour_agents,
        },
    };
    let layout = OneStepLayout::new(
        opened.review,
        opened.source,
        opened.ask,
        collaborators,
        window,
        cx,
    );
    cx.observe_window_appearance(window, |_, window, cx| {
        cx.set_global(Appearance::of_window(window.appearance()));
        cx.notify();
    })
    .detach();
    cx.focus_self(window);
    layout
}
