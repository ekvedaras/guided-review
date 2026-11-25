use anyhow::Context as _;

use crate::app_folders::AppFolders;
use crate::pieces;
use crate::pull_request::Sha;
use crate::review_subject::ReviewSubject;
use crate::tour::{BasicTour, GuidedTour, Tour};

use super::carried::ReadCarrier;
use super::new_commits::PickCodeSource;
use super::replacing_jobs::RepinnedTour;
use super::review_builder::ReviewBuilder;
use super::review_state::ReviewStateFile;

/// Switch to guided tour as the app runs it: the basic tour's read ticks go to the guided tour's
/// steps, the basic file goes, and a review of the guided tour opens.
pub struct TourSwitcher {
    pub folders: AppFolders,
    pub subject: ReviewSubject,
    pub pick_code_source: PickCodeSource,
}

impl TourSwitcher {
    /// Blocks on the code source and on git; the app runs it off the main thread.
    pub fn run(&self) -> anyhow::Result<RepinnedTour> {
        let guided =
            GuidedTour::load_of(&Tour::path_for(&self.folders, &self.subject), &self.subject)?;
        let source = (self.pick_code_source)(&guided.pin.pinned_tips())?;
        let state_file = ReviewStateFile::for_subject(&self.folders, &self.subject);
        let files = pieces::cut(source.as_ref(), &guided.pin.base, &guided.pin.head)?;
        let reads = state_file.load()?.file_read_heads();
        let heads = reads.values().cloned().chain(self.basic_head()).collect();
        let carrier = ReadCarrier::of(&guided, &files, &heads, source.as_ref())?;
        let carried = carrier.carry(&reads);
        state_file.update(|state| state.carry_file_reads_into_steps(&carried))?;
        let mut review = ReviewBuilder::new(&self.folders, &self.subject)
            .build(Tour::Guided(guided), source.as_ref())?;
        let started_read = review.progress().read;
        review.steps_carried_read_ticks_are(started_read);
        review.takes_over_from_basic_with(carrier);
        self.delete_basic_file()?;
        Ok(RepinnedTour {
            review,
            source: source.into(),
        })
    }

    /// The head the basic review ticks files at while the guided review is built.
    fn basic_head(&self) -> Option<Sha> {
        BasicTour::load_of(
            &Tour::basic_path_for(&self.folders, &self.subject),
            &self.subject,
        )
        .ok()
        .map(|basic| basic.pin.head)
    }

    /// Only once the state is saved: a crash before this carries again instead of losing ticks.
    fn delete_basic_file(&self) -> anyhow::Result<()> {
        let path = Tour::basic_path_for(&self.folders, &self.subject);
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => {
                Err(error).with_context(|| format!("could not delete {}", path.display()))
            }
        }
    }
}
