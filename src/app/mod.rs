//! The GPUI app. Shared components know nothing of the layout; a layout assembles them
//! around a `viewer::Review`, so a new layout later is one new file in `layouts`.

mod active_session;
pub mod assets;
mod avatar_pictures;
mod code_links;
mod code_selection;
pub mod components;
mod dock_icon;
mod frame_stats;
pub mod keymap;
mod layouts;
mod page_rows;
mod prose_selection;
mod review_session;
mod settings_window;
mod sidebar_rows;
#[cfg(feature = "snapshots")]
mod snapshots;
mod switcher_window;
mod tour_writer;
mod updater;
mod word_keys;
mod writing_window;

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use gpui::{
    App, AppContext, AsyncApp, Bounds, Context, Entity, KeyBinding, Menu, MenuItem,
    TitlebarOptions, Window, WindowBounds, WindowHandle, WindowOptions, actions, point, px, size,
};
use gpui_platform::application;

use crate::app_link::{Answer, AppListener, OpenRequest, Request};
use crate::github::GitHub;
use crate::hc_tour_agent::HcTourAgents;
use crate::open_review::{OpenedReview, open_review};
use crate::pull_request::PullRequestUrl;
use crate::viewer::{
    ActivePr, AfterClose, AgentReplied, AppStateFile, AppWindow, BuildTicket, Command, FailedBuild,
    Finished, HandOffId, KeyScope, Keymap, OpenDecision, OpenOrigin, OpenOutcome, PrSwitcher,
    RelevantPullRequestsPort, RememberedPullRequestsFile, ReviewStateFile, ShowReview,
    ShowSettings, ShowSwitcher, ShowWriting, ToastTone, TourAgents, TourFact, WriteDecision,
};
use components::ask_panel::AskPanel;
pub use components::ask_panel::AskSetup;
use components::comment_composer::CommentComposer;
use components::jump_palette::JumpPalette;
use components::palette::Appearance;
use components::picture_zoom::PictureZoom;
use components::pr_switcher::{PrSwitcherView, PrSwitching};
use components::settings_panel::SettingsPanel;
use components::shortcuts_sheet::ShortcutsSheet;
use components::text_box;
use layouts::one_step::{
    CloseWindow, Collaborators, CopySelection, GoBack, GoForward, GoToNextUnread, OneStepLayout,
    OpenPrOnGitHub, ToggleJump, ToggleRead, ToggleSettings, ToggleShortcuts, ToggleWrapLines,
};
use review_session::TourWork;
use settings_window::SettingsWindow;
use switcher_window::SwitcherWindow;
use writing_window::WritingWindow;

/// What the app's loop takes in: requests from outside, and the app's own asks of itself.
enum Asked {
    Request(Request, std::sync::mpsc::Sender<Answer>),
    ReopenShown,
}

/// The one running app. It blocks until a quit request comes or the window closes with no build left to
/// wait for (`AppWindow::closed`). The window opens
/// when the first `open` request has built its review, or earlier holding only Settings when they are
/// asked for.
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
            // A picture is taken once the window holds still, which an endless animation never does.
            #[cfg(feature = "snapshots")]
            if snapshots::is_on() {
                cx.set_reduce_motion(true);
            }
            let remembered = AppStateFile::in_home(&home);
            let active = cx.new(|_| ActivePr::remembering_in(remembered));
            let switching = pr_switching(&requests_to_self, &home, active.clone(), cx);
            switching.refresh(cx);
            cx.set_global(switching);
            tour_writer::resume(cx);
            if takes_focus() {
                dock_icon::show();
            } else {
                dock_icon::stay_in_background();
            }
            quit_on_cmd_q(cx);
            components::toast_stack::Toaster::install(cx);
            let configured = keymap::configured_keymap();
            if let Some(notice) = configured.notice() {
                components::toast_stack::post_toast(cx, ToastTone::Danger, notice);
            }
            apply_keymap(configured.keymap, cx);
            avatar_pictures::AvatarPictures::install(cx);
            prose_selection::ProseSelections::install(cx);
            updater::Updates::install(&home, cx);
            word_keys::take_before_macos(cx);
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
        relevant: relevant_pull_requests(),
        home: home.to_path_buf(),
        active,
        agents: Arc::new(HcTourAgents::new()),
        list: cx
            .new(|_| PrSwitcher::remembering_in(RememberedPullRequestsFile::in_home(home), home)),
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

