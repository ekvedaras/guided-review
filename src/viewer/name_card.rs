//! The Definition card: what a name hovered in a diff line means at its commit, as the card draws
//! it. It is built once when a lookup lands, so drawing it parses and ranks nothing.

use std::ops::Range;
use std::sync::Arc;

use crate::file_path::FilePath;
use crate::navigation::{Definition, Lookup, Place, Reach, Usage, Word};

use super::code_link::CodeLink;
use super::highlight::Capture;
use super::jump::JumpTarget;
use super::name_highlights::NameHighlights;

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

/// A stretch of a card's code text: what it is, for the colour, and whether it is the hovered name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeRun {
    pub range: Range<usize>,
    pub capture: Option<Capture>,
    pub is_name: bool,
}

impl CodeRun {
    /// The captures cut around `name`, which gets a run of its own that keeps the capture it sat in.
    /// `captures` are in text order and do not overlap.
    pub fn compose(
        captures: &[(Range<usize>, Capture)],
        name: Option<&Range<usize>>,
    ) -> Arc<[Self]> {
        let mut runs: Vec<Self> = Vec::new();
        for (range, capture) in captures {
            let pieces = match name {
                Some(name) => vec![
                    range.start..range.end.min(name.start),
                    range.start.max(name.end)..range.end,
                ],
                None => vec![range.clone()],
            };
            runs.extend(
                pieces
                    .into_iter()
                    .filter(|piece| piece.start < piece.end)
                    .map(|range| Self {
                        range,
                        capture: Some(*capture),
                        is_name: false,
                    }),
            );
        }
        if let Some(name) = name {
            runs.push(Self {
                range: name.clone(),
                capture: captures
                    .iter()
                    .find(|(range, _)| range.start <= name.start && name.start < range.end)
                    .map(|(_, capture)| *capture),
                is_name: true,
            });
        }
        runs.sort_by_key(|run| run.range.start);
        runs.into()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefinitionRow {
    pub signature: Arc<str>,
    pub signature_runs: Arc<[CodeRun]>,
    pub doc: Option<Arc<str>>,
    /// `src/cart.ts:4`
    pub place: Arc<str>,
    /// The definition sits in a file the review changes.
    pub in_pr: bool,
    pub target: NameTarget,
}

/// A usage's line as the card shows it: the name is findable at a glance, even in a long line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snippet {
    pub text: Arc<str>,
    /// Where the name sits in `text`; `None` when the line does not hold it as a whole word.
    pub name: Option<Range<usize>>,
    /// The colours of `text` and the name's mark, in text order.
    pub runs: Arc<[CodeRun]>,
}

/// A name that starts past this many characters is shown from a few characters before it.
const SNIPPET_LEAD_AT_MOST: usize = 40;
const SNIPPET_LEAD_KEPT: usize = 8;
const SNIPPET_CHARS_AT_MOST: usize = 200;
const CUT: char = '…';

impl Snippet {
    pub fn of(line: &str, word: &str) -> Self {
        Self::built(line.trim(), word, &[])
    }

    /// `line` is trimmed, and `captures` are byte ranges into it, in order.
    pub fn highlighted(line: &str, word: &str, captures: &[(Range<usize>, Capture)]) -> Self {
        Self::built(line, word, captures)
    }

    fn built(line: &str, word: &str, captures: &[(Range<usize>, Capture)]) -> Self {
        let Some(at) = whole_word_at(line, word) else {
            let text = first_chars(line, SNIPPET_CHARS_AT_MOST);
            return Self {
                runs: CodeRun::compose(&cut_captures(captures, 0, text.len(), 0), None),
                text: text.into(),
                name: None,
            };
        };
        let before = line.get(..at).unwrap_or_default();
        let skipped = before.chars().count().saturating_sub(SNIPPET_LEAD_AT_MOST);
        let (start, lead) = if skipped == 0 {
            (0, String::new())
        } else {
            let keep_from = skipped + SNIPPET_LEAD_AT_MOST - SNIPPET_LEAD_KEPT;
            let start = line
                .char_indices()
                .nth(keep_from)
                .map_or(at, |(start, _)| start);
            (start, CUT.to_string())
        };
        let shown = first_chars(line.get(start..).unwrap_or_default(), SNIPPET_CHARS_AT_MOST);
        let name_at = lead.len() + at - start;
        let name = name_at..name_at + word.len();
        let captures = cut_captures(captures, start, shown.len(), lead.len());
        Self {
            runs: CodeRun::compose(&captures, Some(&name)),
            name: Some(name),
            text: format!("{lead}{shown}").into(),
        }
    }

