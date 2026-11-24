//! The Definition card of a name hovered in a diff line: which commit it is read at, who answers,
//! and where a click on what the card lists goes.

use crate::navigation::{Lookup, Place};
use crate::pieces::ChangedFile;
use crate::viewer::code_link::CodeLinks;
use crate::viewer::jump::{JumpPlace, JumpTarget};
use crate::viewer::name_card::{DefinitionCard, NameTarget, PlacesOfReview};
use crate::viewer::name_desk::{HoveredName, NameKey, NameOutcome, NameWork};
use crate::viewer::step_page::Side;

use std::sync::Arc;

use super::ReviewCore;

impl ReviewCore {
    /// The lookup to run off the UI thread for a name under the pointer, or `None` when it is on
    /// its way or known, or the line is in no changed file. The card reads `Loading` from now on.
    pub fn look_up_name(&mut self, hovered: &HoveredName) -> Option<NameWork> {
        let key = self.name_key(hovered)?;
        self.names.ask(key)
    }

    /// Takes in what a `NameWork` brought back and makes the card of it.
    pub fn name_looked_up(&mut self, outcome: NameOutcome) {
        let NameOutcome { key, result } = outcome;
        self.keep_card_of(key, result);
    }

    /// A snapshot scene's lookup, landed as if it had run, for a name the review does not have to
    /// hold in a real file.
    pub fn stage_looked_up_name(&mut self, hovered: &HoveredName, lookup: Lookup) {
        if let Some(key) = self.name_key(hovered) {
            self.keep_card_of(key, Ok(lookup));
        }
    }

    fn keep_card_of(&mut self, key: NameKey, result: Result<Lookup, String>) {
        let card = match result {
            Ok(lookup) => {
                let hovered = Place {
                    path: key.path.clone(),
                    line: key.line,
                };
                let places = ReviewPlaces::of(self, key.side);
                DefinitionCard::of(&lookup, &hovered, &places)
            }
            Err(because) => DefinitionCard::Failed {
                word: key.word.clone(),
                because,
            },
        };
        self.names.keep(key, card);
    }

    /// `None` until the name is first asked for. Built once, so drawing it again costs nothing.
    pub fn definition_card(&self, hovered: &HoveredName) -> Option<Arc<DefinitionCard>> {
        self.names.card(&self.name_key(hovered)?)
    }

    fn name_key(&self, hovered: &HoveredName) -> Option<NameKey> {
        let file = self
            .files
            .iter()
            .find(|file| file.path == hovered.at.path)?;
        let (sha, path) = match hovered.at.side {
            Side::Head => (self.head.clone(), file.path.clone()),
            Side::Base => (
                self.base.clone(),
                file.old_path.clone().unwrap_or_else(|| file.path.clone()),
            ),
        };
        Some(NameKey {
            side: hovered.at.side,
            sha,
            path,
            line: hovered.at.line,
            word: hovered.word.clone(),
        })
    }
}

/// What the card needs to know of the review's own places, read once for a card.
struct ReviewPlaces<'a> {
    core: &'a ReviewCore,
    side: Side,
    tour_places: Vec<JumpTarget>,
}

impl<'a> ReviewPlaces<'a> {
    fn of(core: &'a ReviewCore, side: Side) -> Self {
        Self {
            core,
            side,
            tour_places: core.jump_targets(""),
        }
    }

    /// The changed file that is at `path` on this side of the review.
    fn changed_file_at(&self, path: &crate::file_path::FilePath) -> Option<&'a ChangedFile> {
        self.core.files.iter().find(|file| match self.side {
            Side::Head => file.path == *path,
            Side::Base => file.old_path.as_ref().unwrap_or(&file.path) == path,
        })
    }

    fn tour_target(&self, place: &Place) -> Option<JumpTarget> {
        let file = self.changed_file_at(&place.path)?;
        let line = usize::try_from(place.line.get()).ok()?;
        let in_piece = (self.side == Side::Head)
            .then(|| {
                file.pieces.iter().find(|piece| {
                    piece
                        .head_lines
                        .as_ref()
                        .is_some_and(|lines| lines.contains(&line))
                })
            })
            .flatten();
        let shown = |wanted: JumpPlace| {
            self.tour_places
                .iter()
                .find(|target| target.place == wanted)
                .cloned()
        };
        in_piece
            .and_then(|piece| shown(JumpPlace::Piece(piece.id.clone())))
            .or_else(|| shown(JumpPlace::File(file.path.clone())))
    }
}

impl PlacesOfReview for ReviewPlaces<'_> {
    fn is_changed(&self, path: &crate::file_path::FilePath) -> bool {
        self.changed_file_at(path).is_some()
    }

    fn target_of(&self, place: &Place) -> NameTarget {
        match self.tour_target(place) {
            Some(target) => NameTarget::Tour(target),
            None => NameTarget::Code(
                CodeLinks {
                    host: &self.core.code_host,
                    base: &self.core.base,
                    head: &self.core.head,
                }
                .to_place(self.side, &place.path, place.line),
            ),
        }
    }
}
