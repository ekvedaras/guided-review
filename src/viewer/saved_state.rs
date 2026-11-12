use std::fmt;
use std::ops::Deref;
use std::sync::Arc;

use super::review_state::{ReviewState, ReviewStateFile};
use super::typing_save::{Saves, TypingSave};

/// The review's state as its file keeps it. Anyone may read it; it changes only through `change`,
/// `type_in` or `record`, and none of them writes while a re-pin runs: the re-pin writes the file
/// from another thread and this review is replaced when it is done, so a save now would write
/// over the re-pin's record.
pub(super) struct SavedState {
    state: ReviewState,
    file: ReviewStateFile,
    saves: Arc<Saves>,
    repin: RepinGuard,
}

#[derive(Debug)]
pub struct RepinRunning;

impl fmt::Display for RepinRunning {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter
            .write_str("the tour is being updated to the new commits; try again when it is done")
    }
}

impl std::error::Error for RepinRunning {}

#[derive(Default)]
struct RepinGuard {
    running: bool,
    /// A refused change is named to the reviewer until the re-pin ends.
    refused: bool,
}

impl SavedState {
    pub(super) fn new(state: ReviewState, file: ReviewStateFile) -> Self {
        Self {
            state,
            file,
            saves: Arc::default(),
            repin: RepinGuard::default(),
        }
    }

    /// The reviewer's change: refused while a re-pin runs, so nothing changes; else made and saved at once.
    pub(super) fn change<T>(
        &mut self,
        change: impl FnOnce(&mut ReviewState) -> T,
    ) -> anyhow::Result<T> {
        self.refuse_while_repinning()?;
        let changed = change(&mut self.state);
        self.save()?;
        Ok(changed)
    }

    /// A keystroke: refused while a re-pin runs; else made at once and saved by the next `typing_save`.
    pub(super) fn type_in<T>(
        &mut self,
        change: impl FnOnce(&mut ReviewState) -> T,
    ) -> anyhow::Result<T> {
        self.refuse_while_repinning()?;
        let changed = change(&mut self.state);
        self.saves.typed();
        Ok(changed)
    }

    /// Made even while a re-pin runs, for what already happened elsewhere (on GitHub, in the tour
    /// agent) and must not happen twice; only the save is refused then.
    pub(super) fn record<T>(
        &mut self,
        change: impl FnOnce(&mut ReviewState) -> T,
    ) -> anyhow::Result<T> {
        let changed = change(&mut self.state);
        self.refuse_while_repinning()?;
        self.save()?;
        Ok(changed)
    }

    /// For what the review keeps outside its state but would lose all the same when the re-pin replaces it.
    pub(super) fn refuse_while_repinning(&mut self) -> anyhow::Result<()> {
        if self.repin.running {
            self.repin.refused = true;
            Err(RepinRunning.into())
        } else {
            Ok(())
        }
    }

    /// Text whose save failed goes to disk with the next `typing_save`.
    pub(super) fn save_with_next_typing(&self) {
        self.saves.typed();
    }

    pub(super) fn start_repin(&mut self) {
        self.repin.running = true;
    }

    pub(super) fn repin_failed(&mut self) {
        self.repin = RepinGuard::default();
    }

    pub(super) fn refused_while_repinning(&self) -> bool {
        self.repin.refused
    }

    /// Why the last save failed, until one goes through.
    pub(super) fn not_saved(&self) -> Option<String> {
        self.saves.problem()
    }

    pub(super) fn file(&self) -> &ReviewStateFile {
        &self.file
    }

    /// The save of what was typed since the last save, to run in the background; `None` when nothing is unsaved.
    pub(super) fn typing_save(&self) -> Option<TypingSave> {
        if !self.saves.take_typing_unsaved() {
            return None;
        }
        Some(TypingSave::new(
            self.file.clone(),
            Arc::clone(&self.saves),
            self.state.typed_text(),
        ))
    }

    /// The viewer owns ticks and viewed sections; asks are saved by the Ask panel, so only
    /// these are written into a freshly loaded file.
    fn save(&self) -> anyhow::Result<()> {
        let turn = self.saves.next_turn();
        self.saves.write(turn, || {
            self.file
                .update(|saved| saved.take_viewer_sections(&self.state))
        })?;
        self.saves.typing_saved();
        Ok(())
    }
}

impl Deref for SavedState {
    type Target = ReviewState;

    fn deref(&self) -> &ReviewState {
        &self.state
    }
}
