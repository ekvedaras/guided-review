//! The Definition card: what a name hovered in a diff line means at its commit, as the card draws
//! it. It is built once when a lookup lands, so drawing it parses and ranks nothing.

use std::sync::Arc;

use crate::file_path::FilePath;
use crate::navigation::{Definition, DefinitionKind, Lookup, Place, Reach, Usage, Word};

use super::code_link::CodeLink;
use super::jump::JumpTarget;

/// How many usages the card lists; the rest are counted.
pub const USAGES_SHOWN: usize = 20;

/// Where a click on a definition or a usage goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NameTarget {
    /// The place is shown in the tour: the reader jumps there, as the cmd+k list does.
    Tour(JumpTarget),
    /// Any other place opens through the `CodeOpener`, at the commit the lookup read.
    Code(CodeLink),
}

/// Why the card may show less than the repo holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardNote {
    /// The code is read from GitHub, so only the review's changed files were searched.
    UsagesFromChangedFilesOnly,
    /// More files hold the name than are parsed.
    SearchCutShort,
}

impl CardNote {
    pub const fn text(self) -> &'static str {
        match self {
            Self::UsagesFromChangedFilesOnly => "Usages from the PR's files only",
            Self::SearchCutShort => "Search cut short: too many files hold this name",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefinitionRow {
    pub kind: DefinitionKind,
    pub signature: Arc<str>,
    pub doc: Option<Arc<str>>,
    /// `src/cart.ts:4`
    pub place: Arc<str>,
    /// The definition sits in a file the review changes.
    pub in_pr: bool,
    pub target: NameTarget,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageRow {
    pub place: Arc<str>,
    pub text: Arc<str>,
    pub target: NameTarget,
}

/// Its texts are cut when the lookup lands, so drawing the card builds none.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundNames {
    pub word: Word,
    /// Best match first.
    pub definitions: Vec<DefinitionRow>,
    /// The first `USAGES_SHOWN`, those in the PR's files first.
    pub usages: Vec<UsageRow>,
    pub usages_not_shown: usize,
    pub notes: Vec<CardNote>,
    /// "DEFINITION", or "2 DEFINITIONS" when several match, so the reader knows the best one may be wrong.
    pub definitions_heading: Arc<str>,
    /// "3 USAGES"; `None` when there are none.
    pub usages_heading: Option<Arc<str>>,
    /// "and 6 more"
    pub more_usages: Option<Arc<str>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DefinitionCard {
    Loading {
        word: Word,
        /// "Looking up total…"
        says: Arc<str>,
    },
    Failed {
        word: Word,
        because: String,
        /// "Could not look up total: …"
        says: Arc<str>,
    },
    Found(FoundNames),
}

/// What the review knows of the places a lookup found.
pub(super) trait PlacesOfReview {
    fn is_changed(&self, path: &FilePath) -> bool;
    fn target_of(&self, place: &Place) -> NameTarget;
}

impl DefinitionCard {
    pub(super) fn loading(word: Word) -> Self {
        let says = format!("Looking up {word}…").into();
        Self::Loading { word, says }
    }

    pub(super) fn failed(word: Word, because: String) -> Self {
        let says = format!("Could not look up {word}: {because}").into();
        Self::Failed {
            word,
            because,
            says,
        }
    }

    pub(super) fn of(lookup: &Lookup, hovered: &Place, review: &dyn PlacesOfReview) -> Self {
        let definitions = lookup
            .definitions
            .iter()
            .map(|definition| definition_row(definition, review))
            .collect();
        let mut usages: Vec<&Usage> = lookup
            .usages
            .iter()
            .filter(|usage| usage.place != *hovered)
            .collect();
        usages.sort_by_key(|usage| !review.is_changed(&usage.place.path));
        let usages_found = usages.len();
        let usages_not_shown = usages_found.saturating_sub(USAGES_SHOWN);
        let usages = usages
            .into_iter()
            .take(USAGES_SHOWN)
            .map(|usage| UsageRow {
                place: place_label(&usage.place),
                text: Arc::clone(&usage.text),
                target: review.target_of(&usage.place),
            })
            .collect();
        let mut notes = Vec::new();
        if lookup.reach == Reach::ChangedFiles {
            notes.push(CardNote::UsagesFromChangedFilesOnly);
        }
        if lookup.cut_short {
            notes.push(CardNote::SearchCutShort);
        }
        let definitions_heading = match lookup.definitions.len() {
            0 | 1 => "DEFINITION".into(),
            count => format!("{count} DEFINITIONS").into(),
        };
        let usages_heading = match usages_found {
            0 => None,
            1 => Some("1 USAGE".into()),
            count => Some(format!("{count} USAGES").into()),
        };
        Self::Found(FoundNames {
            word: lookup.word.clone(),
            definitions,
            usages,
            usages_not_shown,
            notes,
            definitions_heading,
            usages_heading,
            more_usages: (usages_not_shown > 0)
                .then(|| format!("and {usages_not_shown} more").into()),
        })
    }
}

fn definition_row(definition: &Definition, review: &dyn PlacesOfReview) -> DefinitionRow {
    DefinitionRow {
        kind: definition.kind,
        signature: Arc::clone(&definition.signature),
        doc: definition.doc.clone(),
        place: place_label(&definition.place),
        in_pr: review.is_changed(&definition.place.path),
        target: review.target_of(&definition.place),
    }
}

fn place_label(place: &Place) -> Arc<str> {
    format!("{}:{}", place.path, place.line.get()).into()
}
