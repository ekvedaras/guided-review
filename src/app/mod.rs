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
mod tour_writer;
mod word_keys;

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use gpui::{
    App, AppContext, AsyncApp, Bounds, Context, Entity, KeyBinding, TitlebarOptions, Window,
    WindowBounds, WindowHandle, WindowOptions, point, px, size,
};
use gpui_platform::application;

use crate::app_link::{Answer, AppListener, OpenRequest, Request};
use crate::github::GitHub;
use crate::hc_tour_agent::HcTourAgents;
use crate::open_review::{OpenedReview, open_review};
use crate::pull_request::PullRequestUrl;
use crate::viewer::{
    ActivePr, AgentReplied, AppStateFile, BuildTicket, Finished, HandOffId, OpenDecision,
    OpenOrigin, PrSwitcher, ReviewStateFile, TourAgents,
};
use components::ask_panel::AskPanel;
pub use components::ask_panel::AskSetup;
use components::comment_composer::CommentComposer;
use components::jump_palette::JumpPalette;
use components::palette::Appearance;
use components::pr_switcher::{PrSwitcherView, PrSwitching};
use components::settings_panel::SettingsPanel;
use components::text_box;
use layouts::one_step::{
    Collaborators, CopySelection, GoBack, GoForward, OneStepLayout, ToggleJump, ToggleSettings,
    ToggleWrapLines,
};
use review_session::TourWork;

/// What the app's loop takes in: requests from outside, and the app's own asks of itself.
enum Asked {
    Request(Request, std::sync::mpsc::Sender<Answer>),
    ReopenShown,
}

/// The one running app. It blocks until the window is closed or a quit request comes. The window opens
/// when the first `open` request has built its review.
pub fn serve(listener: AppListener, home: PathBuf) {
    let (requests, asked) = unbounded::<Asked>();
    let requests_to_self = requests.clone();
    listener.serve(move |request| {
        let (reply, answer) = std::sync::mpsc::channel();
        if requests
            .unbounded_send(Asked::Request(request, reply))
            .is_err()
        {
            return refused("the app is closing");
        }
        answer
            .recv_timeout(Duration::from_secs(10))
            .unwrap_or_else(|_| refused("the app did not answer in time"))
    });
    application()
        .with_assets(assets::Assets)
        .run(move |cx: &mut App| {
            let remembered = AppStateFile::in_home(&home);
            let active = cx.new(|_| ActivePr::remembering_in(remembered));
            let switching = pr_switching(&requests_to_self, &home, active.clone(), cx);
            switching.refresh(cx);
            cx.set_global(switching);
            tour_writer::resume(cx);
            dock_icon::show();
            bind_keys(cx);
            word_keys::take_before_macos(cx);
            cx.on_window_closed(|cx, _| cx.quit()).detach();
            cx.spawn(async move |cx| answer_requests(asked, active, home, cx).await)
                .detach();
        });
}

/// Picking a PR in the switcher is an open request the app makes of itself, so it takes the same path
/// as one from the command line. No one waits for its answer; a failed build shows in the window.
fn pr_switching(
    requests: &UnboundedSender<Asked>,
    home: &Path,
    active: Entity<ActivePr>,
    cx: &mut App,
) -> PrSwitching {
    let requests = requests.clone();
    let reopen_requests = requests.clone();
    let home_folder = home.to_path_buf();
    PrSwitching {
        relevant: Arc::new(GitHub::from_env()),
        home: home.to_path_buf(),
        active,
        agents: Arc::new(HcTourAgents::new()),
        list: cx.new(|_| PrSwitcher::default()),
        show: Arc::new(move |pr| {
            let (reply, _) = std::sync::mpsc::channel();
            let open = open_request_of(&home_folder, &pr);
            if requests
                .unbounded_send(Asked::Request(Request::Open(open), reply))
                .is_err()
            {
                eprintln!("could not switch to {pr}: the app is closing");
            }
        }),
        reopen: Arc::new(move || {
            if reopen_requests.unbounded_send(Asked::ReopenShown).is_err() {
                eprintln!("could not reopen the PR: the app is closing");
            }
        }),
    }
}

/// The open of a PR the app makes of itself, from the folder its state file remembers. An unreadable
/// state file only loses the remembered clone folder.
fn open_request_of(home: &Path, pr: &PullRequestUrl) -> OpenRequest {
    let folder = ReviewStateFile::for_pr(home, pr)
        .opened_from()
        .ok()
        .flatten()
        .unwrap_or_else(|| home.to_path_buf());
    OpenRequest {
        pr: pr.to_string(),
        folder,
        terminal: None,
    }
}

fn refused(reason: &str) -> Answer {
    Answer::Refused {
        reason: reason.to_owned(),
    }
}

/// What the app shows now. The window is there once the first review is built.
struct Screen {
    active: Entity<ActivePr>,
    window: Option<WindowHandle<OneStepLayout>>,
}

type SharedScreen = Rc<RefCell<Screen>>;

