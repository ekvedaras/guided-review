//! The page of the open screen, laid out for reading. `Review::page` builds it once per change of the
//! review and keeps it, so every question about what the screen shows reads the same page.

use super::ask::Place;
use super::file_block::FileBlock;
use super::review::{ReviewCore, Screen};
use super::row_selection::{RowSelection, SelectedLines};
use super::step_page::DiffLine;
use crate::file_path::FilePath;

/// The content of one screen: a step's pieces in their file blocks, the safety net's, or none on the summary.
pub struct Page {
    screen: Screen,
    blocks: Vec<FileBlock>,
    images_to_load: Vec<FilePath>,
}

impl Page {
    pub(super) fn of(review: &ReviewCore) -> Self {
        let screen = review.screen().clone();
        let pieces = match &screen {
            Screen::Step(_) => review
                .current_step()
                .map(|page| page.pieces)
                .unwrap_or_default(),
            Screen::SafetyNet => review
                .safety_net()
                .map(|page| page.pieces)
                .unwrap_or_default(),
            Screen::Summary => Vec::new(),
        };
        let blocks = FileBlock::group(pieces);
        let mut images_to_load: Vec<FilePath> = Vec::new();
        for path in blocks
            .iter()
            .flat_map(|block| &block.pieces)
            .filter(|piece| piece.is_image)
            .map(|piece| piece.id.path())
        {
            if review.display_options().image_preview(path).is_none()
                && !images_to_load.contains(path)
            {
                images_to_load.push(path.clone());
            }
        }
        Self {
            screen,
            blocks,
            images_to_load,
        }
    }

    pub fn screen(&self) -> &Screen {
        &self.screen
    }

    /// Every line the screen shows, top to bottom.
    pub fn lines(&self) -> impl Iterator<Item = &DiffLine> + Clone {
        self.blocks.iter().flat_map(FileBlock::lines)
    }

    /// The lines a drag selected; `None` when where it started or where it is now is not on this page.
    pub fn selected(&self, selection: &RowSelection) -> Option<SelectedLines> {
        selection.resolve(self.lines())
    }

    /// What an Ask about the selected lines is about: the lines and the pieces of this page that hold them.
    pub fn place_of(&self, selected: &SelectedLines) -> Place {
        let step = match &self.screen {
            Screen::Step(id) => Some(id.clone()),
            Screen::Summary | Screen::SafetyNet => None,
        };
        Place::Lines {
            step,
            start: selected.first().clone(),
            end: selected.last().clone(),
            pieces: self
                .blocks
                .iter()
                .flat_map(|block| &block.pieces)
                .filter(|piece| piece.lines().any(|line| selected.contains(&line.location)))
                .map(|piece| piece.id.clone())
                .collect(),
        }
    }

    /// The pictures of the page that are not loaded yet, each once.
    pub fn images_to_load(&self) -> &[FilePath] {
        &self.images_to_load
    }
}