    pub fn name_text(&self) -> Option<&str> {
        self.name.clone().and_then(|name| self.text.get(name))
    }
}

/// The captures that fall in `len` bytes from `start`, moved to where those bytes are shown after
/// `lead` bytes of text.
fn cut_captures(
    captures: &[(Range<usize>, Capture)],
    start: usize,
    len: usize,
    lead: usize,
) -> Vec<(Range<usize>, Capture)> {
    let end = start + len;
    captures
        .iter()
        .filter(|(range, _)| range.end > start && range.start < end)
        .map(|(range, capture)| {
            (
                range.start.max(start) - start + lead..range.end.min(end) - start + lead,
                *capture,
            )
        })
        .collect()
}

fn first_chars(text: &str, count: usize) -> &str {
    text.char_indices()
        .nth(count)
        .and_then(|(end, _)| text.get(..end))
        .unwrap_or(text)
}

fn whole_word_at(line: &str, word: &str) -> Option<usize> {
    let part_of_name = |c: char| c.is_alphanumeric() || c == '_' || c == '$';
    line.match_indices(word).find_map(|(at, found)| {
        let before = line.get(..at).and_then(|text| text.chars().next_back());
        let after = line
            .get(at + found.len()..)
            .and_then(|text| text.chars().next());
        (!before.is_some_and(part_of_name) && !after.is_some_and(part_of_name)).then_some(at)
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageLine {
    /// The line's number, for the gutter.
    pub number: Arc<str>,
    pub snippet: Snippet,
    pub target: NameTarget,
}

/// A file's usages as an IDE's references list groups them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageFile {
    pub path: Arc<str>,
    /// `src/app`; empty for a file in the root.
    pub folder: Arc<str>,
    /// `cart.ts`
    pub name: Arc<str>,
    /// How many usages the file holds, shown or not.
    pub count: Arc<str>,
    /// The file's first usage.
    pub target: NameTarget,
    /// The lines shown, of the ones within `USAGES_SHOWN` in all.
    pub lines: Vec<UsageLine>,
}

/// Its texts are cut when the lookup lands, so drawing the card builds none.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundNames {
    pub word: Word,
    /// "2 definitions · 629 usages in 41 files"; empty when nothing was found.
    pub counts: Arc<str>,
    /// Best match first.
    pub definitions: Vec<DefinitionRow>,
    /// Files with usages in the PR's files first, then by path.
    pub usages: Vec<UsageFile>,
    pub usages_not_shown: usize,
    pub notes: Vec<CardNote>,
    /// "and 6 more in 2 files"
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

    pub(super) fn of(
        lookup: &Lookup,
        hovered: &Place,
        review: &dyn PlacesOfReview,
        highlights: &NameHighlights,
    ) -> Self {
        let definitions: Vec<DefinitionRow> = lookup
            .definitions
            .iter()
            .map(|definition| definition_row(definition, review, highlights))
            .collect();
        let usages = usages_in_card_order(lookup, hovered, &|path| review.is_changed(path));
        let usages_found = usages.len();
        let by_file = by_file(&usages);
        let mut budget = USAGES_SHOWN;
        let mut files_not_shown = 0;
        let mut files = Vec::new();
        for group in &by_file {
            let shown = group.len().min(budget);
            budget -= shown;
            files_not_shown += usize::from(shown < group.len());
            if let Some(file) = usage_file(group, shown, &lookup.word, review, highlights) {
                files.push(file);
            }
        }
        let usages_not_shown = usages_found.saturating_sub(USAGES_SHOWN);
        let mut notes = Vec::new();
        if lookup.reach == Reach::ChangedFiles {
            notes.push(CardNote::UsagesFromChangedFilesOnly);
        }
        if lookup.cut_short {
            notes.push(CardNote::SearchCutShort);
        }
        Self::Found(FoundNames {
            word: lookup.word.clone(),
            counts: counts(definitions.len(), usages_found, by_file.len()).into(),
            definitions,
            usages: files,
            usages_not_shown,
            notes,
            more_usages: (usages_not_shown > 0).then(|| {
                format!(
                    "and {usages_not_shown} more in {}",
                    counted(files_not_shown, "file", "files")
                )
                .into()
            }),
        })
    }
}

/// The usages the card counts: those in the review's files first, then by path and line; the
/// hovered line is left out.
pub(super) fn usages_in_card_order<'a>(
    lookup: &'a Lookup,
    hovered: &Place,
    is_changed: &dyn Fn(&FilePath) -> bool,
) -> Vec<&'a Usage> {
    let mut usages: Vec<&Usage> = lookup
        .usages
        .iter()
        .filter(|usage| usage.place != *hovered)
        .collect();
    usages.sort_by_key(|usage| {
        (
            !is_changed(&usage.place.path),
            usage.place.path.as_str(),
            usage.place.line.get(),
        )
    });
    usages
}

