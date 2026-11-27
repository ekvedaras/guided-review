//! Pictures of the app for looking at the UI without a screen grab. Built with the `snapshots`
//! feature and on when `GUIDED_REVIEW_SNAPSHOTS` names a folder: the layout stages its screens and
//! overlays one by one (`layouts::one_step::scenes`), each is saved there as a PNG, then the app quits.
//!
//! The window is drawn here on a timer, not by the display: a locked screen or a sleeping display
//! delivers no frames, so a run that waited for them would never draw.

use std::ops::ControlFlow;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Context as _;
use gpui::{AsyncWindowContext, Global, Modifiers, MouseMoveEvent, PlatformInput, point, px};

use crate::agent_cli::AgentCli;
use crate::app::components::palette::Appearance;
use crate::pull_request::PullRequestUrl;
use crate::review_subject::ReviewSubject;
use crate::viewer::{
    AgentLinkOff, AgentStatus, AppAgent, HostError, LiveTerminal, PrAuthor, RelevantPullRequest,
    RelevantPullRequests, RelevantPullRequestsPort, ReviewAgentStart, SendOutcome, TerminalId,
    TourAgentPort, TourAgentStart, WhyListed,
};

const FOLDER_ENV: &str = "GUIDED_REVIEW_SNAPSHOTS";
const APPEARANCE_ENV: &str = "GUIDED_REVIEW_SNAPSHOT_APPEARANCE";
const RELEASE_LOOK_ENV: &str = "GUIDED_REVIEW_SNAPSHOT_RELEASE_LOOK";
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

