//! The GPUI app. Shared components know nothing of the layout; a layout assembles them
//! around a `viewer::Review`, so a new layout later is one new file in `layouts`.

mod active_session;
pub mod assets;
mod avatar_pictures;
mod code_links;
mod code_selection;
pub mod components;
mod frame_stats;
pub mod keymap;
mod language_servers;
mod layouts;
mod page_rows;
mod prose_selection;
mod review_agent;
mod review_session;
mod settings_window;
mod sidebar_rows;
#[cfg(feature = "snapshots")]
mod snapshots;
mod switcher_window;
mod tour_writer;
mod updater;
mod writing_window;

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use gpui::{
    AnyWindowHandle, App, AppContext, AsyncApp, Bounds, Context, Entity, KeyBinding, Window,
    WindowBounds, WindowHandle, WindowOptions, actions, px, size,
};
use gpui_platform::application;

use crate::app_folders::AppFolders;
use crate::app_icon::AppIcon;
use crate::app_link::{Answer, AppListener, BUILD_WAIT, OpenRequest, Request};
use crate::findings::FindingId;
use crate::github::GitHub;
use crate::hc_tour_agent::HcTourAgents;
use crate::local_reviews_on_disk::clean_snapshot_refs;
use crate::open_review::{OpenedReview, open_review};
use crate::platform::{self, Os};
use crate::review_subject::ReviewSubject;
use crate::review_worktree::GitReviewWorktrees;
use crate::tour::{Tour, ToursOnDisk};
use crate::update::running_build;
use crate::viewer::{
    ActivePr, AfterClose, AgentReplied, AppStateFile, AppWindow, BuildTicket, Command, FailedBuild,
    Finished, HandOffId, KeyScope, Keymap, OpenDecision, OpenOrigin, OpenOutcome, PrSwitcher,
    PullsOfBranchPort, Relaunched, RelevantPullRequestsPort, RememberedPullRequestsFile,
    ReviewStateFile, ShowBuilding, ShowReview, ShowSettings, ShowSwitcher, ShowWriting, ToastTone,
    TourAgentPort, TourAgents, WriteDecision,
};
use crate::viewer::{FixReply, WhyReply};
use components::ask_panel::AskPanel;
use components::comment_composer::CommentComposer;
use components::comments_sheet::CommentsSheet;
use components::findings_sheet::FindingsSheet;
use components::jump_palette::JumpPalette;
use components::palette::Appearance;
use components::picture_zoom::PictureZoom;
use components::pr_switcher::{PrSwitcherView, PrSwitching};
use components::settings_panel::SettingsPanel;
use components::shortcuts_sheet::ShortcutsSheet;
use components::text_box;
use layouts::one_step::{
    AskAboutStep, AskToOpenPr, CloseWindow, Collaborators, CopySelection, FocusNextChange,
    FocusPreviousChange, GoBack, GoForward, GoToNextUnread, OneStepLayout, OpenIssue,
    OpenPrOnGitHub, ReadDown, ReadUp, ShowMoreAbove, ShowMoreBelow, ToggleAutoMerge,
    ToggleCommentsList, ToggleDraft, ToggleFindingsList, ToggleFold, ToggleJump, TogglePieceLabels,
    TogglePrSwitcher, ToggleRead, ToggleRenderedView, ToggleSettings, ToggleShortcuts,
    ToggleWhitespace, ToggleWrapLines,
};
use review_session::TourWork;
use settings_window::SettingsWindow;
use switcher_window::SwitcherWindow;
use writing_window::WritingWindow;

/// What the app's loop takes in: requests from outside, and the app's own asks of itself.
enum Asked {
    Request(Request, std::sync::mpsc::Sender<Answer>),
    ReopenShown,
    /// The OS launched the running app again, as a click on its Dock icon with no window in sight.
    Relaunched,
    /// Drop review deleted the review's files; the window shows the switcher in its place.
    Dropped(ReviewSubject),
}

