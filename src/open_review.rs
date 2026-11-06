use std::path::Path;
use std::sync::Arc;

use crate::app::AskSetup;
use crate::claude_cli::{ClaudeCli, ClaudeRefreshWriter};
use crate::code_source::CodeSource;
use crate::config::Config;
use crate::hc_tour_agent::HcTourAgents;
use crate::ide::{Ide, NoIde};
use crate::opened_pr::OpenedPr;
use crate::pull_request::PullRequestUrl;
use crate::tour::Tour;
use crate::viewer::{
    Asks, GitHubPort, Review, TourAgents, TourRefresher, TourReloader, TourRepinner, WorkingCopy,
};

/// Everything the app shows and works with for one PR. Nothing else holds the PR's objects, so dropping
/// this frees them.
pub struct OpenedReview {
    pub review: Review,
    pub source: Arc<dyn CodeSource>,
    pub ask: AskSetup,
    pub ide: Result<Ide, NoIde>,
    pub github: Arc<dyn GitHubPort>,
    pub repinner: TourRepinner,
    pub refresher: TourRefresher,
    pub reloader: TourReloader,
    pub tour_agents: TourAgents,
}

/// The refusal `open` gives before it asks or starts any app.
pub fn require_tour(pr: &PullRequestUrl, home: &Path) -> anyhow::Result<()> {
    if !Tour::path_for(home, pr).exists() {
        anyhow::bail!("no tour for {pr} yet; run `/tour {pr}` first");
    }
    Ok(())
}

/// Builds the review of `pr` at the commits its tour is pinned to. `clones_from` is the folder a local
/// clone is searched from. Slow, so the app runs it off the UI thread.
pub fn open_review(
    pr: &PullRequestUrl,
    clones_from: &Path,
    home: &Path,
) -> anyhow::Result<OpenedReview> {
    let opened = OpenedPr::in_folders(pr, home, clones_from);
    require_tour(pr, home)?;
    let tour = Tour::load(&opened.tour_path())?;
    let code = opened.at_pinned_tips(&tour)?;
    let working_copy_is_at_tour_head = code.source.working_tree_is_at(&tour.head);
    let mut review = Review::open(tour, code.source.as_ref(), opened.state_file())?;
    review.working_copy_is(WorkingCopy::of(working_copy_is_at_tour_head));
    opened.state_file().remember_folder(clones_from)?;

    let clone = code.source.local_clone().map(Path::to_path_buf);
    let config = Config::load_from(home)?;
    let pick_code_source = opened.code_source_picker();

    let agent_folder = clone.clone().unwrap_or_else(|| home.to_path_buf());
    let ask = AskSetup {
        asks: Asks::load(Arc::new(opened.state_file()))?,
        launcher: Arc::new(ClaudeCli::new()),
        clone: clone.clone(),
        settings: config.ask_settings(),
    };
    let refresher = TourRefresher {
        home: home.to_path_buf(),
        pr: pr.clone(),
        pick_code_source: Arc::clone(&pick_code_source),
        writer: Arc::new(ClaudeRefreshWriter::new(
            clone.clone().unwrap_or_else(|| home.to_path_buf()),
        )),
    };
    let reloader = TourReloader {
        home: home.to_path_buf(),
        pr: pr.clone(),
        pick_code_source: Arc::clone(&pick_code_source),
    };
    let repinner = TourRepinner {
        home: home.to_path_buf(),
        pr: pr.clone(),
        pick_code_source,
    };
    Ok(OpenedReview {
        review,
        source: code.source.into(),
        ask,
        ide: Ide::for_clone(clone, config.ide_template()),
        github: Arc::new(opened.github),
        repinner,
        refresher,
        reloader,
        tour_agents: TourAgents::new(Arc::new(HcTourAgents::new())).starting_in(agent_folder),
    })
}
