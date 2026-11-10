//! Writing a tour for a PR picked in the switcher: start its tour agent, follow it while it writes, and
//! keep the active PR's writing up to date. It runs at app level, so it goes on when the window switches.

use std::ops::ControlFlow;
use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;
use futures::channel::mpsc::unbounded;
use gpui::{App, AsyncApp, Entity};

use super::components::pr_switcher::PrSwitching;
use crate::config::Config;
use crate::open_review::writer_folder;
use crate::pull_request::PullRequestUrl;
use crate::viewer::{ActivePr, AgentStatus, TerminalId, TourAgentPort, TourAgents, WriteDecision};

const SCREEN_LINES: usize = 20;
const SCREEN_EVERY: Duration = Duration::from_secs(2);

/// Shows the PR's writing on the loading screen, starting it when none is on.
pub fn write(pr: PullRequestUrl, cx: &mut App) {
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

pub fn try_again(pr: PullRequestUrl, cx: &mut App) {
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
pub fn show_agent(pr: PullRequestUrl, cx: &mut App) {
    let agents = TourAgents::new(Arc::clone(&cx.global::<PrSwitching>().agents));
    cx.background_executor()
        .spawn(async move {
            if let Err(error) = agents.show(&pr) {
                eprintln!("could not show the tour agent of {pr}: {error:#}");
            }
        })
        .detach();
}

fn start(switching: &PrSwitching, pr: PullRequestUrl, cx: &mut App) {
    let active = switching.active.clone();
    let shown = active
        .read(cx)
        .shown()
        .cloned()
        .unwrap_or_else(|| pr.clone());
    let (port, home) = (Arc::clone(&switching.agents), switching.home.clone());
    cx.spawn(async move |cx| {
        let (writer_pr, writer_port) = (pr.clone(), Arc::clone(&port));
        let started = cx
            .background_executor()
            .spawn(async move {
                let summary_skill = match Config::load_from(&home) {
                    Ok(config) => config
                        .summary_instructions(&home)
                        .skill()
                        .map(str::to_owned),
                    Err(error) => {
                        eprintln!("warning: summary instructions not read: {error:#}");
                        None
                    }
                };
                TourAgents::new(writer_port)
                    .with_summary_skill(summary_skill)
                    .starting_in(writer_folder(&writer_pr, &home))
                    .write_tour(&writer_pr, &shown)
            })
            .await;
        let terminal = started.as_ref().ok().cloned();
        active.update(cx, |active, cx| {
            active.writer_started(&pr, started);
            cx.notify();
        });
        if let Some(terminal) = terminal {
            follow(&active, port, &pr, terminal, cx).await;
        }
    })
    .detach();
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
    pr: &PullRequestUrl,
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
