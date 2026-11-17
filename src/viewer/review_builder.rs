use crate::app_folders::AppFolders;
use crate::code_source::CodeSource;
use crate::config::Config;
use crate::opened_subject::{OpenedChangeSet, unreadable_local_tours};
use crate::review_subject::ReviewSubject;
use crate::tour::Tour;

use super::branch_link::LocalReviewsOfRepo;
use super::code_link::WorkingCopy;
use super::problems::Problem;
use super::review::Review;
use super::review_state::ReviewStateFile;

/// Builds a subject's review from its tour as it is on disk, with every fact the config, the clone
/// and the app folders give it. Open, Update tour, reload, Refresh and the switch to the guided tour
/// all build here and differ only in how they change the tour file first, so a rebuilt review reads
/// as one opened fresh; what the reviewer was doing is the replaced review's to hand on.
pub struct ReviewBuilder {
    folders: AppFolders,
    subject: ReviewSubject,
}

impl ReviewBuilder {
    /// A local change set's `subject` names its repo, which the review looks up its branch's PR in.
    pub fn new(folders: &AppFolders, subject: &ReviewSubject) -> Self {
        Self {
            folders: folders.clone(),
            subject: subject.clone(),
        }
    }

    /// Reads the code at the tour's own base and head. Blocks on git; the app runs it off the main
    /// thread.
    pub fn build(&self, tour: Tour, source: &dyn CodeSource) -> anyhow::Result<Review> {
        let config = Config::load_from(&self.folders)?;
        let local_facts = self
            .subject
            .local()
            .map(|change_set| OpenedChangeSet::new(change_set, &self.folders).facts(tour.pin()))
            .transpose()?;
        let working_copy = WorkingCopy::of(source.working_tree_is_at(&tour.pin().head));
        let state_file = ReviewStateFile::for_subject(&self.folders, &self.subject);
        let mut review = match tour {
            Tour::Basic(tour) => Review::open_basic(tour, source, state_file)?,
            Tour::Guided(tour) => Review::open(tour, source, state_file)?,
        };
        if let Some(facts) = local_facts {
            review.local_facts_are(facts);
        }
        review.working_copy_is(working_copy);
        review
            .display_options_mut()
            .use_more_lines(config.more_lines());
        if let Some(pr) = self.subject.pull_request() {
            review.prefer_merge_method(config.merge_method(&pr.repo));
            review.local_reviews_of_repo_are(LocalReviewsOfRepo::on_disk(&self.folders, pr));
        }
        let unreadable = unreadable_local_tours(&self.folders);
        if !unreadable.is_empty() {
            review.raise_problem(Problem::LocalToursNotRead(unreadable));
        }
        Ok(review)
    }
}