/// The one running app. It blocks until a quit request comes or the window closes with no build left to
/// wait for (`AppWindow::closed`). The window opens
/// when the first `open` request has built its review, or earlier holding only Settings when they are
/// asked for.
pub fn serve(listener: AppListener, folders: AppFolders) {
    let (requests, asked) = unbounded::<Asked>();
    let requests_to_self = requests.clone();
    listener.serve(move |request| {
        let (reply, answer) = std::sync::mpsc::channel();
        let waited_for = match &request {
            Request::Open(open) if open.waits_for_build => Some(open.subject.clone()),
            _ => None,
        };
        if requests
            .unbounded_send(Asked::Request(request, reply))
            .is_err()
        {
            return refused("the app is closing");
        }
        match waited_for {
            Some(pr) => answer
                .recv_timeout(BUILD_WAIT)
                .unwrap_or(Answer::StillOpening { pr }),
            None => answer
                .recv_timeout(Duration::from_secs(10))
                .unwrap_or_else(|_| refused("the app did not answer in time")),
        }
    });
    let app = application().with_assets(assets::Assets);
    let relaunches = requests_to_self.clone();
    app.on_reopen(move |_| {
        if relaunches.unbounded_send(Asked::Relaunched).is_err() {
            eprintln!("could not bring the window back: the app is closing");
        }
    });
    app.run(move |cx: &mut App| {
        // A picture is taken once the window holds still, which an endless animation never does.
        #[cfg(feature = "snapshots")]
        if snapshots::is_on() {
            cx.set_reduce_motion(true);
        }
        let remembered = AppStateFile::in_folders(&folders);
        let active = cx.new(|_| ActivePr::remembering_in(remembered));
        let switching = pr_switching(&requests_to_self, &folders, active.clone(), cx);
        switching.refresh(cx);
        let review_agents = review_agent::agents_of(&switching);
        cx.set_global(switching);
        tour_writer::resume(cx);
        let cleaned = folders.clone();
        cx.background_executor()
            .spawn(async move {
                // Nobody looks at the start's clean-up; a ref it keeps only waits for the next start.
                for not_cleaned in clean_snapshot_refs(&cleaned) {
                    eprintln!("{not_cleaned}");
                }
                // A review agent outlives the app, and its checkout with it. Without herdr's answer
                // which ones run, nothing is swept.
                match review_agents.running_subjects() {
                    Ok(running) => {
                        for not_swept in
                            GitReviewWorktrees::under(cleaned.review_worktrees()).sweep(&running)
                        {
                            eprintln!("{not_swept}");
                        }
                    }
                    Err(error) => {
                        eprintln!("kept the review worktrees, herdr did not say which agents run: {error:#}");
                    }
                }
            })
            .detach();
        let focus = if takes_focus() {
            platform::Focus::Taken
        } else {
            platform::Focus::LeftAlone
        };
        let appearance = Appearance::of_system(cx);
        cx.set_global(appearance);
        platform::start(focus, AppIcon::of(running_build(), appearance.look()), Quit, cx);
        cx.on_action(|_: &Quit, cx| cx.quit());
        components::toast_stack::Toaster::install(cx);
        let configured = keymap::configured_keymap();
        if let Some(notice) = configured.notice() {
            components::toast_stack::post_toast(cx, ToastTone::Danger, notice);
        }
        apply_keymap(configured.keymap, cx);
        avatar_pictures::AvatarPictures::install(cx);
        prose_selection::ProseSelections::install(cx);
        updater::Updates::install(folders.home(), cx);
        cx.spawn(async move |cx| answer_requests(asked, active, folders, cx).await)
            .detach();
    });
}

