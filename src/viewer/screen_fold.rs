//! Open all pieces / Fold all pieces: what the step toolbar's fold button does, and the same on a
//! basic tour's file screen, whose boxes fold on their own with no step to fold.

use std::collections::BTreeSet;

use super::jump::PaletteAction;
use super::page::{Page, PageRow};
use super::review::{Review, Screen};
use crate::file_path::FilePath;
use crate::tour::StepId;

/// What opens or folds all the pieces of the open screen at once.
pub enum ScreenFold {
    /// The toolbar's button: the step's own fold.
    Step { step: StepId, is_folded: bool },
    /// A file screen has no step fold; its boxes are flipped one by one.
    Boxes {
        screen: Screen,
        paths: Vec<FilePath>,
        is_folded: bool,
    },
}

impl ScreenFold {
    fn is_folded(&self) -> bool {
        match self {
            Self::Step { is_folded, .. } | Self::Boxes { is_folded, .. } => *is_folded,
        }
    }

    fn action(&self) -> PaletteAction {
        if self.is_folded() {
            PaletteAction::OpenAllPieces
        } else {
            PaletteAction::FoldAllPieces
        }
    }
}

impl Page {
    /// `None` where the screen has nothing to open or fold all at once.
    pub fn screen_fold(&self) -> Option<ScreenFold> {
        let toolbar = self.rows().iter().find_map(|row| match row {
            PageRow::Toolbar(toolbar) => Some(toolbar),
            _ => None,
        })?;
        if let Some(fold) = &toolbar.fold {
            return Some(ScreenFold::Step {
                step: fold.step.clone(),
                is_folded: fold.is_folded,
            });
        }
        let boxes: Vec<_> = self
            .rows()
            .iter()
            .filter_map(|row| match row {
                PageRow::PieceHeader(header) => header.fold.as_ref(),
                _ => None,
            })
            .filter(|fold| matches!(fold.screen, Screen::File(_)))
            .collect();
        let screen = boxes.first()?.screen.clone();
        let is_folded = boxes.iter().all(|fold| fold.is_folded);
        let paths: BTreeSet<_> = boxes.iter().map(|fold| fold.path.clone()).collect();
        Some(ScreenFold::Boxes {
            screen,
            paths: paths.into_iter().collect(),
            is_folded,
        })
    }
}

impl Review {
    /// The palette's entry for it, offered only where the screen has the toggle.
    pub(super) fn screen_fold_action(&self) -> Option<PaletteAction> {
        self.page().screen_fold().as_ref().map(ScreenFold::action)
    }

    /// `O`: opens every piece of the screen, or folds them again once all are open. Says whether it
    /// did: not on a screen with no fold toggle.
    pub fn toggle_screen_fold(&mut self) -> bool {
        let Some(fold) = self.page().screen_fold() else {
            return false;
        };
        match fold {
            ScreenFold::Step { step, .. } => self.toggle_fold(&step),
            ScreenFold::Boxes {
                screen,
                paths,
                is_folded,
            } => self.set_boxes_folded(&screen, &paths, !is_folded),
        }
        true
    }
}
