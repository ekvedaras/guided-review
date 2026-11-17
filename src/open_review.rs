use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::app::AskSetup;
use crate::app_folders::AppFolders;
use crate::claude_cli::{ClaudeCli, ClaudeRefreshWriter};
use crate::code_source::CodeSource;
use crate::config::Config;
use crate::github::GitHub;
use crate::hc_tour_agent::HcTourAgents;
use crate::ide::{Ide, NoIde};
use crate::local_git::LocalGit;
use crate::opened_change_set::{OpenedChangeSet, local_reviews_of_repo};
use crate::opened_pr::OpenedPr;
use crate::pull_request::PullRequestUrl;
use crate::review_subject::{LocalChangeSet, ReviewSubject};
use crate::summary_instructions::SummaryInstructions;
use crate::tour::Tour;
use crate::viewer::{
    GitHubPort, NoGitHub, PickCodeSource, PrMover, Review, ReviewBuilder, ReviewStateFile,
    TourAgents, TourRefresher, TourReloader, TourRepinner, TourSwitcher,
    let_go_of_unread_snapshots,
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
    pub switcher: TourSwitcher,
    pub tour_agents: TourAgents,
}

/// Where a tour agent that writes the subject's tour works. A local change set's agent works in its
/// folder. A PR's works in the clone the PR was last opened from, else one another PR of the repo was
/// opened from, else the clone `config.toml` names, else the reviewer's home, where the skill reads
/// code through `show`.
pub fn writer_folder(subject: &ReviewSubject, folders: &AppFolders) -> PathBuf {
    let pr = match subject {
        ReviewSubject::PullRequest(pr) => pr,
        ReviewSubject::Local(change_set) => return change_set.folder().as_path().to_path_buf(),
    };
    let own = ReviewStateFile::for_subject(folders, subject)
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

/// What the two kinds of subject build differently; the rest of the review is the same.
struct Built {
    review: Review,
    source: Box<dyn CodeSource>,
    pick_code_source: PickCodeSource,
    github: Arc<dyn GitHubPort>,
}

/// Builds the review of `subject` at the commits its tour is pinned to. `clones_from` is the folder a
/// local clone is searched from. Slow, so the app runs it off the UI thread.
pub fn open_review(
    subject: &ReviewSubject,
    clones_from: &Path,
    folders: &AppFolders,
) -> anyhow::Result<OpenedReview> {
    let subject = &with_its_repo(subject);
    let built = match subject {
        ReviewSubject::PullRequest(pr) => build_pull_request(pr, clones_from, folders)?,
        ReviewSubject::Local(change_set) => build_local(change_set, folders)?,
    };
    assemble(subject, folders, built)
}

/// A local change set named by its key alone learns its repo from the clone's `origin`, and keeps it
/// for every review rebuilt after this open.
fn with_its_repo(subject: &ReviewSubject) -> ReviewSubject {
    match subject {
        ReviewSubject::PullRequest(_) => subject.clone(),
        ReviewSubject::Local(change_set) => ReviewSubject::from(
            change_set
                .clone()
                .in_repo(LocalGit::origin_of(change_set.folder().as_path())),
        ),
    }
}

fn build_pull_request(
    pr: &PullRequestUrl,
    clones_from: &Path,
    folders: &AppFolders,
) -> anyhow::Result<Built> {
    let subject = ReviewSubject::from(pr.clone());
    let opened = OpenedPr::in_folders(pr, folders, clones_from);
    opened.make_basic_tour_if_no_tour()?;
    if let Err(error) = let_go_of_unread_snapshots(folders, pr) {
        // A ref kept too long only waits for the next open; the review reads the same either way.
        eprintln!("could not let go of the snapshots {pr} no longer reads: {error:#}");
    }
    let tour = Tour::load_of(&Tour::shown_path_for(folders, &subject), &subject)?;
    let code = opened.at_pinned_tips(tour.pin())?;
    let mut review = ReviewBuilder::new(folders, &subject).build(tour, code.source.as_ref())?;
    review.local_reviews_of_repo_are(folders, local_reviews_of_repo(folders, pr));
    opened.state_file().remember_folder(clones_from)?;
    Ok(Built {
        review,
        source: code.source,
        pick_code_source: opened.code_source_picker(),
        github: Arc::new(opened.github),
    })
}

/// A local review reads the snapshot its tour is pinned to from its own folder, and has no GitHub to
/// ask: the port it holds refuses every call.
fn build_local(change_set: &LocalChangeSet, folders: &AppFolders) -> anyhow::Result<Built> {
    let subject = ReviewSubject::from(change_set.clone());
    let opened = OpenedChangeSet::new(change_set, folders);
    opened.make_basic_tour_if_no_tour()?;
    let tour = Tour::load_of(&Tour::shown_path_for(folders, &subject), &subject)?;
    let code = opened.at_pinned_tips()?;
    let review = ReviewBuilder::new(folders, &subject).build(tour, code.source.as_ref())?;
    Ok(Built {
        review,
        source: code.source,
        pick_code_source: opened.code_source_picker(),
        github: Arc::new(NoGitHub::looking_up_branches_with(Arc::new(
            GitHub::from_env(),
        ))),
    })
}

fn assemble(
    subject: &ReviewSubject,
    folders: &AppFolders,
    built: Built,
) -> anyhow::Result<OpenedReview> {
    let Built {
        review,
        source,
        pick_code_source,
        github,
    } = built;
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
    Ok(OpenedReview {
        review,
        source: source.into(),
        ask,
        ide: Ide::for_clone(clone, config.ide_template()),
        github,
        repinner,
        refresher,
        reloader,
        switcher,
        tour_agents: TourAgents::new(Arc::new(HcTourAgents::new(Tour::tours_dir(folders))))
            .starting_in(agent_folder)
            .with_summary_skill(SummaryInstructions::skill_for_launch(folders)),
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
    }
}
