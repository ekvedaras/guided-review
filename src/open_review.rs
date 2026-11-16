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
use crate::opened_change_set::OpenedChangeSet;
use crate::opened_pr::OpenedPr;
use crate::pull_request::PullRequestUrl;
use crate::review_subject::{LocalChangeSet, ReviewSubject};
use crate::summary_instructions::SummaryInstructions;
use crate::tour::{BasicTour, GuidedTour, Tour, ToursOnDisk};
use crate::viewer::{
    GitHubPort, NoGitHub, PickCodeSource, Review, ReviewStateFile, TourAgents, TourRefresher,
    TourReloader, TourRepinner, TourSwitcher, WorkingCopy,
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
    let built = match subject {
        ReviewSubject::PullRequest(pr) => build_pull_request(pr, clones_from, folders)?,
        ReviewSubject::Local(change_set) => build_local(change_set, folders)?,
    };
    assemble(subject, folders, built)
}

fn build_pull_request(
    pr: &PullRequestUrl,
    clones_from: &Path,
    folders: &AppFolders,
) -> anyhow::Result<Built> {
    let subject = ReviewSubject::from(pr.clone());
    let opened = OpenedPr::in_folders(pr, folders, clones_from);
    opened.make_basic_tour_if_no_tour()?;
    // A basic file with a guided one beside it shows the basic tour.
    let tour = if ToursOnDisk::of(folders, &subject).opens_basic() {
        Tour::Basic(BasicTour::load_of(
            &Tour::basic_path_for(folders, &subject),
            &subject,
        )?)
    } else {
        Tour::Guided(GuidedTour::load_of(&opened.tour_path(), &subject)?)
    };
    let code = opened.at_pinned_tips(tour.pin())?;
    let working_copy_is_at_tour_head = code.source.working_tree_is_at(&tour.pin().head);
    let mut review = match tour {
        Tour::Basic(tour) => Review::open_basic(tour, code.source.as_ref(), opened.state_file())?,
        Tour::Guided(tour) => Review::open(tour, code.source.as_ref(), opened.state_file())?,
    };
    review.working_copy_is(WorkingCopy::of(working_copy_is_at_tour_head));
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
    let tour = if ToursOnDisk::of(folders, &subject).opens_basic() {
        Tour::Basic(BasicTour::load_of(
            &Tour::basic_path_for(folders, &subject),
            &subject,
        )?)
    } else {
        Tour::Guided(GuidedTour::load_of(&opened.tour_path(), &subject)?)
    };
    let code = opened.at_pinned_tips()?;
    let facts = opened.facts(tour.pin())?;
    let working_copy_is_at_tour_head = code.source.working_tree_is_at(&tour.pin().head);
    let mut review = match tour {
        Tour::Basic(tour) => Review::open_basic(tour, code.source.as_ref(), opened.state_file())?,
        Tour::Guided(tour) => Review::open(tour, code.source.as_ref(), opened.state_file())?,
    };
    review.local_facts_are(facts);
    review.working_copy_is(WorkingCopy::of(working_copy_is_at_tour_head));
    Ok(Built {
        review,
        source: code.source,
        pick_code_source: opened.code_source_picker(),
        github: Arc::new(NoGitHub),
    })
}

fn assemble(
    subject: &ReviewSubject,
    folders: &AppFolders,
    built: Built,
) -> anyhow::Result<OpenedReview> {
    let Built {
        mut review,
        source,
        pick_code_source,
        github,
    } = built;
    let clone = source.local_clone().map(Path::to_path_buf);
    let config = Config::load_from(folders)?;
    review
        .display_options_mut()
        .use_more_lines(config.more_lines());

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
