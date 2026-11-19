//! Writing a tour for a PR picked in the switcher: start its tour agent, follow it while it writes, and
//! keep the active PR's writing up to date. It runs at app level, so it goes on when the window switches.

use std::ops::ControlFlow;
use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;
use futures::channel::mpsc::unbounded;
use gpui::{App, AsyncApp, Entity};

use super::components::pr_switcher::PrSwitching;
use crate::instructions::SummaryInstructions;
use crate::opened_subject::writer_folder;
use crate::review_subject::ReviewSubject;
use crate::viewer::{
    ActivePr, AgentStatus, TerminalId, TourAgentPort, TourAgents, WriteDecision, WriterHandOver,
};

const SCREEN_LINES: usize = 20;
const SCREEN_EVERY: Duration = Duration::from_secs(2);
/// How long a writer task waits for a busy tour agent before it is handed over again.
const HAND_OVER_RETRY: Duration = Duration::from_secs(3);

/// Shows the PR's writing on the loading screen, starting it when none is on.
pub fn write(pr: ReviewSubject, cx: &mut App) {
    let switching = cx.global::<PrSwitching>().clone();
    let decision = switching.active.update(cx, |active, cx| {
        let decision = active.write(pr.clone());
        cx.notify();
        decision
    });
    if decision == WriteDecision::StartWriter {
        start(&switching, pr, cx);
    }
}

/// Writes the guided tour of the PR on screen while the reviewer keeps reading its basic tour: no
/// loading screen. The task goes to the PR's tour agent once it is idle, and one is started when the PR
/// has none.
pub fn write_in_background(pr: ReviewSubject, cx: &mut App) {
    let switching = cx.global::<PrSwitching>().clone();
    let decision = switching.active.update(cx, |active, cx| {
        let decision = active.write_in_background(pr.clone());
        cx.notify();
        decision
    });
    if decision == WriteDecision::StartWriter {
        hand_over(&switching, pr, WriterTask::GuidedOfBasic, cx);
    }
}

/// What the PR's agent is asked to write.
#[derive(Clone, Copy)]
enum WriterTask {
    /// The tour of a PR that has none.
    Tour,
    /// The guided tour of a PR read as a basic tour.
    GuidedOfBasic,
}

/// Hands the writer task to the PR's agent, waiting while it is busy, and follows it once it has the task.
fn hand_over(switching: &PrSwitching, pr: ReviewSubject, task: WriterTask, cx: &mut App) {
    let active = switching.active.clone();
    let shown = active
        .read(cx)
        .shown()
        .cloned()
        .unwrap_or_else(|| pr.clone());
    let port = Arc::clone(&switching.agents);
    let agents = TourAgents::new(Arc::clone(&port))
        .finding_head_branches_with(Arc::clone(&switching.head_branches))
        .with_summary_skill(SummaryInstructions::skill_for_launch(&switching.folders))
        .starting_in(writer_folder(&pr, &switching.folders));
    cx.spawn(async move |cx| {
        loop {
            let (task_pr, task_shown, task_agents) = (pr.clone(), shown.clone(), agents.clone());
            let handed = cx
                .background_executor()
                .spawn(async move {
                    match task {
                        WriterTask::Tour => task_agents.write_tour(&task_pr, &task_shown),
                        WriterTask::GuidedOfBasic => {
                            task_agents.hand_writer_task(&task_pr, &task_shown)
                        }
                    }
                })
                .await;
            let terminal = match handed {
                Ok(WriterHandOver::Delivered(terminal) | WriterHandOver::Started(terminal)) => {
                    terminal
                }
                Ok(WriterHandOver::Waiting(_)) => {
                    let is_still_wanted = active.read_with(cx, |active, _| {
                        active.writing_of(&pr).is_some_and(|writing| {
                            writing.terminal.is_none() && !writing.has_failed()
                        })
                    });
                    if !is_still_wanted {
                        return;
                    }
                    cx.background_executor().timer(HAND_OVER_RETRY).await;
                    continue;
                }
                Err(error) => {
                    active.update(cx, |active, cx| {
                        active.writer_started(&pr, Err(error));
                        cx.notify();
                    });
                    return;
                }
            };
            active.update(cx, |active, cx| {
                active.writer_started(&pr, Ok(terminal.clone()));
                cx.notify();
            });
            follow(&active, port, &pr, terminal, cx).await;
            return;
        }
    })
    .detach();
}

/// Starts the writer of a PR whose writing `ActivePr` just began.
pub fn start_writer(pr: ReviewSubject, cx: &mut App) {
    let switching = cx.global::<PrSwitching>().clone();
    start(&switching, pr, cx);
}

