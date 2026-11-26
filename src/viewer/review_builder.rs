use crate::app_folders::AppFolders;
use crate::code_source::CodeSource;
use crate::config::Config;
use crate::local_reviews_on_disk::unreadable_local_tours;
use crate::local_work_tree::LocalWorkTree;
use crate::review_subject::ReviewSubject;
use crate::tour::Tour;

use super::branch_link::LocalReviewsOfRepo;
use super::capability::HostCapabilities;
use super::code_link::WorkingCopy;
use super::new_commits::PickCodeSource;
use super::new_commits::catch_up_findings;
use super::problems::Problem;
use super::replacing_jobs::RepinnedTour;
use super::review::Review;
use super::review_state::ReviewStateFile;
use crate::tour::GuidedTour;

/// Builds a subject's review from its tour as it is on disk, with every fact the config, the clone
/// and the app folders give it. Open, Update tour, reload, Refresh and the switch to the guided tour
/// all build here and differ only in how they change the tour file first, so a rebuilt review reads
/// as one opened fresh; what the reviewer was doing is the replaced review's to hand on.
pub struct ReviewBuilder {
    folders: AppFolders,
    subject: ReviewSubject,
    host: HostCapabilities,
}

impl ReviewBuilder {
    /// A local change set's `subject` names its repo, which the review looks up its branch's PR in.
    pub fn new(folders: &AppFolders, subject: &ReviewSubject, host: HostCapabilities) -> Self {
        Self {
            folders: folders.clone(),
            subject: subject.clone(),
            host,
        }
    }

    /// Reads the code at the tour's own base and head. Blocks on git; the app runs it off the main
    /// thread.
    pub fn build(&self, tour: Tour, source: &dyn CodeSource) -> anyhow::Result<Review> {
        let config = Config::load_from(&self.folders)?;
        let local_facts = self
            .subject
            .local()
            .map(|change_set| {
                LocalWorkTree::at(change_set.folder().clone())
                    .facts(change_set, &tour.pin().pinned_tips())
            })
            .transpose()?;
        let pinned = tour.pin().pinned_tips();
        let working_copy = WorkingCopy::of(source.working_tree_is_at(&pinned.head));
        let state_file = ReviewStateFile::for_subject(&self.folders, &self.subject);
        if let Err(error) = catch_up_findings(
            &self.folders,
            &self.subject,
            &state_file,
            source,
            &pinned.base_branch_tip,
            &pinned.head,
        ) {
            eprintln!("could not follow the findings to the review's commits: {error:#}");
        }
        let mut review = match tour {
            Tour::Basic(tour) => Review::open_basic(tour, source, state_file, self.host)?,
            Tour::Guided(tour) => Review::open(tour, source, state_file, self.host)?,
        };
        if let Some(facts) = local_facts {
            review.local_facts_are(facts);
        }
        review.working_copy_is(working_copy);
        review.read_findings(&self.folders);
        review
            .display_options_mut()
            .use_more_lines(config.more_lines());
        review.read_merge_method_from(&config);
        if let Some(pr) = self.subject.pull_request() {
            review.local_reviews_of_repo_are(LocalReviewsOfRepo::on_disk(&self.folders, pr));
        }
        let unreadable = unreadable_local_tours(&self.folders);
        if !unreadable.is_empty() {
            review.raise_problem(Problem::LocalToursNotRead(unreadable));
        }
        Ok(review)
    }
}

/// Reads the tour file again at the commits it is pinned to, for a tour the agent rewrote while the app
/// shows it.
pub struct TourReloader {
    pub folders: AppFolders,
    pub subject: ReviewSubject,
    pub pick_code_source: PickCodeSource,
    pub host: HostCapabilities,
}

impl TourReloader {
    /// Blocks on the code source; the app runs it off the main thread.
    pub fn run(&self) -> anyhow::Result<RepinnedTour> {
        let tour =
            GuidedTour::load_of(&Tour::path_for(&self.folders, &self.subject), &self.subject)?;
        let source = (self.pick_code_source)(&tour.pin.pinned_tips())?;
        let review = ReviewBuilder::new(&self.folders, &self.subject, self.host)
            .build(Tour::Guided(tour), source.as_ref())?;
        Ok(RepinnedTour {
            review,
            source: source.into(),
        })
    }
}