async fn answer_requests(
    mut asked: UnboundedReceiver<Asked>,
    active: Entity<ActivePr>,
    home: PathBuf,
    cx: &mut AsyncApp,
) {
    let screen: SharedScreen = Rc::new(RefCell::new(Screen {
        active,
        window: None,
    }));
    while let Some(asked) = asked.next().await {
        let (request, reply) = match asked {
            Asked::Request(request, reply) => (request, reply),
            Asked::ReopenShown => {
                reopen_shown(&screen, &home, cx);
                continue;
            }
        };
        match request {
            Request::Quit => {
                reply.send(Answer::Quitting).ok();
                cx.update(|cx| cx.quit());
            }
            Request::AgentReplied {
                pr,
                hand_off,
                replied,
            } => {
                let answer = mark_replied(&screen, pr, hand_off, &replied, cx);
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
                let active = screen.borrow().active.clone();
                let decision = active.update(cx, |active, cx| {
                    let decision = active.request(pr.clone(), origin);
                    cx.notify();
                    decision
                });
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

/// The save of the settings asks for the PR on screen to be built again. The window is held first (typing
/// saved, focus out of the boxes) so the build reads the state file the old session no longer changes;
/// `reopen` then raises the Opening veil and gives the ticket, and the build takes the path of any switch.
fn reopen_shown(screen: &SharedScreen, home: &Path, cx: &mut AsyncApp) {
    let (active, window) = {
        let screen = screen.borrow();
        (screen.active.clone(), screen.window)
    };
    if let Some(window) = window {
        cx.update(|cx| {
            window.update(cx, OneStepLayout::hold_for_reopen).ok();
        });
    }
    let reopening = active.update(cx, |active, cx| {
        let reopening = active.reopen();
        cx.notify();
        reopening
    });
    let Some((ticket, pr)) = reopening else {
        return;
    };
    let open = open_request_of(home, &pr);
    let (screen, home) = (Rc::clone(screen), home.to_path_buf());
    cx.spawn(async move |cx| build_and_show(screen, ticket, pr, open, home, cx).await)
        .detach();
}

/// Only the PR in the window is marked here, because its session holds the state it writes back. For
/// any other PR the command marks the state file.
fn mark_replied(
    screen: &SharedScreen,
    pr: String,
    hand_off: HandOffId,
    replied: &AgentReplied,
    cx: &mut AsyncApp,
) -> Answer {
    let (active, window) = {
        let screen = screen.borrow();
        (screen.active.clone(), screen.window)
    };
    let shown = active.read_with(cx, |active, _| active.shown().map(ToString::to_string));
    let window = window.filter(|_| shown.as_deref() == Some(pr.as_str()));
    let Some(window) = window else {
        if let Ok(replied) = pr.parse::<PullRequestUrl>() {
            active.update(cx, |active, cx| {
                active.agent_replied(&replied);
                cx.notify();
            });
        }
        return Answer::NotShown { pr };
    };
    let marked = cx.update(|cx| {
        window
            .update(cx, |layout, _, cx| {
                layout.hand_off_replied(hand_off, replied, cx)
            })
            .and_then(|marked| marked)
    });
    match marked {
        Ok(()) => {
            if let Ok(replied_pr) = pr.parse::<PullRequestUrl>() {
                active.update(cx, |active, _| {
                    active.hold_reply_for_reopen(&replied_pr, hand_off, replied);
                });
            }
            Answer::HandOffAnswered { pr }
        }
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
    let active = screen.borrow().active.clone();
    let finished = active.update(cx, |active, cx| {
        let finished = active.finished(ticket, built);
        cx.notify();
        finished
    });
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
                Ok(window) => {
                    screen.borrow_mut().window = Some(window);
                    apply_replies_held_for_reopen(&active, window, cx);
                }
                Err(error) => {
                    eprintln!("could not show {pr}: {error:#}");
                    std::process::exit(1);
                }
            }
        }
    }
}

/// The replies that landed while the PR reopened were applied to the old window, whose state the build
/// may have read too early; the new window gets them again.
fn apply_replies_held_for_reopen(
    active: &Entity<ActivePr>,
    window: WindowHandle<OneStepLayout>,
    cx: &mut AsyncApp,
) {
    let held = active.update(cx, |active, _| active.take_replies_for_reopened());
    for (id, replied) in held {
        let applied = cx.update(|cx| {
            window
                .update(cx, |layout, _, cx| {
                    layout.hand_off_replied(id, &replied, cx)
                })
                .and_then(|applied| applied)
        });
        if let Err(error) = applied {
            eprintln!("could not apply an agent reply to the reopened PR: {error:#}");
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
        "{} && !{} && !{} && !{} && !{} && !{} && !{}",
        OneStepLayout::KEY_CONTEXT,
        AskPanel::KEY_CONTEXT,
        JumpPalette::KEY_CONTEXT,
        PrSwitcherView::KEY_CONTEXT,
        SettingsPanel::KEY_CONTEXT,
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
        KeyBinding::new("cmd-,", ToggleSettings, Some(OneStepLayout::KEY_CONTEXT)),
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
