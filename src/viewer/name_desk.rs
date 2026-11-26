//! Looking up the names a reviewer hovers. A lookup reads files and parses them, so it runs off
//! the UI thread: the review hands out a `NameWork`, the app runs it with the code source and lands
//! its outcome. It is not one of the review's `Work` kinds, which are one at a time, since hovers
//! of different names are asked and answered side by side.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::{Arc, Mutex, PoisonError};

use crate::code_source::CodeSource;
use crate::codebase::Codebase;
use crate::file_path::FilePath;
use crate::language::Language;
use crate::line_number::LineNumber;
use crate::navigation::{
    AnsweredBy, CodeNavigator, Column, Lookup, NavigationRequest, Place, ServerAnswer,
    ServerOptIns, Waiting, Word,
};
use crate::pull_request::Sha;

use super::highlight::Capture;
use super::line_location::{LineLocation, Side};
use super::name_card::{DefinitionCard, ServerQuestion};
use super::name_highlights::NameHighlights;

/// A name under the pointer in a shown diff line. The location's path is the file's new path, as
/// everywhere in the viewer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HoveredName {
    pub at: LineLocation,
    /// Where the name starts in the line, so two names spelt alike on one line are two names.
    pub column: Column,
    pub word: Word,
}

/// The name under the pointer, and where it sits in the line's code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NameUnderPointer {
    pub name: HoveredName,
    /// Byte range of the name in the line's code.
    pub range: Range<usize>,
}

impl HoveredName {
    /// `highlights` are the line's, from tree-sitter over the whole file; while they are not ready
    /// a word the language spells as a keyword is no name.
    pub fn under_pointer(
        at: LineLocation,
        code: &str,
        highlights: &[(Range<usize>, Capture)],
        byte: usize,
    ) -> Option<NameUnderPointer> {
        let language = Language::for_path(at.path.as_str())?;
        let range = name_range(code, byte)?;
        let text = code.get(range.clone())?;
        let covering = highlights
            .iter()
            .find(|(span, _)| span.start <= range.start && range.end <= span.end);
        let is_name = match covering {
            Some((_, capture)) => can_be_a_name(*capture),
            None => !language.is_keyword(text),
        };
        if !is_name {
            return None;
        }
        let word = Word::new(text)?;
        let column = Column::try_from(range.start).ok()?;
        Some(NameUnderPointer {
            name: Self { at, column, word },
            range,
        })
    }
}

/// What tree-sitter calls a keyword, literal, comment or built-in is nothing a lookup can find.
fn can_be_a_name(capture: Capture) -> bool {
    !matches!(
        capture,
        Capture::Keyword
            | Capture::Comment
            | Capture::String
            | Capture::StringSpecial
            | Capture::Number
            | Capture::TypeBuiltin
            | Capture::ConstantBuiltin
            | Capture::VariableBuiltin
    )
}

/// The run of letters, digits and `_` the byte is on, or ends. A `$`, `->` or `::` is not part of it.
fn name_range(code: &str, byte: usize) -> Option<Range<usize>> {
    let is_name = |c: char| c.is_alphanumeric() || c == '_';
    if byte > code.len() {
        return None;
    }
    let at = (0..=byte).rev().find(|at| code.is_char_boundary(*at))?;
    let on_name = code
        .get(at..)?
        .chars()
        .next()
        .filter(|c| is_name(*c))
        .map(|_| at);
    let under = if let Some(under) = on_name {
        under
    } else {
        let (before, c) = code.get(..at)?.char_indices().next_back()?;
        is_name(c).then_some(before)?
    };
    let start = code
        .get(..under)?
        .char_indices()
        .rev()
        .take_while(|(_, c)| is_name(*c))
        .last()
        .map_or(under, |(start, _)| start);
    let end = code
        .get(under..)?
        .char_indices()
        .find(|(_, c)| !is_name(*c))
        .map_or(code.len(), |(after, _)| under + after);
    Some(start..end)
}