fn relevant_pull_requests() -> Arc<dyn RelevantPullRequestsPort> {
    #[cfg(feature = "snapshots")]
    if snapshots::is_on() {
        return Arc::new(snapshots::SamplePullRequests);
    }
    Arc::new(GitHub::from_env())
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
        at_step: None,
    }
}

fn refused(reason: &str) -> Answer {
    Answer::Refused {
        reason: reason.to_owned(),
    }
}

type OneWindow = AppWindow<
    WindowHandle<SettingsWindow>,
    WindowHandle<SwitcherWindow>,
    WindowHandle<WritingWindow>,
    WindowHandle<OneStepLayout>,
>;

/// What the app shows now. The window is there once the first review is built or Settings is asked for.
struct Screen {
    active: Entity<ActivePr>,
    window: OneWindow,
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
        window: AppWindow::Closed,
    }));
    let closed_screen = Rc::clone(&screen);
    cx.update(|cx| {
        cx.on_window_closed(move |cx, _| {
            let mut screen = closed_screen.borrow_mut();
            let active = screen.active.clone();
            if screen.window.closed(active.read(cx)) == AfterClose::EndApp {
                cx.quit();
            }
        })
        .detach();
    });
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
            Request::ShowSettings => {
                let answer = show_settings(&screen, &home, cx);
                reply.send(answer).ok();
            }
            Request::ShowSwitcher => {
                let answer = show_switcher(&screen, cx);
                reply.send(answer).ok();
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
                let tour = TourFact::of(&pr, &home);
                let outcome = active.update(cx, |active, cx| {
                    let outcome = active.open(pr.clone(), origin, tour);
                    cx.notify();
                    outcome
                });
                let decision = match outcome {
                    OpenOutcome::Tour(decision) => decision,
                    OpenOutcome::Writing(decision) => {
                        show_writing(&screen, &active, cx);
                        if decision == WriteDecision::StartWriter {
                            cx.update(|cx| tour_writer::start_writer(pr, cx));
                        }
                        reply.send(Answer::Writing { pr: open.pr }).ok();
                        continue;
                    }
                };
                match decision {
                    OpenDecision::KeepInBackground => {
                        reply.send(Answer::KeptInBackground { pr: open.pr }).ok();
                    }
                    OpenDecision::Reload => {
                        reply.send(Answer::Reloaded { pr: open.pr }).ok();
                        let window = screen.borrow().window.review();
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

/// Settings over the PR on screen, or with no PR open, in the one window holding only Settings.
fn show_settings(screen: &SharedScreen, home: &Path, cx: &mut AsyncApp) -> Answer {
    let window = screen.borrow().window;
    let shown = cx.update(|cx| {
        bring_app_to_front(cx);
        match window.show_settings() {
            ShowSettings::OverReview(window) => window
                .update(cx, |layout, window, cx| {
                    layout.show_settings(window, cx);
                    raise(window);
                })
                .map(|()| AppWindow::Review(window)),
            ShowSettings::Raise(window) => window
                .update(cx, |_, window, _| raise(window))
                .map(|()| AppWindow::SettingsOnly(window)),
            ShowSettings::OpenSettingsOnly => {
                open_settings_window(home, cx).map(AppWindow::SettingsOnly)
            }
            ShowSettings::ReplaceSwitcher(window) => {
                let home = home.to_path_buf();
                window
                    .update(cx, |_, window, cx| {
                        window.set_window_title(&marked_if_dev("Settings".to_owned()));
                        window.replace_root(cx, |window, cx| SettingsWindow::new(home, window, cx));
                    })
                    .map(|()| AppWindow::SettingsOnly(WindowHandle::new(window.window_id())))
            }
            ShowSettings::NotWhileWriting => Err(anyhow::anyhow!(
                "a tour is being written; Settings opens once it is shown"
            )),
        }
    });
    match shown {
        Ok(window) => {
            screen.borrow_mut().window = window;
            Answer::ShowingSettings
        }
        Err(error) => refused(&format!("could not show Settings: {error:#}")),
    }
}

/// The PR switcher over the PR on screen, or with no PR open, in the one window holding only the
/// switcher. Settings being edited with no PR open come to the front instead, so nothing typed is lost.
fn show_switcher(screen: &SharedScreen, cx: &mut AsyncApp) -> Answer {
    let window = screen.borrow().window;
    let shown = cx.update(|cx| {
        bring_app_to_front(cx);
        match window.show_switcher() {
            ShowSwitcher::OverReview(window) => window
                .update(cx, |layout, window, cx| {
                    layout.show_pr_switcher(window, cx);
                    raise(window);
                })
                .map(|()| AppWindow::Review(window)),
            ShowSwitcher::Raise(window) => window
                .update(cx, |_, window, _| raise(window))
                .map(|()| AppWindow::SwitcherOnly(window)),
            ShowSwitcher::RaiseSettings(window) => window
                .update(cx, |_, window, _| raise(window))
                .map(|()| AppWindow::SettingsOnly(window)),
            ShowSwitcher::RaiseWriting(window) => window
                .update(cx, |_, window, _| raise(window))
                .map(|()| AppWindow::WritingOnly(window)),
            ShowSwitcher::OpenSwitcherOnly => open_switcher_window(cx).map(AppWindow::SwitcherOnly),
        }
    });
    match shown {
        Ok(window) => {
            screen.borrow_mut().window = window;
            Answer::ShowingSwitcher
        }
        Err(error) => refused(&format!("could not show the PR switcher: {error:#}")),
    }
}

/// The save of the settings asks for the PR on screen to be built again. The window is held first (typing
/// saved, focus out of the boxes) so the build reads the state file the old session no longer changes;
/// `reopen` then raises the Opening veil and gives the ticket, and the build takes the path of any switch.
fn reopen_shown(screen: &SharedScreen, home: &Path, cx: &mut AsyncApp) {
    let (active, window) = {
        let screen = screen.borrow();
        (screen.active.clone(), screen.window.review())
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
        (screen.active.clone(), screen.window.review())
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
                raise(window);
            })
            .ok();
        bring_app_to_front(cx);
    });
}

