use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use super::review_thread::ThreadId;
use crate::tour::StepId;

/// The step each thread the reviewer started was written on. Two steps can show the same context row, and
/// the thread GitHub sends back has no step, so without this it goes to the first step showing the row.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub(super) struct WrittenOn(BTreeMap<ThreadId, StepId>);

impl WrittenOn {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn record(&mut self, thread: ThreadId, step: StepId) {
        self.0.insert(thread, step);
    }

    pub fn step_of(&self, thread: &ThreadId) -> Option<&StepId> {
        self.0.get(thread)
    }
}