fn by_file<'a>(sorted: &[&'a Usage]) -> Vec<Vec<&'a Usage>> {
    let mut groups: Vec<Vec<&Usage>> = Vec::new();
    for usage in sorted {
        match groups.last_mut() {
            Some(group)
                if group
                    .first()
                    .is_some_and(|first| first.place.path == usage.place.path) =>
            {
                group.push(usage);
            }
            _ => groups.push(vec![usage]),
        }
    }
    groups
}

fn usage_file(
    group: &[&Usage],
    shown: usize,
    word: &Word,
    review: &dyn PlacesOfReview,
    highlights: &NameHighlights,
) -> Option<UsageFile> {
    let first = group.first()?;
    if shown == 0 {
        return None;
    }
    let path = first.place.path.as_str();
    let (folder, name) = path.rsplit_once('/').unwrap_or(("", path));
    Some(UsageFile {
        path: path.into(),
        folder: folder.into(),
        name: name.into(),
        count: group.len().to_string().into(),
        target: review.target_of(&first.place),
        lines: group
            .iter()
            .take(shown)
            .map(|usage| UsageLine {
                number: usage.place.line.get().to_string().into(),
                snippet: Snippet::highlighted(
                    &usage.text,
                    word.as_str(),
                    highlights.usage(&usage.place),
                ),
                target: review.target_of(&usage.place),
            })
            .collect(),
    })
}

fn counted(count: usize, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}

fn counts(definitions: usize, usages: usize, files: usize) -> String {
    let mut parts = Vec::new();
    if definitions > 0 {
        parts.push(counted(definitions, "definition", "definitions"));
    }
    if usages > 0 {
        parts.push(format!(
            "{} in {}",
            counted(usages, "usage", "usages"),
            counted(files, "file", "files")
        ));
    }
    parts.join(" · ")
}

fn definition_row(
    definition: &Definition,
    review: &dyn PlacesOfReview,
    highlights: &NameHighlights,
) -> DefinitionRow {
    DefinitionRow {
        signature: Arc::clone(&definition.signature),
        signature_runs: CodeRun::compose(highlights.definition(&definition.place), None),
        doc: definition.doc.clone(),
        place: place_label(&definition.place),
        in_pr: review.is_changed(&definition.place.path),
        target: review.target_of(&definition.place),
    }
}

fn place_label(place: &Place) -> Arc<str> {
    format!("{}:{}", place.path, place.line.get()).into()
}