/// The review is built off the UI thread while the PR on screen stays. When a build fails with no window
/// open, the app ends with the reason in its log; otherwise the reason is also shown in the window, over
/// Settings or the PR on screen, whichever it holds.
async fn build_and_show(
    screen: SharedScreen,
    ticket: BuildTicket,
    pr: PullRequestUrl,
    open: OpenRequest,
    home: PathBuf,
    cx: &mut AsyncApp,
) {
    let (build_pr, folder, at_step) = (pr.clone(), open.folder.clone(), open.at_step.clone());
    let built = cx
        .background_executor()
        .spawn(async move {
            open_review(&build_pr, &folder, &home).map(|mut opened| {
                if let Some(step) = &at_step {
                    opened.review.go_to(step);
                }
                opened
            })
        })
        .await;
    let active = screen.borrow().active.clone();
    let finished = active.update(cx, |active, cx| {
        let finished = active.finished(ticket, built);
        cx.notify();
        finished
    });
    let window = screen.borrow().window;
    match finished {
        Finished::Superseded => {}
        Finished::Stay { reason } => {
            eprintln!("{reason}");
            match window.build_failed() {
                FailedBuild::EndApp => std::process::exit(1),
                FailedBuild::ShowInSettings(window) => cx.update(|cx| {
                    window
                        .update(cx, |settings_window, _, cx| {
                            settings_window.build_failed(reason, cx);
                        })
                        .ok();
                }),
                FailedBuild::ShowInSwitcher(window) => cx.update(|cx| {
                    window
                        .update(cx, |switcher_window, _, cx| {
                            switcher_window.build_failed(reason, cx);
                        })
                        .ok();
                }),
                // `ActivePr::finished` put the failure on the loading screen, which observes it.
                FailedBuild::ShowInWriting(_) => {}
                FailedBuild::ShowInReview(window) => cx.update(|cx| {
                    window
                        .update(cx, |layout, _, cx| layout.switch_failed(reason, cx))
                        .ok();
                }),
            }
        }
        Finished::Switch(opened) => {
            let shown = cx.update(|cx| {
                let shown = match window.show_review() {
                    ShowReview::Switch(window) => {
                        switch_window(window, opened, &pr, cx).map(|()| window)
                    }
                    ShowReview::TurnIntoReview(window) => turn_into_review(window, opened, &pr, cx),
                    ShowReview::TurnSwitcherIntoReview(window) => {
                        turn_switcher_into_review(window, opened, &pr, cx)
                    }
                    ShowReview::TurnWritingIntoReview(window) => {
                        turn_writing_into_review(window, opened, &pr, cx)
                    }
                    ShowReview::OpenWindow => open_window(opened, &pr, cx),
                };
                bring_app_to_front(cx);
                shown
            });
            match shown {
                Ok(window) => {
                    screen.borrow_mut().window = AppWindow::Review(window);
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
        window.replace_root(cx, |window, cx| new_layout(opened, pr, None, window, cx));
    })
}

/// The window that held only Settings becomes the review's, with Settings still open over it.
fn turn_into_review(
    window: WindowHandle<SettingsWindow>,
    opened: OpenedReview,
    pr: &PullRequestUrl,
    cx: &mut App,
) -> anyhow::Result<WindowHandle<OneStepLayout>> {
    window.update(cx, |settings_window, window, cx| {
        let settings = settings_window.panel();
        window.replace_root(cx, |window, cx| {
            new_layout(opened, pr, Some(settings), window, cx)
        });
    })?;
    Ok(WindowHandle::new(window.window_id()))
}

/// The window shows the loading screen of the tour being written: the one over the PR on screen, or with
/// no PR open a window holding only that screen.
fn show_writing(screen: &SharedScreen, active: &Entity<ActivePr>, cx: &mut AsyncApp) {
    let window = screen.borrow().window;
    let shown = cx.update(|cx| {
        bring_app_to_front(cx);
        match window.show_writing() {
            ShowWriting::OpenWindow => {
                open_writing_window(active, cx).map(|window| Some(AppWindow::WritingOnly(window)))
            }
            ShowWriting::Raise(window) => window
                .update(cx, |_, window, _| raise(window))
                .map(|()| None),
            ShowWriting::OverReview(window) => window
                .update(cx, |_, window, _| raise(window))
                .map(|()| None),
            ShowWriting::LeaveSettings => Ok(None),
            ShowWriting::ReplaceSwitcher(window) => {
                replace_switcher_with_writing(window, active, cx)
                    .map(|window| Some(AppWindow::WritingOnly(window)))
            }
        }
    });
    match shown {
        Ok(Some(window)) => screen.borrow_mut().window = window,
        Ok(None) => {}
        Err(error) => eprintln!("could not show the tour being written: {error:#}"),
    }
}

/// The PR picked in the switcher has no tour yet: the switcher's window shows the loading screen of the
/// tour being written.
fn replace_switcher_with_writing(
    window: WindowHandle<SwitcherWindow>,
    active: &Entity<ActivePr>,
    cx: &mut App,
) -> anyhow::Result<WindowHandle<WritingWindow>> {
    window.update(cx, |_, window, cx| {
        window.set_window_title(&marked_if_dev("Writing a tour".to_owned()));
        window.replace_root(cx, |window, cx| WritingWindow::new(active, window, cx));
        raise(window);
    })?;
    Ok(WindowHandle::new(window.window_id()))
}

/// The window that held only the loading screen becomes the review's.
fn turn_writing_into_review(
    window: WindowHandle<WritingWindow>,
    opened: OpenedReview,
    pr: &PullRequestUrl,
    cx: &mut App,
) -> anyhow::Result<WindowHandle<OneStepLayout>> {
    window.update(cx, |_, window, cx| {
        window.replace_root(cx, |window, cx| new_layout(opened, pr, None, window, cx));
    })?;
    Ok(WindowHandle::new(window.window_id()))
}

actions!(app, [Quit]);

/// cmd+q ends the app from whichever window has the focus. The review's window answers it first,
/// warning once while work is in flight; this is for a window with no review. The menu item is what macOS shows the key beside.
fn quit_on_cmd_q(cx: &mut App) {
    cx.on_action(|_: &Quit, cx| cx.quit());
    cx.set_menus([Menu {
        name: "guided-review".into(),
        items: vec![MenuItem::action("Quit guided-review", Quit)],
        disabled: false,
    }]);
}

/// The window that held only the switcher becomes the picked PR's review.
fn turn_switcher_into_review(
    window: WindowHandle<SwitcherWindow>,
    opened: OpenedReview,
    pr: &PullRequestUrl,
    cx: &mut App,
) -> anyhow::Result<WindowHandle<OneStepLayout>> {
    window.update(cx, |_, window, cx| {
        window.replace_root(cx, |window, cx| new_layout(opened, pr, None, window, cx));
    })?;
    Ok(WindowHandle::new(window.window_id()))
}

/// Binds every key again, so keys saved in Settings work at once. The text boxes' own editing
/// keys are not the reviewer's to change.
pub fn apply_keymap(keymap: Keymap, cx: &mut App) {
    cx.clear_key_bindings();
    text_box::bind_keys(cx);
    bind_keys(&keymap, cx);
    keymap::set_keymap(keymap, cx);
}

/// Every shortcut comes from the keymap the shortcuts sheet lists.
fn bind_keys(keymap: &Keymap, cx: &mut App) {
    let typing_is_not_navigation = format!(
        "{} && !{} && !{} && !{} && !{} && !{} && !{} && !{} && !{}",
        OneStepLayout::KEY_CONTEXT,
        AskPanel::KEY_CONTEXT,
        JumpPalette::KEY_CONTEXT,
        PrSwitcherView::KEY_CONTEXT,
        SettingsPanel::KEY_CONTEXT,
        ShortcutsSheet::KEY_CONTEXT,
        PictureZoom::KEY_CONTEXT,
        CommentComposer::KEY_CONTEXT,
        text_box::TYPING_CONTEXT
    );
    let bindings = Command::ALL.into_iter().flat_map(|command| {
        let context = match command.scope() {
            KeyScope::Page => Some(typing_is_not_navigation.clone()),
            KeyScope::App => None,
            KeyScope::Window => Some(OneStepLayout::KEY_CONTEXT.to_owned()),
            KeyScope::TextBox => Some(text_box::TYPING_CONTEXT.to_owned()),
            KeyScope::NotTyping => Some(format!(
                "{} && !{}",
                OneStepLayout::KEY_CONTEXT,
                text_box::TYPING_CONTEXT
            )),
        };
        keymap.chords(command).iter().map(move |chord| {
            let keys = chord.keystroke();
            let context = context.as_deref();
            match command {
                Command::NextScreen => KeyBinding::new(&keys, GoForward, context),
                Command::PreviousScreen => KeyBinding::new(&keys, GoBack, context),
                Command::NextUnreadStep => KeyBinding::new(&keys, GoToNextUnread, context),
                Command::Jump => KeyBinding::new(&keys, ToggleJump, context),
                Command::ToggleRead => KeyBinding::new(&keys, ToggleRead, context),
                Command::CopyCode => KeyBinding::new(&keys, CopySelection, context),
                Command::AddToReview => KeyBinding::new(&keys, text_box::Submit, context),
                Command::AskAgent => KeyBinding::new(&keys, text_box::SubmitToAgent, context),
                Command::CloseBox => KeyBinding::new(&keys, text_box::Cancel, context),
                Command::WrapLines => KeyBinding::new(&keys, ToggleWrapLines, context),
                Command::Settings => KeyBinding::new(&keys, ToggleSettings, context),
                Command::Shortcuts => KeyBinding::new(&keys, ToggleShortcuts, context),
                Command::OpenOnGitHub => KeyBinding::new(&keys, OpenPrOnGitHub, context),
                Command::CloseWindow => KeyBinding::new(&keys, CloseWindow, context),
                Command::Quit => KeyBinding::new(&keys, Quit, context),
            }
        })
    });
    cx.bind_keys(bindings);
}

fn open_window(
    opened: OpenedReview,
    pr: &PullRequestUrl,
    cx: &mut App,
) -> anyhow::Result<WindowHandle<OneStepLayout>> {
    let options = window_options(window_title(&opened, pr), cx);
    cx.open_window(options, |window, cx| {
        cx.set_global(Appearance::of(window));
        cx.new(|cx| new_layout(opened, pr, None, window, cx))
    })
}

fn open_settings_window(home: &Path, cx: &mut App) -> anyhow::Result<WindowHandle<SettingsWindow>> {
    let options = window_options("Settings".to_owned(), cx);
    let home = home.to_path_buf();
    cx.open_window(options, |window, cx| {
        cx.set_global(Appearance::of(window));
        cx.new(|cx| SettingsWindow::new(home, window, cx))
    })
}

fn open_switcher_window(cx: &mut App) -> anyhow::Result<WindowHandle<SwitcherWindow>> {
    let options = window_options("Pick a pull request".to_owned(), cx);
    let switching = cx.global::<PrSwitching>().clone();
    cx.open_window(options, |window, cx| {
        cx.set_global(Appearance::of(window));
        cx.new(|cx| SwitcherWindow::new(switching, window, cx))
    })
}

fn open_writing_window(
    active: &Entity<ActivePr>,
    cx: &mut App,
) -> anyhow::Result<WindowHandle<WritingWindow>> {
    let options = window_options("Writing a tour".to_owned(), cx);
    let active = active.clone();
    cx.open_window(options, |window, cx| {
        cx.set_global(Appearance::of(window));
        cx.new(|cx| WritingWindow::new(&active, window, cx))
    })
}

/// A snapshot run draws behind the reviewer's work and never takes their focus.
#[cfg(feature = "snapshots")]
fn takes_focus() -> bool {
    !snapshots::is_on()
}

#[cfg(not(feature = "snapshots"))]
fn takes_focus() -> bool {
    true
}

fn bring_app_to_front(cx: &mut App) {
    if takes_focus() {
        cx.activate(true);
    }
}

fn raise(window: &mut Window) {
    if takes_focus() {
        window.activate_window();
    }
}

fn window_options(title: String, cx: &App) -> WindowOptions {
    let bounds = Bounds::centered(None, size(px(1200.), px(800.)), cx);
    WindowOptions {
        focus: takes_focus(),
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: Some(TitlebarOptions {
            title: Some(marked_if_dev(title).into()),
            appears_transparent: true,
            traffic_light_position: Some(point(px(14.), px(14.))),
        }),
        ..Default::default()
    }
}

fn window_title(opened: &OpenedReview, pr: &PullRequestUrl) -> String {
    format!("{pr}: {}", opened.review.title())
}

/// A debug build names itself in its title, which the Window menu and Mission Control show.
fn marked_if_dev(title: String) -> String {
    if cfg!(debug_assertions) {
        format!("{title} (dev)")
    } else {
        title
    }
}

fn new_layout(
    opened: OpenedReview,
    pr: &PullRequestUrl,
    carried_settings: Option<Entity<SettingsPanel>>,
    window: &mut Window,
    cx: &mut Context<'_, OneStepLayout>,
) -> OneStepLayout {
    window.set_window_title(&marked_if_dev(window_title(&opened, pr)));
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
    let keeps_settings_focused = carried_settings.is_some();
    let layout = OneStepLayout::new(
        opened.review,
        opened.source,
        opened.ask,
        collaborators,
        carried_settings,
        window,
        cx,
    );
    cx.observe_window_appearance(window, |_, window, cx| {
        cx.set_global(Appearance::of(window));
        cx.notify();
    })
    .detach();
    if !keeps_settings_focused {
        cx.focus_self(window);
    }
    layout
}
