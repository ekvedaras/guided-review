use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use super::review_state::{ReviewStateFile, TypedText};

/// Saves of one review happen in turns. A save that was made later wrote the newer state, so an
/// older one still waiting in the background must not write over it.
#[derive(Default)]
pub struct SaveOrder {
    issued: AtomicU64,
    written: Mutex<u64>,
}

impl SaveOrder {
    pub fn next_turn(&self) -> u64 {
        self.issued.fetch_add(1, Ordering::Relaxed) + 1
    }

    pub fn write(
        &self,
        turn: u64,
        write: impl FnOnce() -> anyhow::Result<()>,
    ) -> anyhow::Result<()> {
        let mut written = self
            .written
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if turn < *written {
            return Ok(());
        }
        write()?;
        *written = turn;
        Ok(())
    }
}

/// What was typed, ready to be written by a thread that is not the UI's.
pub struct TypingSave {
    file: ReviewStateFile,
    order: Arc<SaveOrder>,
    turn: u64,
    typed: TypedText,
}

impl TypingSave {
    pub(super) fn new(file: ReviewStateFile, order: Arc<SaveOrder>, typed: TypedText) -> Self {
        let turn = order.next_turn();
        Self {
            file,
            order,
            turn,
            typed,
        }
    }

    pub fn run(self) -> anyhow::Result<()> {
        let Self {
            file,
            order,
            turn,
            typed,
        } = self;
        order.write(turn, || file.update(|saved| saved.take_typed_text(typed)))
    }
}
