use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use super::review_state::{ReviewStateFile, TypedText};

/// The saves of one review. They happen in turns: a save that was made later wrote the newer
/// state, so an older one still waiting in the background must not write over it. The last save
/// that failed is kept until one goes through, so the reviewer learns that what they typed or
/// ticked is only in memory.
#[derive(Default)]
pub struct Saves {
    issued: AtomicU64,
    written: Mutex<u64>,
    typing_is_unsaved: AtomicBool,
    problem: Mutex<Option<String>>,
}

impl Saves {
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
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if turn < *written {
            return Ok(());
        }
        let result = write();
        *self
            .problem
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) =
            result.as_ref().err().map(|error| format!("{error:#}"));
        result?;
        *written = turn;
        Ok(())
    }

    pub(super) fn problem(&self) -> Option<String> {
        self.problem
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    pub(super) fn typed(&self) {
        self.typing_is_unsaved.store(true, Ordering::Relaxed);
    }

    pub(super) fn typing_saved(&self) {
        self.typing_is_unsaved.store(false, Ordering::Relaxed);
    }

    /// Says whether there was unsaved text, and counts it as on its way to disk.
    pub(super) fn take_typing_unsaved(&self) -> bool {
        self.typing_is_unsaved.swap(false, Ordering::Relaxed)
    }
}

/// What was typed, ready to be written by a thread that is not the UI's.
#[must_use = "the typed text reaches the file only when the save runs"]
pub struct TypingSave {
    file: ReviewStateFile,
    saves: Arc<Saves>,
    turn: u64,
    typed: TypedText,
}

impl TypingSave {
    pub(super) fn new(file: ReviewStateFile, saves: Arc<Saves>, typed: TypedText) -> Self {
        let turn = saves.next_turn();
        Self {
            file,
            saves,
            turn,
            typed,
        }
    }

    /// A failed save leaves the text unsaved, so the next save writes it again.
    pub fn run(self) -> anyhow::Result<()> {
        let Self {
            file,
            saves,
            turn,
            typed,
        } = self;
        let result = saves.write(turn, || file.update(|saved| saved.take_typed_text(typed)));
        if result.is_err() {
            saves.typed();
        }
        result
    }
}
