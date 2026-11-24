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
use crate::navigation::{FileSearch, Lookup, Lookups, Word};
use crate::pull_request::Sha;

use super::name_card::DefinitionCard;
use super::step_page::{LineLocation, Side};

/// A name under the pointer in a shown diff line. The location's path is the file's new path, as
/// everywhere in the viewer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HoveredName {
    pub at: LineLocation,
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
    pub fn under_pointer(at: LineLocation, code: &str, byte: usize) -> Option<NameUnderPointer> {
        Language::for_path(at.path.as_str())?;
        let range = name_range(code, byte)?;
        let word = Word::new(code.get(range.clone())?)?;
        Some(NameUnderPointer {
            name: Self { at, word },
            range,
        })
    }
}

/// The run of letters, digits and `_` the byte is on, or ends. A `$`, `->` or `::` is not part of it.
fn name_range(code: &str, byte: usize) -> Option<Range<usize>> {
    let is_name = |(_, c): &(usize, char)| c.is_alphanumeric() || *c == '_';
    if byte > code.len() {
        return None;
    }
    let chars: Vec<(usize, char)> = code.char_indices().collect();
    let under = chars.iter().rposition(|(start, _)| *start <= byte)?;
    let at = chars
        .get(under)
        .filter(|char| is_name(char))
        .map(|_| under)
        .or_else(|| {
            under
                .checked_sub(1)
                .filter(|before| chars.get(*before).is_some_and(is_name))
        })?;
    let first = chars
        .get(..=at)?
        .iter()
        .rposition(|char| !is_name(char))
        .map_or(0, |gap| gap + 1);
    let after = chars
        .get(at..)?
        .iter()
        .position(|char| !is_name(char))
        .map_or(chars.len(), |gap| at + gap);
    let start = chars.get(first)?.0;
    let end = chars.get(after).map_or(code.len(), |(end, _)| *end);
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
    pub(super) word: Word,
}

/// A lookup to run on a background thread.
pub struct NameWork {
    lookups: Arc<Lookups>,
    key: NameKey,
}

impl NameWork {
    pub fn run(self, source: &dyn CodeSource) -> NameOutcome {
        let result = self
            .lookups
            .look_up(source, &self.key.sha, &self.key.word, &self.key.path)
            .map_err(|error| format!("{error:#}"));
        NameOutcome {
            key: self.key,
            result,
        }
    }
}

/// What a `NameWork` brought back, for `Review::name_looked_up`.
pub struct NameOutcome {
    pub(super) key: NameKey,
    pub(super) result: Result<Lookup, String>,
}

pub(super) struct NameDesk {
    lookups: Arc<Lookups>,
    cards: HashMap<NameKey, Arc<DefinitionCard>>,
}

impl NameDesk {
    pub(super) fn new(search: FileSearch) -> Self {
        Self {
            lookups: Arc::new(Lookups::new(search)),
            cards: HashMap::new(),
        }
    }

    /// Work to run unless the name is on its way or known. A failed lookup is asked again.
    pub(super) fn ask(&mut self, key: NameKey) -> Option<NameWork> {
        if self
            .cards
            .get(&key)
            .is_some_and(|card| !matches!(**card, DefinitionCard::Failed { .. }))
        {
            return None;
        }
        self.cards.insert(
            key.clone(),
            Arc::new(DefinitionCard::Loading {
                word: key.word.clone(),
            }),
        );
        Some(NameWork {
            lookups: Arc::clone(&self.lookups),
            key,
        })
    }

    pub(super) fn keep(&mut self, key: NameKey, card: DefinitionCard) {
        self.cards.insert(key, Arc::new(card));
    }

    pub(super) fn card(&self, key: &NameKey) -> Option<Arc<DefinitionCard>> {
        self.cards.get(key).cloned()
    }
}
