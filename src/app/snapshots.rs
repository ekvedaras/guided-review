//! Pictures of the app for looking at the UI without a screen grab. Built with the `snapshots`
//! feature and on when `GUIDED_REVIEW_SNAPSHOTS` names a folder: the layout stages its screens and
//! overlays one by one (`layouts::one_step::scenes`), each is saved there as a PNG, then the app quits.
//!
//! The window is drawn here on a timer, not by the display: a locked screen or a sleeping display
//! delivers no frames, so a run that waited for them would never draw.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Context as _;
use gpui::{AsyncWindowContext, Global, Modifiers, MouseMoveEvent, PlatformInput, point, px};

use crate::app::components::ask_panel::AskSetup;
use crate::app::components::palette::Appearance;
use crate::pull_request::PullRequestUrl;
use crate::viewer::{
    AgentLaunch, AgentLauncher, AgentPort, GitHubError, RelevantPullRequest, RelevantPullRequests,
    RelevantPullRequestsPort, TurnStopper, WhyListed,
};

const FOLDER_ENV: &str = "GUIDED_REVIEW_SNAPSHOTS";
const APPEARANCE_ENV: &str = "GUIDED_REVIEW_SNAPSHOT_APPEARANCE";
const TICK: Duration = Duration::from_millis(50);
const MIN_SETTLE: Duration = Duration::from_millis(400);
/// A scene is done once its picture stops changing for this long: code loaded, highlights and
/// diagrams drawn, one-shot animations ended.
const STABLE_FOR: Duration = Duration::from_secs(1);
/// A scene that never stops changing (an endless animation) is taken as it is after this long.
const MAX_SETTLE: Duration = Duration::from_secs(20);

pub fn is_on() -> bool {
    folder().is_some()
}

fn folder() -> Option<PathBuf> {
    std::env::var(FOLDER_ENV)
        .ok()
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

/// Pictures in one appearance whatever macOS shows, so a run gives the same pictures each time.
pub fn appearance() -> Option<Appearance> {
    if !is_on() {
        return None;
    }
    match std::env::var(APPEARANCE_ENV).ok()?.as_str() {
        "light" => Some(Appearance::Light),
        "dark" => Some(Appearance::Dark),
        _ => None,
    }
}

pub struct Camera {
    folder: PathBuf,
    /// Name starts of the pictures to take; empty takes them all.
    only: Vec<String>,
}

impl Camera {
    pub fn from_env() -> Option<Self> {
        let folder = folder()?;
        if let Err(error) = std::fs::create_dir_all(&folder) {
            eprintln!("could not make {}: {error}", folder.display());
            return None;
        }
        let only = std::env::var("GUIDED_REVIEW_SNAPSHOT_ONLY")
            .unwrap_or_default()
            .split(',')
            .map(str::trim)
            .filter(|start| !start.is_empty())
            .map(str::to_owned)
            .collect();
        Some(Self { folder, only })
    }

    fn wants(&self, name: &str) -> bool {
        self.only.is_empty()
            || self
                .only
                .iter()
                .any(|start| name.starts_with(start.as_str()))
    }

    /// Draws the window until its picture settles, then saves it as `<name>.png`. Fails once the
    /// window is gone.
    pub async fn take(&self, name: &str, cx: &mut AsyncWindowContext) -> anyhow::Result<()> {
        if !self.wants(name) {
            return Ok(());
        }
        let started = Instant::now();
        let mut last = None;
        let mut unchanged_since = started;
        loop {
            cx.background_executor().timer(TICK).await;
            let image = cx
                .update(|window, cx| {
                    // The real pointer may rest over the window; hover marks would differ run to run.
                    window.dispatch_event(
                        PlatformInput::MouseMove(MouseMoveEvent {
                            position: point(px(-1000.), px(-1000.)),
                            pressed_button: None,
                            modifiers: Modifiers::default(),
                        }),
                        cx,
                    );
                    window.refresh();
                    window.draw(cx).clear(cx);
                    window.render_to_image()
                })
                .context("the window is gone")?
                .context("could not draw the window")?;
            if last.as_ref() != Some(&image) {
                unchanged_since = Instant::now();
            }
            let is_settled =
                started.elapsed() >= MIN_SETTLE && unchanged_since.elapsed() >= STABLE_FOR;
            if is_settled || started.elapsed() >= MAX_SETTLE {
                if !is_settled {
                    eprintln!("{name} kept changing; taken after {MAX_SETTLE:?}");
                }
                let path = self.folder.join(format!("{name}.png"));
                return image
                    .save(&path)
                    .with_context(|| format!("could not save {}", path.display()));
            }
            last = Some(image);
        }
    }
}

/// Opens the finish-review form, whose open state lives in the sidebar's element state.
#[derive(Default)]
pub struct StagedFinishReview(pub bool);

impl Global for StagedFinishReview {}

/// Shows the gallery of surfaces the demo PR has none of, in place of the screen.
#[derive(Default)]
pub struct StagedGallery(pub bool);

impl Global for StagedGallery {}

/// A fixed list of this repo's PRs: the reviewer's real list may hold private repos.
pub struct SamplePullRequests;

impl RelevantPullRequestsPort for SamplePullRequests {
    fn relevant_pull_requests(&self) -> Result<RelevantPullRequests, GitHubError> {
        let sample = [
            (
                14,
                "Settings opens without a PR and keeps the app alive while an open builds",
                WhyListed::Mine,
            ),
            (
                12,
                "Settings panel for summary instructions, IDE, Ask and clones",
                WhyListed::Mine,
            ),
            (
                11,
                "viewer: a Settings model for config.toml that keeps comments",
                WhyListed::ReviewRequested,
            ),
            (
                10,
                "viewer: the summary screen shows the tour's reviewer summary",
                WhyListed::Involved,
            ),
        ];
        let pull_requests = sample
            .into_iter()
            .filter_map(|(number, title, why_listed)| {
                let url = sample_pr(number)?;
                Some(RelevantPullRequest {
                    url,
                    title: title.to_owned(),
                    author: "ekvedaras".to_owned(),
                    is_draft: false,
                    updated_at: format!("2026-11-{number:02}T12:00:00Z"),
                    why_listed,
                })
            })
            .collect();
        Ok(RelevantPullRequests {
            pull_requests,
            teams_left_out: Vec::new(),
        })
    }
}

pub fn sample_pr(number: u32) -> Option<PullRequestUrl> {
    format!("https://github.com/ekvedaras/guided-review/pull/{number}")
        .parse()
        .ok()
}

/// Ask starts its AI when the panel opens; pictures need the panel, not an answer.
pub struct SilentAgents;

impl AgentLauncher for SilentAgents {
    fn start(&self, _: &AgentLaunch) -> anyhow::Result<Box<dyn AgentPort>> {
        Ok(Box::new(SilentAgent))
    }
}

struct SilentAgent;

impl AgentPort for SilentAgent {
    fn ask(&mut self, _: &str, _: &mut dyn FnMut(&str)) -> anyhow::Result<()> {
        Ok(())
    }

    fn turn_stopper(&self) -> Arc<dyn TurnStopper> {
        Arc::new(Self)
    }
}

impl TurnStopper for SilentAgent {
    fn stop_turn(&self) -> anyhow::Result<()> {
        Ok(())
    }
}

pub fn quiet_ask(setup: AskSetup) -> AskSetup {
    if !is_on() {
        return setup;
    }
    AskSetup {
        launcher: Arc::new(SilentAgents),
        ..setup
    }
}
