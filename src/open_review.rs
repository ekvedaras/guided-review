use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::app::AskSetup;
use crate::app_folders::AppFolders;
use crate::claude_cli::{ClaudeCli, ClaudeRefreshWriter};
use crate::code_source::CodeSource;
use crate::config::Config;
use crate::hc_tour_agent::HcTourAgents;
use crate::ide::{Ide, NoIde};
use crate::local_git::LocalGit;
use crate::opened_pr::OpenedPr;
use crate::pull_request::PullRequestUrl;
use crate::summary_instructions::SummaryInstructions;
use crate::tour::{BasicTour, GuidedTour, Tour, ToursOnDisk};
use crate::viewer::{
    GitHubPort, Review, ReviewStateFile, TourAgents, TourRefresher, TourReloader, TourRepinner,
    WorkingCopy,
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

/// Where a tour agent that writes the PR's tour works: the clone the PR was last opened from, else one
/// another PR of the repo was opened from, else the clone `config.toml` names, else the reviewer's
/// home, where the skill reads code through `show`.
pub fn writer_folder(pr: &PullRequestUrl, folders: &AppFolders) -> PathBuf {
    let own = ReviewStateFile::for_pr(folders, pr)
        .opened_from()
        .ok()
        .flatten();
    let others = ReviewStateFile::folders_opened_from_in(folders, pr);
    // An unreadable state file or config only costs the clone: the skill still reads code through `show`.
    own.into_iter()
        .chain(others)
        .chain([folders.home().to_path_buf()])
        .find_map(|folder| LocalGit::find(&pr.repo, &folder).ok().flatten())
        .map_or_else(
            || folders.home().to_path_buf(),
            |clone| clone.root().to_path_buf(),
        )
}

/// Builds the review of `pr` at the commits its tour is pinned to. `clones_from` is the folder a local
/// clone is searched from. Slow, so the app runs it off the UI thread.
pub fn open_review(
    pr: &PullRequestUrl,
    clones_from: &Path,
    folders: &AppFolders,
) -> anyhow::Result<OpenedReview> {
    let opened = OpenedPr::in_folders(pr, folders, clones_from);
    opened.make_basic_tour_if_no_tour()?;
    // A basic file with a guided one beside it shows the basic tour; the guided tour waits for #86.
    let tour = if ToursOnDisk::of(folders, pr).opens_basic() {
        Tour::Basic(BasicTour::load(&Tour::basic_path_for(folders, pr))?)
    } else {
        Tour::Guided(GuidedTour::load(&opened.tour_path())?)
    };
    let code = opened.at_pinned_tips(tour.pin())?;
    let working_copy_is_at_tour_head = code.source.working_tree_is_at(&tour.pin().head);
    let mut review = match tour {
        Tour::Basic(tour) => Review::open_basic(tour, code.source.as_ref(), opened.state_file())?,
        Tour::Guided(tour) => Review::open(tour, code.source.as_ref(), opened.state_file())?,
    };
    review.working_copy_is(WorkingCopy::of(working_copy_is_at_tour_head));
    opened.state_file().remember_folder(clones_from)?;

    let clone = code.source.local_clone().map(Path::to_path_buf);
    let config = Config::load_from(folders)?;
    review
        .display_options_mut()
        .use_more_lines(config.more_lines());
    let pick_code_source = opened.code_source_picker();

    let agent_folder = clone
        .clone()
        .unwrap_or_else(|| folders.home().to_path_buf());
    let ask = AskSetup {
        launcher: Arc::new(ClaudeCli::new()),
        settings: config.ask_settings(),
    };
    let refresher = TourRefresher {
        folders: folders.clone(),
        pr: pr.clone(),
        pick_code_source: Arc::clone(&pick_code_source),
        writer: Arc::new(ClaudeRefreshWriter::new(
            clone
                .clone()
                .unwrap_or_else(|| folders.home().to_path_buf()),
            Tour::tours_dir(folders),
        )),
    };
    let reloader = TourReloader {
        folders: folders.clone(),
        pr: pr.clone(),
        pick_code_source: Arc::clone(&pick_code_source),
    };
    let repinner = TourRepinner {
        folders: folders.clone(),
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
        tour_agents: TourAgents::new(Arc::new(HcTourAgents::new(Tour::tours_dir(folders))))
            .starting_in(agent_folder)
            .with_summary_skill(SummaryInstructions::skill_for_launch(folders)),
    })
}