/// One lookup: the same name on the same line is one key, whatever the pointer did between.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) struct NameKey {
    pub(super) side: Side,
    /// The commit the line is read at: base for a removed line, else head (a local review's
    /// snapshot).
    pub(super) sha: Sha,
    /// The file's path at `sha`, which differs from the shown one for a renamed file's removed lines.
    pub(super) path: FilePath,
    pub(super) line: LineNumber,
    pub(super) column: Column,
    pub(super) word: Word,
}

/// A lookup to run on a background thread.
pub struct NameWork {
    navigator: Arc<dyn CodeNavigator>,
    key: NameKey,
    /// The review's files as the key's commit names them, to rank the shown usages.
    changed: Arc<[FilePath]>,
}

impl NameWork {
    pub fn run(self, source: &dyn CodeSource) -> NameOutcome {
        self.run_reporting(source, &|_| {})
    }

    /// `waiting` hears what the navigator waits on, for `Review::name_waiting`, before the
    /// outcome lands.
    pub fn run_reporting(
        self,
        source: &dyn CodeSource,
        waiting: &dyn Fn(NameWaiting),
    ) -> NameOutcome {
        let hovered = Place {
            path: self.key.path.clone(),
            line: self.key.line,
        };
        let request = NavigationRequest {
            sha: self.key.sha.clone(),
            at: hovered.clone(),
            column: self.key.column,
            word: self.key.word.clone(),
        };
        let result = self
            .navigator
            .navigate(source, &request, &|state| {
                waiting(NameWaiting {
                    key: self.key.clone(),
                    state,
                });
            })
            .map(|answer| {
                let lookup = answer.lookup;
                let highlights = NameHighlights::of(
                    &lookup,
                    &hovered,
                    &|path| self.changed.contains(path),
                    source,
                    &self.key.sha,
                );
                Looked {
                    lookup,
                    answered_by: answer.answered_by,
                    highlights,
                }
            })
            .map_err(|error| format!("{error:#}"));
        NameOutcome {
            key: self.key,
            result,
        }
    }
}

/// A lookup and the colours of what the card will show of it.
pub struct Looked {
    pub(super) lookup: Lookup,
    pub(super) answered_by: AnsweredBy,
    pub(super) highlights: NameHighlights,
}

/// What a name's lookup waits on, for `Review::name_waiting`.
#[derive(Debug, Clone)]
pub struct NameWaiting {
    pub(super) key: NameKey,
    pub(super) state: Waiting,
}

/// What a `NameWork` brought back, for `Review::name_looked_up`.
pub struct NameOutcome {
    pub(super) key: NameKey,
    pub(super) result: Result<Looked, String>,
}

/// A language server adapter that may answer names in one repo, once the reviewer has said yes.
struct ServerOptIn {
    adapter: Arc<dyn CodeNavigator>,
    codebase: Codebase,
    answers: Arc<dyn ServerOptIns>,
    /// What the answer was when the kept cards were made: they follow a change on the next ask.
    cards_made_under: Option<ServerAnswer>,
}

impl ServerOptIn {
    /// `None` until the reviewer is asked.
    fn answer(&self) -> Option<ServerAnswer> {
        self.answers.answer(&self.codebase)
    }
}

/// The answer of a review that shares it with no other.
struct OwnAnswer(Mutex<Option<ServerAnswer>>);

impl ServerOptIns for OwnAnswer {
    fn answer(&self, _codebase: &Codebase) -> Option<ServerAnswer> {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn answered(&self, _codebase: &Codebase, answer: ServerAnswer) {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = Some(answer);
    }
}

pub(super) struct NameDesk {
    /// Answers whenever no language server may.
    by_name: Arc<dyn CodeNavigator>,
    servers: Option<ServerOptIn>,
    cards: HashMap<NameKey, Arc<DefinitionCard>>,
}

impl NameDesk {
    pub(super) fn new(by_name: Arc<dyn CodeNavigator>) -> Self {
        Self {
            by_name,
            servers: None,
            cards: HashMap::new(),
        }
    }

    pub(super) fn by_name(&self) -> Arc<dyn CodeNavigator> {
        Arc::clone(&self.by_name)
    }