/// After a restart, finds each remembered writing's terminal again and follows it. Asking herdr is
/// brief and happens once, before the window exists.
pub fn resume(cx: &mut App) {
    let switching = cx.global::<PrSwitching>().clone();
    let (active, port) = (switching.active.clone(), switching.agents);
    let to_follow = active.update(cx, |active, cx| {
        let to_follow = active.resume_writings(port.as_ref());
        cx.notify();
        to_follow
    });
    for (pr, terminal) in to_follow {
        let (active, port) = (active.clone(), Arc::clone(&port));
        cx.spawn(async move |cx| follow(&active, port, &pr, terminal, cx).await)
            .detach();
    }
}

pub fn try_again(pr: ReviewSubject, cx: &mut App) {
    let switching = cx.global::<PrSwitching>().clone();
    switching.active.update(cx, |active, cx| {
        active.try_again(&pr);
        cx.notify();
    });
    start(&switching, pr, cx);
}

/// Back to the PR shown behind the loading screen; the writing goes on.
pub fn stop_waiting(cx: &mut App) {
    let active = cx.global::<PrSwitching>().active.clone();
    active.update(cx, |active, cx| {
        active.stop_waiting();
        cx.notify();
    });
}

/// Brings the writer's terminal up in herdr. Slow (it runs `herdr-app`), so it runs off the UI thread.
pub fn show_agent(pr: ReviewSubject, cx: &mut App) {
    let switching = cx.global::<PrSwitching>();
    let agents = TourAgents::new(Arc::clone(&switching.agents))
        .finding_head_branches_with(Arc::clone(&switching.head_branches));
    cx.background_executor()
        .spawn(async move {
            if let Err(error) = agents.show(&pr) {
                eprintln!("could not show the tour agent of {pr}: {error:#}");
            }
        })
        .detach();
}

fn start(switching: &PrSwitching, pr: ReviewSubject, cx: &mut App) {
    hand_over(switching, pr, WriterTask::Tour, cx);
}

enum WriterEvent {
    Status(AgentStatus),
    /// Herdr's stream of the terminal ended: the terminal is gone.
    Gone,
    /// Time to read the screen again for the loading screen.
    Tick,
}

/// Follows the writer until its PR's writing is over or belongs to another writer (after Try again).
/// Dropping the event stream then ends the herdr follow and the ticker.
async fn follow(
    active: &Entity<ActivePr>,
    port: Arc<dyn TourAgentPort>,
    pr: &ReviewSubject,
    terminal: TerminalId,
    cx: &mut AsyncApp,
) {
    let (events, mut incoming) = unbounded();
    let (statuses, follow_port, followed) = (events.clone(), Arc::clone(&port), terminal.clone());
    cx.background_executor()
        .spawn(async move {
            let followed_to_end = follow_port.follow(&followed, &mut |status| {
                if statuses
                    .unbounded_send(WriterEvent::Status(status))
                    .is_err()
                {
                    ControlFlow::Break(())
                } else {
                    ControlFlow::Continue(())
                }
            });
            if let Err(error) = followed_to_end {
                eprintln!("could not follow the tour agent {followed}: {error:#}");
            }
            // Nobody listens any more when the writing ended first.
            statuses.unbounded_send(WriterEvent::Gone).ok();
        })
        .detach();
    cx.spawn(async move |cx| {
        loop {
            cx.background_executor().timer(SCREEN_EVERY).await;
            if events.unbounded_send(WriterEvent::Tick).is_err() {
                break;
            }
        }
    })
    .detach();
    while let Some(event) = incoming.next().await {
        let (is_on, is_watched) = active.read_with(cx, |active, _| {
            let is_on = active
                .writing_of(pr)
                .is_some_and(|writing| writing.terminal.as_ref() == Some(&terminal));
            let is_watched = active.loading().is_some_and(|(loading, _)| loading == pr);
            (is_on, is_watched)
        });
        if !is_on {
            return;
        }
        if matches!(event, WriterEvent::Tick) && !is_watched {
            continue;
        }
        let (read_port, read_terminal) = (Arc::clone(&port), terminal.clone());
        let lines = cx
            .background_executor()
            .spawn(async move { read_port.screen_lines(&read_terminal, SCREEN_LINES) })
            .await;
        let is_gone = matches!(event, WriterEvent::Gone);
        active.update(cx, |active, cx| {
            // A terminal that is gone has no screen to read; the last lines read stay.
            if let Ok(lines) = lines {
                active.writer_screen(pr, lines);
            }
            match event {
                WriterEvent::Status(status) => active.writer_status(pr, status),
                WriterEvent::Gone => active.writer_gone(pr),
                WriterEvent::Tick => {}
            }
            cx.notify();
        });
        if is_gone {
            return;
        }
    }
}