/// Picking a PR in the switcher is an open request the app makes of itself, so it takes the same path
/// as one from the command line. No one waits for its answer; a failed build shows in the window.
fn pr_switching(
    requests: &UnboundedSender<Asked>,
    folders: &AppFolders,
    active: Entity<ActivePr>,
    cx: &mut App,
) -> PrSwitching {
    let requests = requests.clone();
    let reopen_requests = requests.clone();
    let drop_requests = requests.clone();
    let all_folders = folders.clone();
    PrSwitching {
        relevant: relevant_pull_requests(),
        folders: folders.clone(),
        active,
        agents: switcher_agents(folders),
        head_branches: head_branches(),
        list: cx.new(|_| {
            PrSwitcher::remembering_in(RememberedPullRequestsFile::in_folders(folders), folders)
        }),
        show: Arc::new(move |pr| {
            let (reply, _) = std::sync::mpsc::channel();
            let open = open_request_of(&all_folders, &pr);
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
        dropped: Arc::new(move |subject| {
            if drop_requests
                .unbounded_send(Asked::Dropped(subject))
                .is_err()
            {
                eprintln!("could not close the dropped review: the app is closing");
            }
        }),
    }
}

fn switcher_agents(folders: &AppFolders) -> Arc<dyn TourAgentPort> {
    let herdr: Arc<dyn TourAgentPort> = Arc::new(HcTourAgents::new(Tour::tours_dir(folders)));
    #[cfg(feature = "snapshots")]
    if snapshots::is_on() {
        return Arc::new(snapshots::NoReviewerTerminals(herdr));
    }
    herdr
}

fn head_branches() -> Arc<dyn PullsOfBranchPort> {
    #[cfg(feature = "snapshots")]
    if snapshots::is_on() {
        return Arc::new(crate::viewer::NoHost::default());
    }
    Arc::new(GitHub::from_env())
}

fn relevant_pull_requests() -> Arc<dyn RelevantPullRequestsPort> {
    #[cfg(feature = "snapshots")]
    if snapshots::is_on() {
        return Arc::new(snapshots::SamplePullRequests);
    }
    Arc::new(GitHub::from_env())
}

/// The open of a review the app makes of itself, from the folder a PR's clone is searched from: the
/// one its state file remembers, which need not be a clone, else home. A local change set opens from
/// its own folder. An unreadable state file only loses the remembered folder.
fn open_request_of(folders: &AppFolders, subject: &ReviewSubject) -> OpenRequest {
    let folder = subject.own_folder().map_or_else(
        || {
            ReviewStateFile::for_subject(folders, subject)
                .opened_from()
                .ok()
                .flatten()
                .unwrap_or_else(|| folders.home().to_path_buf())
        },
        Path::to_path_buf,
    );
    OpenRequest {
        subject: subject.to_string(),
        folder,
        terminal: None,
        at_step: None,
        waits_for_build: false,
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

/// The asker (a CLI call) may have given up and gone before the app answers; there is nobody left to tell.
fn answer_asker(reply: &std::sync::mpsc::Sender<Answer>, answer: Answer) {
    if reply.send(answer).is_err() {
        eprintln!("the asker went away before the answer was ready");
    }
}

async fn answer_requests(
    mut asked: UnboundedReceiver<Asked>,
    active: Entity<ActivePr>,
    folders: AppFolders,
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
                reopen_shown(&screen, &folders, cx);
                continue;
            }
            Asked::Relaunched => {
                bring_window_back(&screen, cx);
                continue;
            }
            Asked::Dropped(subject) => {
                close_dropped(&screen, &subject, cx);
                continue;
            }
        };
        match request {
            Request::Quit => {
                answer_asker(&reply, Answer::Quitting);
                cx.update(|cx| cx.quit());
            }
            Request::ShowSettings => {
                let answer = show_settings(&screen, &folders, cx);
                answer_asker(&reply, answer);
            }
            Request::ShowSwitcher => {
                let answer = show_switcher(&screen, cx);
                answer_asker(&reply, answer);
            }
            Request::AgentReplied {
                pr,
                hand_off,
                replied,
            } => {
                let answer = mark_replied(&screen, pr, hand_off, &replied, cx);
                answer_asker(&reply, answer);
            }
            Request::FixReplied {
                pr,
                finding,
                reply: said,
            } => {
                let answer = take_fix_reply(&screen, pr, &finding, &said, cx);
                answer_asker(&reply, answer);
            }
            Request::WhyReplied {
                pr,
                finding,
                reply: said,
            } => {
                let answer = take_why_reply(&screen, pr, &finding, &said, cx);
                answer_asker(&reply, answer);
            }
            Request::FindingsDone { pr } => {
                let answer = read_findings(&screen, pr, cx).await;
                answer_asker(&reply, answer);
            }
            Request::Open(open) => {
                let pr = match open.subject.parse::<ReviewSubject>() {
                    Ok(subject) => subject,
                    Err(error) => {
                        eprintln!("refused to open {}: {error:#}", open.subject);
                        answer_asker(
                            &reply,
                            refused(&ReviewSubject::not_a_subject(&open.subject)),
                        );
                        continue;
                    }
                };
                claim_terminal(&pr, &open, &folders, cx);
                let origin = origin_of(&open, &folders, cx).await;
                let active = screen.borrow().active.clone();
                let on_disk = ToursOnDisk::of(&folders, &pr);
                let outcome = active.update(cx, |active, cx| {
                    let outcome = active.open(pr.clone(), origin, on_disk);
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
                        answer_asker(&reply, Answer::Writing { pr: open.subject });
                        continue;
                    }
                };
                match decision {
                    OpenDecision::KeepInBackground => {
                        answer_asker(&reply, Answer::KeptInBackground { pr: open.subject });
                    }
                    OpenDecision::GuidedReady => {
                        answer_asker(&reply, Answer::GuidedReady { pr: open.subject });
                    }
                    OpenDecision::Reload => {
                        answer_asker(&reply, Answer::Reloaded { pr: open.subject });
                        let window = screen.borrow().window.review();
                        if let Some(window) = window {
                            show_again(window, cx);
                        }
                    }
                    OpenDecision::Build(ticket) => {
                        show_building(&screen, cx);
                        // An open that waits is answered when the build ends; any other at once.
                        let waiting = if open.waits_for_build {
                            Some(reply)
                        } else {
                            reply
                                .send(Answer::Opening {
                                    pr: open.subject.clone(),
                                })
                                .ok();
                            None
                        };
                        let (screen, folders) = (Rc::clone(&screen), folders.clone());
                        cx.spawn(async move |cx| {
                            let subject = open.subject.clone();
                            let answer =
                                build_and_show(screen, ticket, pr, open, folders, cx).await;
                            if let Some(waiting) = waiting {
                                waiting.send(answer.into_answer(subject)).ok();
                            }
                        })
                        .detach();
                    }
                }
            }
        }
    }
}

fn bring_window_back(screen: &SharedScreen, cx: &mut AsyncApp) {
    let window = screen.borrow().window;
    let brought_back = cx.update(|cx| {
        bring_app_to_front(cx);
        match window.relaunched() {
            Relaunched::OpenSwitcherOnly => open_switcher_window(cx).map(AppWindow::SwitcherOnly),
            Relaunched::Raise(window) => any_handle(window)
                .map_or(Ok(()), |handle| {
                    handle.update(cx, |_, window, _| raise(window))
                })
                .map(|()| window),
        }
    });
    match brought_back {
        Ok(window) => screen.borrow_mut().window = window,
        Err(error) => eprintln!("could not bring the window back: {error:#}"),
    }
}

fn show_building(screen: &SharedScreen, cx: &mut AsyncApp) {
    let window = screen.borrow().window;
    let shown = cx.update(|cx| {
        bring_app_to_front(cx);
        match window.show_building() {
            ShowBuilding::OpenSwitcherOnly => open_switcher_window(cx).map(AppWindow::SwitcherOnly),
            ShowBuilding::Raise(window) => any_handle(window)
                .map_or(Ok(()), |handle| {
                    handle.update(cx, |_, window, _| raise(window))
                })
                .map(|()| window),
        }
    });
    match shown {
        Ok(window) => screen.borrow_mut().window = window,
        Err(error) => eprintln!("could not show the PR being opened: {error:#}"),
    }
}

fn any_handle(window: OneWindow) -> Option<AnyWindowHandle> {
    match window {
        AppWindow::Closed => None,
        AppWindow::SettingsOnly(window) => Some(window.into()),
        AppWindow::SwitcherOnly(window) => Some(window.into()),
        AppWindow::WritingOnly(window) => Some(window.into()),
        AppWindow::Review(window) => Some(window.into()),
    }
}

/// Settings over the PR on screen, or with no PR open, in the one window holding only Settings.
fn show_settings(screen: &SharedScreen, folders: &AppFolders, cx: &mut AsyncApp) -> Answer {
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
                open_settings_window(folders, cx).map(AppWindow::SettingsOnly)
            }
            ShowSettings::ReplaceSwitcher(window) => {
                let folders = folders.clone();
                window
                    .update(cx, |_, window, cx| {
                        window.set_window_title(&marked_with_build("Settings".to_owned()));
                        window.replace_root(cx, |window, cx| {
                            SettingsWindow::new(folders, window, cx)
                        });
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
fn reopen_shown(screen: &SharedScreen, folders: &AppFolders, cx: &mut AsyncApp) {
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
    let open = open_request_of(folders, &pr);
    let (screen, folders) = (Rc::clone(screen), folders.clone());
    cx.spawn(async move |cx| {
        build_and_show(screen, ticket, pr, open, folders, cx).await;
    })
    .detach();
}

/// Only the PR in the window is marked here, because its session holds the state it writes back. For
/// any other PR the command marks the state file, and a build of it under way gets the reply again.
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
    let is_shown = active.read_with(cx, |active, _| active.shows_key(&pr));
    let window = window.filter(|_| is_shown);
    let Some(window) = window else {
        if let Ok(replied_pr) = pr.parse::<ReviewSubject>() {
            active.update(cx, |active, cx| {
                active.agent_replied(&replied_pr);
                active.hold_reply_while_building(&replied_pr, hand_off, replied);
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
            if let Ok(replied_pr) = pr.parse::<ReviewSubject>() {
                active.update(cx, |active, _| {
                    active.hold_reply_while_building(&replied_pr, hand_off, replied);
                });
            }
            Answer::HandOffAnswered { pr }
        }
        Err(error) => refused(&format!("{error:#}")),
    }
}

/// Only the review in the window holds the fix requests in memory; any other has the answer written
/// into its state file by the command.
fn take_fix_reply(
    screen: &SharedScreen,
    pr: String,
    finding: &FindingId,
    reply: &FixReply,
    cx: &mut AsyncApp,
) -> Answer {
    let (active, window) = {
        let screen = screen.borrow();
        (screen.active.clone(), screen.window.review())
    };
    let is_shown = active.read_with(cx, |active, _| active.shows_key(&pr));
    let Some(window) = window.filter(|_| is_shown) else {
        return Answer::NotShown { pr };
    };
    let taken = cx.update(|cx| {
        window
            .update(cx, |layout, _, cx| layout.fix_replied(finding, reply, cx))
            .and_then(|taken| taken)
    });
    match taken {
        Ok(()) => Answer::FixAnswered { pr },
        Err(error) => refused(&format!("{error:#}")),
    }
}

/// Only the review in the window holds the finding threads in memory; any other has the answer written
/// into its state file by the command.
fn take_why_reply(
    screen: &SharedScreen,
    pr: String,
    finding: &FindingId,
    reply: &WhyReply,
    cx: &mut AsyncApp,
) -> Answer {
    let (active, window) = {
        let screen = screen.borrow();
        (screen.active.clone(), screen.window.review())
    };
    let is_shown = active.read_with(cx, |active, _| active.shows_key(&pr));
    let Some(window) = window.filter(|_| is_shown) else {
        return Answer::NotShown { pr };
    };
    let taken = cx.update(|cx| {
        window
            .update(cx, |layout, _, cx| layout.why_replied(finding, reply, cx))
            .and_then(|taken| taken)
    });
    match taken {
        Ok(()) => Answer::WhyAnswered { pr },
        Err(error) => refused(&format!("{error:#}")),
    }
}

/// Only the review in the window holds findings to read again; any other reads its file when it opens.
async fn read_findings(screen: &SharedScreen, pr: String, cx: &mut AsyncApp) -> Answer {
    let (active, window) = {
        let screen = screen.borrow();
        (screen.active.clone(), screen.window.review())
    };
    let is_shown = active.read_with(cx, |active, _| active.shows_key(&pr));
    let Some(window) = window.filter(|_| is_shown) else {
        return Answer::NotShown { pr };
    };
    let catch_up = cx.update(|cx| window.update(cx, |layout, _, cx| layout.findings_catch_up(cx)));
    if let Ok(catch_up) = catch_up
        && let Err(error) = cx
            .background_executor()
            .spawn(async move { catch_up() })
            .await
    {
        eprintln!("could not follow the findings to the review's commits: {error:#}");
    }
    let count = cx.update(|cx| window.update(cx, |layout, _, cx| layout.findings_done(cx)));
    match count {
        Ok(count) => Answer::FindingsRead { pr, count },
        Err(error) => refused(&format!("{error:#}")),
    }
}

/// The terminal an `open` came from becomes the PR's tour agent, whatever the request then does. Slow
/// (it runs `hc`), so it runs off the UI thread; a failure is for the app's log, as the window may stay.
fn claim_terminal(pr: &ReviewSubject, open: &OpenRequest, folders: &AppFolders, cx: &mut AsyncApp) {
    let (pr, terminal) = (pr.clone(), open.terminal.clone());
    let tours = Tour::tours_dir(folders);
    cx.background_executor()
        .spawn(async move {
            let agents = TourAgents::new(Arc::new(HcTourAgents::new(tours)));
            if let Err(error) = agents.claim_for_open(&pr, terminal.as_deref()) {
                eprintln!("could not claim the terminal for {pr}: {error:#}");
            }
        })
        .detach();
}

/// Asks herdr whether the open came from a tour agent the app started; off the UI thread, as it runs `hc`.
async fn origin_of(open: &OpenRequest, folders: &AppFolders, cx: &mut AsyncApp) -> OpenOrigin {
    let Some(terminal) = open.terminal.clone() else {
        return OpenOrigin::Reviewer;
    };
    let tours = Tour::tours_dir(folders);
    cx.background_executor()
        .spawn(async move {
            TourAgents::new(Arc::new(HcTourAgents::new(tours))).origin_of(Some(&terminal))
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

/// The review is built off the UI thread while the PR on screen stays. The reason a build fails is also
/// shown in the window, over Settings or the PR on screen, whichever it holds, and in a switcher the app
/// opens when it has no window.
async fn build_and_show(
    screen: SharedScreen,
    ticket: BuildTicket,
    pr: ReviewSubject,
    open: OpenRequest,
    folders: AppFolders,
    cx: &mut AsyncApp,
) -> BuildEnd {
    let (build_pr, folder, at_step) = (pr.clone(), open.folder.clone(), open.at_step.clone());
    let disk_folders = folders.clone();
    let mut built = cx
        .background_executor()
        .spawn(async move {
            open_review(&build_pr, &folder, &folders).map(|mut opened| {
                if let Some(step) = &at_step {
                    opened.review.go_to(step);
                }
                opened
            })
        })
        .await;
    let active = screen.borrow().active.clone();
    if let Ok(opened) = &mut built {
        let on_disk = ToursOnDisk::of(&disk_folders, &pr);
        let progress =
            active.read_with(cx, |active, _| active.guided_tour_progress_of(&pr, on_disk));
        opened.review.guided_tour_is(progress);
    }
    let finished = active.update(cx, |active, cx| {
        let finished = active.finished(ticket, built);
        cx.notify();
        finished
    });
    let window = screen.borrow().window;
    match finished {
        Finished::Superseded => BuildEnd::Superseded,
        Finished::Stay { reason } => {
            eprintln!("{reason}");
            let failed = BuildEnd::Failed(reason.clone());
            match window.build_failed() {
                FailedBuild::OpenSwitcherWithReason => {
                    let opened = cx.update(|cx| {
                        let window = open_switcher_window(cx)?;
                        window.update(cx, |switcher_window, _, cx| {
                            switcher_window.build_failed(reason, cx);
                        })?;
                        Ok::<_, anyhow::Error>(window)
                    });
                    match opened {
                        Ok(window) => screen.borrow_mut().window = AppWindow::SwitcherOnly(window),
                        Err(error) => {
                            eprintln!("could not show the reason in a window: {error:#}");
                            std::process::exit(1);
                        }
                    }
                }
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
            failed
        }
        Finished::Switch(opened) => {
            let shown = cx.update(|cx| {
                let shown = match window.show_review() {
                    ShowReview::Switch(window) => {
                        switch_window(window, opened, cx).map(|()| window)
                    }
                    ShowReview::TurnIntoReview(window) => turn_into_review(window, opened, cx),
                    ShowReview::TurnSwitcherIntoReview(window) => {
                        turn_switcher_into_review(window, opened, cx)
                    }
                    ShowReview::TurnWritingIntoReview(window) => {
                        turn_writing_into_review(window, opened, cx)
                    }
                    ShowReview::OpenWindow => open_window(opened, cx),
                };
                bring_app_to_front(cx);
                shown
            });
            match shown {
                Ok(window) => {
                    screen.borrow_mut().window = AppWindow::Review(window);
                    apply_replies_held_for_reopen(&active, window, cx);
                    BuildEnd::Shown
                }
                Err(error) => {
                    eprintln!("could not show {pr}: {error:#}");
                    std::process::exit(1);
                }
            }
        }
    }
}

/// How a build ended, for the open that waited for it.
enum BuildEnd {
    Shown,
    Failed(String),
    Superseded,
}

impl BuildEnd {
    fn into_answer(self, subject: String) -> Answer {
        match self {
            Self::Shown => Answer::Opened { pr: subject },
            Self::Failed(reason) => refused(&reason),
            Self::Superseded => refused("another open took its place before this one was built"),
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
    cx: &mut App,
) -> anyhow::Result<()> {
    window.update(cx, |layout, window, cx| {
        layout.flush_typing(cx);
        window.replace_root(cx, |window, cx| new_layout(opened, None, window, cx));
    })
}

/// A dropped review's window holds only the switcher instead, for the reviewer to pick another review.
fn close_dropped(screen: &SharedScreen, subject: &ReviewSubject, cx: &mut AsyncApp) {
    let (active, window) = {
        let screen = screen.borrow();
        (screen.active.clone(), screen.window.review())
    };
    let was_shown = active.update(cx, |active, cx| {
        let was_shown = active.dropped(subject);
        cx.notify();
        was_shown
    });
    let Some(window) = window.filter(|_| was_shown) else {
        return;
    };
    match cx.update(|cx| replace_review_with_switcher(window, cx)) {
        Ok(switcher) => screen.borrow_mut().window = AppWindow::SwitcherOnly(switcher),
        Err(error) => eprintln!("could not close the dropped review {subject}: {error:#}"),
    }
}

fn replace_review_with_switcher(
    window: WindowHandle<OneStepLayout>,
    cx: &mut App,
) -> anyhow::Result<WindowHandle<SwitcherWindow>> {
    let switching = cx.global::<PrSwitching>().clone();
    window.update(cx, |_, window, cx| {
        window.set_window_title(&marked_with_build("Pick a pull request".to_owned()));
        window.replace_root(cx, |window, cx| SwitcherWindow::new(switching, window, cx));
    })?;
    Ok(WindowHandle::new(window.window_id()))
}

/// The window that held only Settings becomes the review's, with Settings still open over it.
fn turn_into_review(
    window: WindowHandle<SettingsWindow>,
    opened: OpenedReview,
    cx: &mut App,
) -> anyhow::Result<WindowHandle<OneStepLayout>> {
    window.update(cx, |settings_window, window, cx| {
        let settings = settings_window.panel();
        window.replace_root(cx, |window, cx| {
            new_layout(opened, Some(settings), window, cx)
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
        window.set_window_title(&marked_with_build("Writing a tour".to_owned()));
        window.replace_root(cx, |window, cx| WritingWindow::new(active, window, cx));
        raise(window);
    })?;
    Ok(WindowHandle::new(window.window_id()))
}

/// The window that held only the loading screen becomes the review's.
fn turn_writing_into_review(
    window: WindowHandle<WritingWindow>,
    opened: OpenedReview,
    cx: &mut App,
) -> anyhow::Result<WindowHandle<OneStepLayout>> {
    window.update(cx, |_, window, cx| {
        window.replace_root(cx, |window, cx| new_layout(opened, None, window, cx));
    })?;
    Ok(WindowHandle::new(window.window_id()))
}

// The Quit key ends the app from whichever window has the focus. The review's window answers it
// first, warning once while work is in flight; the app's own handler is for a window with no review.
actions!(app, [Quit]);

/// The window that held only the switcher becomes the picked PR's review.
fn turn_switcher_into_review(
    window: WindowHandle<SwitcherWindow>,
    opened: OpenedReview,
    cx: &mut App,
) -> anyhow::Result<WindowHandle<OneStepLayout>> {
    window.update(cx, |_, window, cx| {
        window.replace_root(cx, |window, cx| new_layout(opened, None, window, cx));
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
        "{} && !{} && !{} && !{} && !{} && !{} && !{} && !{} && !{} && !{} && !{}",
        OneStepLayout::KEY_CONTEXT,
        AskPanel::KEY_CONTEXT,
        JumpPalette::KEY_CONTEXT,
        PrSwitcherView::KEY_CONTEXT,
        SettingsPanel::KEY_CONTEXT,
        ShortcutsSheet::KEY_CONTEXT,
        FindingsSheet::KEY_CONTEXT,
        CommentsSheet::KEY_CONTEXT,
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
                Command::NextChange => KeyBinding::new(&keys, FocusNextChange, context),
                Command::PreviousChange => KeyBinding::new(&keys, FocusPreviousChange, context),
                Command::ReadDown => KeyBinding::new(&keys, ReadDown, context),
                Command::ReadUp => KeyBinding::new(&keys, ReadUp, context),
                Command::MoreAbove => KeyBinding::new(&keys, ShowMoreAbove, context),
                Command::MoreBelow => KeyBinding::new(&keys, ShowMoreBelow, context),
                Command::ToggleRenderedView => KeyBinding::new(&keys, ToggleRenderedView, context),
                Command::TogglePieceLabels => KeyBinding::new(&keys, TogglePieceLabels, context),
                Command::ToggleDraft => KeyBinding::new(&keys, ToggleDraft, context),
                Command::ToggleAutoMerge => KeyBinding::new(&keys, ToggleAutoMerge, context),
                Command::ToggleFold => KeyBinding::new(&keys, ToggleFold, context),
                Command::Jump => KeyBinding::new(&keys, ToggleJump, context),
                Command::ToggleRead => KeyBinding::new(&keys, ToggleRead, context),
                Command::CopyCode => KeyBinding::new(&keys, CopySelection, context),
                Command::AddToReview => KeyBinding::new(&keys, text_box::Submit, context),
                Command::AskAgent => KeyBinding::new(&keys, text_box::SubmitToAgent, context),
                Command::CommentNow => KeyBinding::new(&keys, text_box::SubmitNow, context),
                Command::AskAgentNow => KeyBinding::new(&keys, text_box::SubmitToAgentNow, context),
                Command::SuggestChange => KeyBinding::new(&keys, text_box::SuggestChange, context),
                Command::CloseBox => KeyBinding::new(&keys, text_box::Cancel, context),
                Command::WrapLines => KeyBinding::new(&keys, ToggleWrapLines, context),
                Command::HideWhitespace => KeyBinding::new(&keys, ToggleWhitespace, context),
                Command::AskAboutStep => KeyBinding::new(&keys, AskAboutStep, context),
                Command::Settings => KeyBinding::new(&keys, ToggleSettings, context),
                Command::Shortcuts => KeyBinding::new(&keys, ToggleShortcuts, context),
                Command::FindingsList => KeyBinding::new(&keys, ToggleFindingsList, context),
                Command::CommentsList => KeyBinding::new(&keys, ToggleCommentsList, context),
                Command::OpenOnGitHub => KeyBinding::new(&keys, OpenPrOnGitHub, context),
                Command::OpenIssue => KeyBinding::new(&keys, OpenIssue, context),
                Command::OpenPr => KeyBinding::new(&keys, AskToOpenPr, context),
                Command::SwitchPr => KeyBinding::new(&keys, TogglePrSwitcher, context),
                Command::CloseWindow => KeyBinding::new(&keys, CloseWindow, context),
                Command::Quit => KeyBinding::new(&keys, Quit, context),
            }
        })
    });
    cx.bind_keys(bindings);
}

fn open_window(opened: OpenedReview, cx: &mut App) -> anyhow::Result<WindowHandle<OneStepLayout>> {
    let options = window_options(window_title(&opened), cx);
    cx.open_window(options, |window, cx| {
        Appearance::follow(window, cx);
        cx.new(|cx| new_layout(opened, None, window, cx))
    })
}

fn open_settings_window(
    folders: &AppFolders,
    cx: &mut App,
) -> anyhow::Result<WindowHandle<SettingsWindow>> {
    let options = window_options("Settings".to_owned(), cx);
    let folders = folders.clone();
    cx.open_window(options, |window, cx| {
        Appearance::follow(window, cx);
        cx.new(|cx| SettingsWindow::new(folders, window, cx))
    })
}

fn open_switcher_window(cx: &mut App) -> anyhow::Result<WindowHandle<SwitcherWindow>> {
    let options = window_options("Pick a pull request".to_owned(), cx);
    let switching = cx.global::<PrSwitching>().clone();
    cx.open_window(options, |window, cx| {
        Appearance::follow(window, cx);
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
        Appearance::follow(window, cx);
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
        titlebar: Some(Os::RUNNING.titlebar(marked_with_build(title).into())),
        app_id: Some(platform::APP_ID.to_owned()),
        ..Default::default()
    }
}

fn window_title(opened: &OpenedReview) -> String {
    opened.review.window_title()
}

fn marked_with_build(title: String) -> String {
    running_build().marked_title(title)
}

fn new_layout(
    opened: OpenedReview,
    carried_settings: Option<Entity<SettingsPanel>>,
    window: &mut Window,
    cx: &mut Context<'_, OneStepLayout>,
) -> OneStepLayout {
    window.set_window_title(&marked_with_build(window_title(&opened)));
    let collaborators = Collaborators {
        host: opened.host,
        ide: opened.ide,
        tour_work: TourWork {
            repinner: Arc::new(opened.repinner),
            refresher: Arc::new(opened.refresher),
            reloader: Arc::new(opened.reloader),
            switcher: Arc::new(opened.switcher),
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
        Appearance::follow(window, cx);
        cx.notify();
    })
    .detach();
    if !keeps_settings_focused {
        cx.focus_self(window);
    }
    layout
}
