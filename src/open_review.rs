use std::path::Path;
use std::sync::Arc;

use crate::app_folders::AppFolders;
use crate::claude_cli::{ClaudeCli, ClaudeRefreshWriter};
use crate::code_source::CodeSource;
use crate::config::{AskSettings, Config};
use crate::github::GitHub;
use crate::hc_tour_agent::HcTourAgents;
use crate::ide::{Ide, NoIde};
use crate::instructions::TourWriter;
use crate::opened_subject::{OpenedChangeSet, OpenedSubject, TourRepinner};
use crate::review_subject::{LocalChangeSet, ReviewSubject};
use crate::tour::Tour;
use crate::viewer::{
    AgentLauncher, CodeHost, PrMover, PullsOfBranchPort, Review, TourAgents, TourRefresher,
    TourReloader, TourSwitcher,
};

/// Everything the Ask panel needs to start the AI, built once at startup.
pub struct AskSetup {
    pub launcher: Arc<dyn AgentLauncher>,
    pub settings: AskSettings,
}

/// Everything the app shows and works with for one PR. Nothing else holds the PR's objects, so dropping
/// this frees them.
pub struct OpenedReview {
    pub review: Review,
    pub source: Arc<dyn CodeSource>,
    pub ask: AskSetup,
    pub ide: Result<Ide, NoIde>,
    pub host: Arc<dyn CodeHost>,
    pub repinner: TourRepinner,
    pub refresher: TourRefresher,
    pub reloader: TourReloader,
    pub switcher: TourSwitcher,
    pub tour_agents: TourAgents,
}

/// Builds the review of `subject` at the commits its tour is pinned to. `clones_from` is the folder a
/// local clone is searched from. Slow, so the app runs it off the UI thread.
pub fn open_review(
    subject: &ReviewSubject,
    clones_from: &Path,
    folders: &AppFolders,
) -> anyhow::Result<OpenedReview> {
    let opened = OpenedSubject::in_folders(subject, folders, clones_from);
    // A local change set keeps the repo its clone names for every review rebuilt after this open.
    let subject = &opened.subject();
    let (mut review, source) = opened.open_review()?;
    let pick_code_source = opened.code_source_picker();
    let host = opened.host();
    review.host_is(host.as_ref());
    let clone = source.local_clone().map(Path::to_path_buf);
    let config = Config::load_from(folders)?;
    let agent_folder = clone
        .clone()
        .unwrap_or_else(|| folders.home().to_path_buf());
    let ask = AskSetup {
        launcher: Arc::new(ClaudeCli::new()),
        settings: config.ask_settings(),
    };
    let refresher = TourRefresher {
        folders: folders.clone(),
        subject: subject.clone(),
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
        subject: subject.clone(),
        pick_code_source: Arc::clone(&pick_code_source),
    };
    let switcher = TourSwitcher {
        folders: folders.clone(),
        subject: subject.clone(),
        pick_code_source: Arc::clone(&pick_code_source),
    };
    let repinner = TourRepinner {
        folders: folders.clone(),
        subject: subject.clone(),
        pick_code_source,
    };
    let tour_agents = TourAgents::new(Arc::new(HcTourAgents::new(Tour::tours_dir(folders))))
        .finding_head_branches_with(Arc::clone(&host) as Arc<dyn PullsOfBranchPort>)
        .starting_in(agent_folder)
        .with_summary_skill(Config::skill_for_launch::<TourWriter>(folders));
    Ok(OpenedReview {
        review,
        source: source.into(),
        ask,
        ide: Ide::for_clone(clone, config.ide_template()),
        host,
        repinner,
        refresher,
        reloader,
        switcher,
        tour_agents,
    })
}

/// The move of a local review to a PR, reading the PR's commits from the review's own folder.
pub fn mover_of(folders: &AppFolders, change_set: &LocalChangeSet) -> PrMover {
    let folder = change_set.folder().as_path().to_path_buf();
    PrMover {
        folders: folders.clone(),
        change_set: change_set.clone(),
        pick_code_source: Arc::new(move |pr, tips| {
            crate::pick_code_source::pick_code_source(&GitHub::from_env(), pr, tips, &folder)
        }),
        drop_local_review: Arc::new(|change_set, folders| {
            OpenedChangeSet::new(change_set, folders).drop_review()
        }),
    }
}
