//! The Definition card of a name hovered in a diff line: which commit it is read at, who answers,
//! and where a click on what the card lists goes.

use std::cell::OnceCell;
use std::sync::Arc;

use crate::file_path::FilePath;
use crate::navigation::{
    AnsweredBy, CodeNavigator, LanguageServers, Lookup, Place, ServerAnswer, is_dependency_file,
};
use crate::pieces::ChangedFile;
use crate::pull_request::RepoName;
use crate::viewer::code_link::CodeLinks;
use crate::viewer::jump::{JumpPlace, JumpTarget};
use crate::viewer::name_card::{DefinitionCard, NameTarget, PlacesOfReview};
use crate::viewer::name_desk::{HoveredName, Looked, NameKey, NameOutcome, NameWaiting, NameWork};
use crate::viewer::name_highlights::NameHighlights;
use crate::viewer::step_page::{LineLocation, PieceDiff, Side};

use super::{ReviewCore, Screen};

impl ReviewCore {
    /// The lookup to run off the UI thread for a name under the pointer, or `None` when it is on
    /// its way or known, or the line is in no changed file. The card reads `Loading` from now on.
    pub fn look_up_name(&mut self, hovered: &HoveredName) -> Option<NameWork> {
        let key = self.name_key(hovered)?;
        let changed = self
            .files
            .iter()
            .map(|file| file.path_on(hovered.at.side).clone())
            .collect();
        self.names.ask(key, changed)
    }

    /// Answers names through `navigator` from now on, with nothing kept of what was asked before.
    pub fn use_navigator(&mut self, navigator: Arc<dyn CodeNavigator>) {
        self.names.use_by_name(navigator);
    }

    /// Lets `adapter` answer names in `repo` once its reviewer says yes; `answer` is what they said
    /// before. The first hover of a name in a repo with no answer carries the question.
    pub fn use_language_servers(
        &mut self,
        adapter: Arc<dyn CodeNavigator>,
        repo: RepoName,
        answer: Option<ServerAnswer>,
    ) {
        self.names.use_language_servers(adapter, repo, answer);
    }

    /// Lets `servers` answer names in `repo` once its reviewer says yes, with the by-name lookup
    /// answering whatever they cannot.
    pub fn use_language_server_pool(
        &mut self,
        servers: &LanguageServers,
        repo: RepoName,
        answer: Option<ServerAnswer>,
    ) {
        let adapter = servers.navigator(repo.clone(), self.names.by_name());
        self.use_language_servers(adapter, repo, answer);
    }

    /// Takes the reviewer's answer to the card's question; returns the repo it is about, for the
    /// app to keep in the config. Nothing without a language server adapter.
    pub fn answer_language_servers(&mut self, answer: ServerAnswer) -> Option<RepoName> {
        self.names.answer(answer)
    }

    /// The card of a name that is still on its way says what the navigator waits on.
    pub fn name_waiting(&mut self, waiting: NameWaiting) {
        let NameWaiting { key, state } = waiting;
        if self.names.is_on_its_way(&key) {
            let card = DefinitionCard::waiting(key.word.clone(), &state);
            self.names.keep(key, card);
        }
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
            let highlights = NameHighlights::staged(&lookup);
            let answered_by = AnsweredBy::ByName { because: None };
            self.keep_card_of(
                key,
                Ok(Looked {
                    lookup,
                    answered_by,
                    highlights,
                }),
            );
        }
    }

    fn keep_card_of(&mut self, key: NameKey, result: Result<Looked, String>) {
        let mut card = match result {
            Ok(Looked {
                lookup,
                answered_by,
                highlights,
            }) => {
                let hovered = Place {
                    path: key.path.clone(),
                    line: key.line,
                };
                let places = ReviewPlaces::of(self, key.side);
                DefinitionCard::of(&lookup, &hovered, &places, &highlights, &answered_by)
            }
            Err(because) => DefinitionCard::failed(key.word.clone(), because),
        };
        if let DefinitionCard::Found(found) = &mut card {
            found.question = self.names.question_for(&key);
        }
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
        let sha = match hovered.at.side {
            Side::Head => self.head.clone(),
            Side::Base => self.base.clone(),
        };
        Some(NameKey {
            side: hovered.at.side,
            sha,
            path: file.path_on(hovered.at.side).clone(),
            line: hovered.at.line,
            column: hovered.column,
            word: hovered.word.clone(),
        })
    }
}

/// What the card needs to know of the review's own places, read once for a card.
struct ReviewPlaces<'a> {
    core: &'a ReviewCore,
    side: Side,
    tour_places: Vec<JumpTarget>,
    /// What each screen shows, in reading order; built on the first place in a changed file.
    screens: OnceCell<Vec<(Screen, Vec<PieceDiff>)>>,
}

impl<'a> ReviewPlaces<'a> {
    fn of(core: &'a ReviewCore, side: Side) -> Self {
        Self {
            core,
            side,
            tour_places: core.jump_targets(""),
            screens: OnceCell::new(),
        }
    }

    /// The changed file that is at `path` on this side of the review.
    fn changed_file_at(&self, path: &FilePath) -> Option<&'a ChangedFile> {
        self.core
            .files
            .iter()
            .find(|file| file.path_on(self.side) == path)
    }

    fn screens(&self) -> &[(Screen, Vec<PieceDiff>)] {
        self.screens.get_or_init(|| {
            let mut screens: Vec<(Screen, Vec<PieceDiff>)> = Vec::new();
            for target in &self.tour_places {
                if screens.iter().any(|(screen, _)| *screen == target.screen) {
                    continue;
                }
                let pieces = target
                    .screen
                    .shown_on()
                    .and_then(|shown_on| self.core.pieces_shown_on(shown_on));
                if let Some(pieces) = pieces {
                    screens.push((target.screen.clone(), pieces));
                }
            }
            screens
        })
    }

    /// The first screen that shows the line, landing on its row; `None` for a line no page shows.
    fn tour_target(&self, place: &Place) -> Option<JumpTarget> {
        let file = self.changed_file_at(&place.path)?;
        let wanted = LineLocation {
            path: file.path.clone(),
            side: self.side,
            line: place.line,
        };
        self.screens().iter().find_map(|(screen, pieces)| {
            let row = pieces
                .iter()
                .flat_map(PieceDiff::lines)
                .find(|row| row.shows(&wanted))?;
            self.tour_places
                .iter()
                .find(|target| {
                    target.screen == *screen
                        && matches!(&target.place, JumpPlace::File(path) if *path == file.path)
                })
                .map(|target| target.at_line(row.location.clone()))
        })
    }
}

impl PlacesOfReview for ReviewPlaces<'_> {
    fn is_changed(&self, path: &FilePath) -> bool {
        self.changed_file_at(path).is_some()
    }

    fn changes_dependencies(&self) -> bool {
        self.core
            .files
            .iter()
            .any(|file| is_dependency_file(&file.path))
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