/// Pictures for the README look like the release install, with no `TEST` badge.
pub fn hides_build_badge() -> bool {
    is_on() && std::env::var(RELEASE_LOOK_ENV).is_ok_and(|value| value == "1")
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
        let image = Self::settle(name, cx).await?;
        let path = self.folder.join(format!("{name}.png"));
        image
            .save(&path)
            .with_context(|| format!("could not save {}", path.display()))
    }

    /// Why the run stopped early, beside its pictures: the app's own log goes with its throwaway home.
    pub fn note_stop(&self, error: &anyhow::Error) {
        std::fs::write(self.folder.join("stopped.txt"), format!("{error:#}\n")).ok();
    }

    /// When pictures are skipped nothing waits for the code to load, and the scenes need it loaded.
    pub async fn wait_for_loading(&self, cx: &mut AsyncWindowContext) -> anyhow::Result<()> {
        if !self.only.is_empty() {
            Self::settle("loading", cx).await?;
        }
        Ok(())
    }

    async fn settle(name: &str, cx: &mut AsyncWindowContext) -> anyhow::Result<image::RgbaImage> {
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
                return Ok(image);
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

/// A fixed list of PRs for the switcher's pictures: this repo's own, and made-up ones on `acme` repos with a
/// long repo name, long titles and drafts. The reviewer's real list may hold private repos.
pub struct SamplePullRequests;

impl RelevantPullRequestsPort for SamplePullRequests {
    fn relevant_pull_requests(&self) -> Result<RelevantPullRequests, HostError> {
        const OURS: &str = "ekvedaras/guided-review";
        const STOREFRONT: &str = "acme/storefront-checkout-and-payments-gateway";
        let sample = [
            (
                OURS,
                14,
                "Settings opens without a PR and keeps the app alive while an open builds",
                "ekvedaras",
                WhyListed::Mine,
                false,
            ),
            (
                "acme/shop",
                42,
                "Cart totals in cents, not floats",
                "sam",
                WhyListed::Involved,
                true,
            ),
            (
                OURS,
                12,
                "Settings panel for summary instructions, IDE and clones",
                "ekvedaras",
                WhyListed::Mine,
                false,
            ),
            (
                STOREFRONT,
                231,
                "Checkout retries a declined card once with 3-D Secure before the order is cancelled and its stock goes back",
                "rivera",
                WhyListed::ReviewRequested,
                false,
            ),
            (
                OURS,
                11,
                "viewer: a Settings model for config.toml that keeps comments",
                "ekvedaras",
                WhyListed::ReviewRequested,
                false,
            ),
            (
                OURS,
                10,
                "viewer: the summary screen shows the tour's reviewer summary",
                "ekvedaras",
                WhyListed::Involved,
                false,
            ),
            (
                STOREFRONT,
                229,
                "Stock never drops below zero when two orders race",
                "rivera",
                WhyListed::Involved,
                true,
            ),
            (
                OURS,
                8,
                "outline: summary_instructions may be { skill = \"<name>\" }, which outline names for the tour writer to run, and the tour agent the app starts is allowed Skill(<name>)",
                "ekvedaras",
                WhyListed::Mine,
                false,
            ),
            (
                OURS,
                7,
                "Summary instructions end the outline, and check warns when reviewer_summary is empty",
                "ekvedaras",
                WhyListed::Mine,
                false,
            ),
        ];
        let pull_requests = sample
            .into_iter()
            .enumerate()
            .filter_map(
                |(nth, (repo, number, title, author, why_listed, is_draft))| {
                    let url = format!("https://github.com/{repo}/pull/{number}")
                        .parse()
                        .ok()?;
                    Some(RelevantPullRequest {
                        url,
                        title: title.to_owned(),
                        author: PrAuthor {
                            login: author.into(),
                            avatar_url: (author == "ekvedaras").then(|| {
                                "https://avatars.githubusercontent.com/ekvedaras?s=64".into()
                            }),
                        },
                        is_draft,
                        updated_at: format!("2026-11-{:02}T12:00:00Z", 20 - nth),
                        why_listed,
                    })
                },
            )
            .collect();
        Ok(RelevantPullRequests {
            pull_requests,
            teams_left_out: Vec::new(),
        })
    }
}

/// Herdr as it is, but with none of the reviewer's own terminals: the switcher lists the folders their
/// Claude terminals work in, and those may be private repos that must not reach a picture.
pub struct NoReviewerTerminals(pub Arc<dyn TourAgentPort>);

impl TourAgentPort for NoReviewerTerminals {
    fn agent_cli(&self) -> AgentCli {
        self.0.agent_cli()
    }

    fn link(&self) -> Result<(), AgentLinkOff> {
        self.0.link()
    }

    fn terminals_of(&self, subject: &ReviewSubject) -> anyhow::Result<Vec<TerminalId>> {
        self.0.terminals_of(subject)
    }

    fn review_terminals_of(&self, subject: &ReviewSubject) -> anyhow::Result<Vec<TerminalId>> {
        self.0.review_terminals_of(subject)
    }

    fn live_terminals(&self) -> anyhow::Result<Vec<LiveTerminal>> {
        Ok(Vec::new())
    }

    fn claim(&self, terminal: &TerminalId, subject: &ReviewSubject) -> anyhow::Result<()> {
        self.0.claim(terminal, subject)
    }

    fn release(&self, terminal: &TerminalId, subject: &ReviewSubject) -> anyhow::Result<()> {
        self.0.release(terminal, subject)
    }

    fn claim_review(&self, terminal: &TerminalId, subject: &ReviewSubject) -> anyhow::Result<()> {
        self.0.claim_review(terminal, subject)
    }

    fn release_review(&self, terminal: &TerminalId, subject: &ReviewSubject) -> anyhow::Result<()> {
        self.0.release_review(terminal, subject)
    }

    fn status(&self, terminal: &TerminalId) -> anyhow::Result<AgentStatus> {
        self.0.status(terminal)
    }

    fn follow(
        &self,
        terminal: &TerminalId,
        on_status: &mut dyn FnMut(AgentStatus) -> ControlFlow<()>,
    ) -> anyhow::Result<()> {
        self.0.follow(terminal, on_status)
    }

    fn send_if_idle(&self, terminal: &TerminalId, text: &str) -> anyhow::Result<SendOutcome> {
        self.0.send_if_idle(terminal, text)
    }

    fn send(&self, terminal: &TerminalId, text: &str) -> anyhow::Result<()> {
        self.0.send(terminal, text)
    }

    fn show(&self, terminal: &TerminalId) -> anyhow::Result<()> {
        self.0.show(terminal)
    }

    fn app_started(&self) -> anyhow::Result<Vec<AppAgent>> {
        self.0.app_started()
    }

    fn start(&self, start: &TourAgentStart) -> anyhow::Result<TerminalId> {
        self.0.start(start)
    }

    fn start_review(&self, start: &ReviewAgentStart) -> anyhow::Result<TerminalId> {
        self.0.start_review(start)
    }

    fn kill(&self, terminal: &TerminalId) -> anyhow::Result<()> {
        self.0.kill(terminal)
    }

    fn screen_lines(&self, terminal: &TerminalId, count: usize) -> anyhow::Result<Vec<String>> {
        self.0.screen_lines(terminal, count)
    }
}

/// A small clone for a local review's pictures, under `home`: `main` with a README and a file that goes,
/// then `feature/cart` one commit ahead, with a changed README, a changed cart and a note that git does
/// not know yet. Made with git itself and no network, so no PR is needed.
pub fn local_repo_in(home: &std::path::Path) -> anyhow::Result<PathBuf> {
    let folder = home.join("shop");
    // Staged once per scene: the tour on disk is pinned to this clone's commits, so it is kept as it is.
    if folder.join(".git").exists() {
        return Ok(folder);
    }
    std::fs::create_dir_all(folder.join("src"))?;
    let git = |args: &[&str]| -> anyhow::Result<()> {
        let status = std::process::Command::new("git")
            .arg("-C")
            .arg(&folder)
            .args([
                "-c",
                "user.name=Sample",
                "-c",
                "user.email=sample@example.invalid",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .status()?;
        anyhow::ensure!(status.success(), "git {} failed", args.join(" "));
        Ok(())
    };
    let write = |path: &str, text: &str| std::fs::write(folder.join(path), text);
    git(&["init", "--quiet", "--initial-branch=main"])?;
    git(&[
        "remote",
        "add",
        "origin",
        "https://github.com/acme/shop.git",
    ])?;
    write("README.md", "# Shop\n\nOrders cost two.\n")?;
    write("old.txt", "goes away\n")?;
    git(&["add", "--all"])?;
    git(&["commit", "--quiet", "-m", "Start the shop"])?;
    git(&["checkout", "--quiet", "-b", "feature/cart"])?;
    write(
        "src/cart.rs",
        "pub fn total(items: &[u32]) -> u32 {\n    items.iter().sum()\n}\n",
    )?;
    std::fs::remove_file(folder.join("old.txt"))?;
    git(&["add", "--all"])?;
    git(&["commit", "--quiet", "-m", "Add the cart"])?;
    write(
        "README.md",
        "# Shop\n\nOrders cost two, and a cart adds them up.\n",
    )?;
    write(
        "src/cart.rs",
        "pub fn total(items: &[u32]) -> u32 {\n    items.iter().sum()\n}\n\npub fn tax(total: u32) -> u32 {\n    total / 5\n}\n",
    )?;
    write("notes.txt", "ask about tax rules\n")?;
    Ok(folder)
}

pub fn sample_pr(number: u32) -> Option<PullRequestUrl> {
    format!("https://github.com/ekvedaras/guided-review/pull/{number}")
        .parse()
        .ok()
}
