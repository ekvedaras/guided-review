use std::sync::Arc;

use crate::app_folders::AppFolders;
use crate::review_subject::ReviewSubject;
use crate::tour::{GuidedTour, Tour};

use super::agent_event::AgentEvent;
use super::new_commits::PickCodeSource;
use super::ports::RefreshWriter;
use super::replacing_jobs::RepinnedTour;
use super::review_builder::ReviewBuilder;

/// The Refresh button's work: the local AI rewrites the marked steps, then a fresh `Review` opens on the result.
pub struct TourRefresher {
    pub folders: AppFolders,
    pub subject: ReviewSubject,
    pub pick_code_source: PickCodeSource,
    pub writer: Arc<dyn RefreshWriter>,
}

impl TourRefresher {
    /// Blocks until the AI is done; the app runs it off the main thread. `on_activity` hears each tool the AI uses.
    pub fn run(&self, on_activity: &mut dyn FnMut(String)) -> anyhow::Result<RepinnedTour> {
        let mut said = String::new();
        let mut failure = None;
        self.writer.refresh(&self.subject, &mut |line| {
            for event in AgentEvent::from_line(line) {
                match event {
                    AgentEvent::ToolUse { summary } => on_activity(summary),
                    AgentEvent::Error(reason) => failure = Some(reason),
                    AgentEvent::TextDelta(_) | AgentEvent::Done { .. } => {}
                }
            }
            if let Some(answer) = AgentEvent::final_answer(line) {
                said = answer;
            }
        })?;
        if let Some(reason) = failure {
            anyhow::bail!("claude stopped: {reason}");
        }
        let tour =
            GuidedTour::load_of(&Tour::path_for(&self.folders, &self.subject), &self.subject)?;
        let source = (self.pick_code_source)(&tour.pin.pinned_tips())?;
        let review = ReviewBuilder::new(&self.folders, &self.subject)
            .build(Tour::Guided(tour), source.as_ref())?;
        if !review.refresh_brief().is_empty() {
            anyhow::bail!("claude ended without refreshing the tour: {said}");
        }
        Ok(RepinnedTour {
            review,
            source: source.into(),
        })
    }
}