    /// Answers by name through `by_name` from now on, with nothing kept of what was asked before.
    pub(super) fn use_by_name(&mut self, by_name: Arc<dyn CodeNavigator>) {
        self.by_name = by_name;
        self.cards.clear();
    }

    /// `answer` is this review's own, shared with no other.
    pub(super) fn use_language_servers(
        &mut self,
        adapter: Arc<dyn CodeNavigator>,
        codebase: Codebase,
        answer: Option<ServerAnswer>,
    ) {
        let answers = Arc::new(OwnAnswer(Mutex::new(answer)));
        self.use_shared_language_servers(adapter, codebase, answers);
    }

    pub(super) fn use_shared_language_servers(
        &mut self,
        adapter: Arc<dyn CodeNavigator>,
        codebase: Codebase,
        answers: Arc<dyn ServerOptIns>,
    ) {
        let cards_made_under = answers.answer(&codebase);
        self.servers = Some(ServerOptIn {
            adapter,
            codebase,
            answers,
            cards_made_under,
        });
        self.cards.clear();
    }

    /// The adapter is only asked once the reviewer said yes for the repo; before that and after a
    /// no the by-name lookup answers.
    fn navigator_for(&self, key: &NameKey) -> Arc<dyn CodeNavigator> {
        match &self.servers {
            Some(servers)
                if servers.answer() == Some(ServerAnswer::Yes) && key.side == Side::Head =>
            {
                Arc::clone(&servers.adapter)
            }
            _ => Arc::clone(&self.by_name),
        }
    }

    /// Asked on a head line of a language a server answers, while the repo is not answered for.
    pub(super) fn question_for(&self, key: &NameKey) -> Option<ServerQuestion> {
        let servers = self.servers.as_ref()?;
        let language = Language::for_path(key.path.as_str()).filter(|_| key.side == Side::Head)?;
        if servers.answer().is_some() {
            return None;
        }
        ServerQuestion::about(&servers.codebase, language)
    }

    pub(super) fn answer(&mut self, answer: ServerAnswer) -> Option<Codebase> {
        let servers = self.servers.as_ref()?;
        servers.answers.answered(&servers.codebase, answer);
        let codebase = servers.codebase.clone();
        self.follow_answer();
        Some(codebase)
    }

    /// After a yes the cards are forgotten, to be asked of the adapter; after a no they stay, without
    /// the question.
    fn follow_answer(&mut self) {
        let Some(servers) = self.servers.as_mut() else {
            return;
        };
        let answer = servers.answer();
        if answer == servers.cards_made_under {
            return;
        }
        servers.cards_made_under = answer;
        match answer {
            Some(ServerAnswer::Yes) | None => self.cards.clear(),
            Some(ServerAnswer::No) => {
                for card in self.cards.values_mut() {
                    if let DefinitionCard::Found(found) = Arc::make_mut(card) {
                        found.question = None;
                    }
                }
            }
        }
    }

    /// Work to run unless the name is on its way or known. A failed lookup is asked again.
    pub(super) fn ask(&mut self, key: NameKey, changed: Arc<[FilePath]>) -> Option<NameWork> {
        self.follow_answer();
        if self
            .cards
            .get(&key)
            .is_some_and(|card| !matches!(**card, DefinitionCard::Failed { .. }))
        {
            return None;
        }
        self.cards.insert(
            key.clone(),
            Arc::new(DefinitionCard::loading(key.word.clone())),
        );
        Some(NameWork {
            navigator: self.navigator_for(&key),
            key,
            changed,
        })
    }

    /// Asked and not answered yet: a waiting state is no news to a card that has landed.
    pub(super) fn is_on_its_way(&self, key: &NameKey) -> bool {
        self.cards.get(key).is_some_and(|card| {
            matches!(
                **card,
                DefinitionCard::Loading { .. } | DefinitionCard::Waiting { .. }
            )
        })
    }

    pub(super) fn keep(&mut self, key: NameKey, card: DefinitionCard) {
        self.cards.insert(key, Arc::new(card));
    }

    pub(super) fn card(&self, key: &NameKey) -> Option<Arc<DefinitionCard>> {
        self.cards.get(key).cloned()
    }
}
