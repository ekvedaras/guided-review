use std::sync::Arc;

use crate::app_folders::AppFolders;
use crate::review_subject::ReviewSubject;
use crate::tour::{GuidedTour, Tour};

use super::capability::HostCapabilities;
use super::new_commits::PickCodeSource;
use super::ports::RefreshWriter;
use super::replacing_jobs::RepinnedTour;
use super::review_builder::ReviewBuilder;

/// The Refresh button's work: the subject's tour agent rewrites the marked steps, then a fresh `Review` opens on the result.
pub struct TourRefresher {
    pub folders: AppFolders,
    pub subject: ReviewSubject,
    pub pick_code_source: PickCodeSource,
    pub host: HostCapabilities,
    pub writer: Arc<dyn RefreshWriter>,
}

impl TourRefresher {
    /// Blocks until the agent is done; the app runs it off the main thread. `on_activity` hears what the agent is doing.
    pub fn run(&self, on_activity: &mut dyn FnMut(String)) -> anyhow::Result<RepinnedTour> {
        self.writer.refresh(&self.subject, on_activity)?;
        let tour =
            GuidedTour::load_of(&Tour::path_for(&self.folders, &self.subject), &self.subject)?;
        let source = (self.pick_code_source)(&tour.pin.pinned_tips())?;
        let review = ReviewBuilder::new(&self.folders, &self.subject, self.host)
            .build(Tour::Guided(tour), source.as_ref())?;
        if !review.refresh_brief().is_empty() {
            anyhow::bail!(
                "the tour agent ended its turn without refreshing the tour: the marks are still there"
            );
        }
        Ok(RepinnedTour {
            review,
            source: source.into(),
        })
    }
}
