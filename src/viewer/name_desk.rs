//! Looking up the names a reviewer hovers. A lookup reads files and parses them, so it runs off
//! the UI thread: the review hands out a `NameWork`, the app runs it with the code source and lands
//! its outcome. It is not one of the review's `Work` kinds, which are one at a time, since hovers
//! of different names are asked and answered side by side.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

use crate::code_source::CodeSource;
use crate::file_path::FilePath;
use crate::language::Language;
use crate::line_number::LineNumber;
use crate::navigation::{
    AnsweredBy, CodeNavigator, Column, Lookup, NavigationRequest, Place, Waiting, Word,
};
use crate::pull_request::Sha;

use super::highlight::Capture;
use super::name_card::DefinitionCard;
use super::name_highlights::NameHighlights;
use super::step_page::{LineLocation, Side};

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

pub(super) struct NameDesk {
    navigator: Arc<dyn CodeNavigator>,
    cards: HashMap<NameKey, Arc<DefinitionCard>>,
}

impl NameDesk {
    pub(super) fn new(navigator: Arc<dyn CodeNavigator>) -> Self {
        Self {
            navigator,
            cards: HashMap::new(),
        }
    }

    /// Work to run unless the name is on its way or known. A failed lookup is asked again.
    pub(super) fn ask(&mut self, key: NameKey, changed: Arc<[FilePath]>) -> Option<NameWork> {
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
            navigator: Arc::clone(&self.navigator),
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
