//! The colours of the code a Definition card shows. The files are highlighted whole, like the
//! diff's, so a line is coloured by what surrounds it; only the files that have a shown row are
//! read, and the result is cut to the rows when the lookup lands.

use crate::navigation::Usage;
use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

use crate::code_source::CodeSource;
use crate::file_path::FilePath;
use crate::navigation::{Lookup, Place};
use crate::pull_request::Sha;
use crate::symbols::NO_ORIGIN;

use std::cell::OnceCell;

use super::highlight::{Capture, highlight_file_lines, highlight_snippet};

type Captures = Arc<[(Range<usize>, Capture)]>;

#[derive(Debug, Default)]
pub struct NameHighlights {
    /// By the definition's place: byte ranges into its signature.
    definitions: HashMap<Place, Captures>,
    /// By the usage's place: byte ranges into its trimmed line.
    usages: HashMap<Place, Captures>,
}

impl NameHighlights {
    pub fn none() -> Self {
        Self::default()
    }

    /// Colours the signatures and the first usages in card order (`USAGES_SHOWN` of them), reading
    /// each file once at `sha`. A file that cannot be read or has no language stays plain.
    pub fn of(
        lookup: &Lookup,
        hovered: &Place,
        is_changed: &dyn Fn(&FilePath) -> bool,
        code: &dyn CodeSource,
        sha: &Sha,
    ) -> Self {
        let shown = usages_in_card_order(lookup, hovered, is_changed);
        let shown = shown.iter().take(USAGES_SHOWN);
        let mut paths: Vec<&FilePath> = lookup
            .definitions
            .iter()
            .map(|definition| &definition.place.path)
            .chain(shown.clone().map(|usage| &usage.place.path))
            .collect();
        paths.sort();
        paths.dedup();
        let mut highlights = Self::none();
        for path in paths {
            let Some(file) = HighlightedFile::read(code, sha, path) else {
                continue;
            };
            for definition in lookup
                .definitions
                .iter()
                .filter(|definition| definition.place.path == *path)
            {
                highlights.definitions.insert(
                    definition.place.clone(),
                    file.signature(&definition.signature, &definition.signature_origin),
                );
            }
            for usage in shown.clone().filter(|usage| usage.place.path == *path) {
                if let Some(captures) = file.line(usage.place.line.get(), &usage.text) {
                    highlights.usages.insert(usage.place.clone(), captures);
                }
            }
        }
        highlights
    }

    /// For a snapshot scene's made-up lookup, whose text is no file: each text is coloured alone.
    pub fn staged(lookup: &Lookup) -> Self {
        let mut highlights = Self::none();
        for definition in &lookup.definitions {
            highlights.definitions.insert(
                definition.place.clone(),
                alone(&definition.place.path, &definition.signature),
            );
        }
        for usage in lookup.usages.iter() {
            highlights
                .usages
                .insert(usage.place.clone(), alone(&usage.place.path, &usage.text));
        }
        highlights
    }

    pub(super) fn definition(&self, place: &Place) -> &[(Range<usize>, Capture)] {
        self.definitions.get(place).map_or(&[], |captures| captures)
    }

    pub(super) fn usage(&self, place: &Place) -> &[(Range<usize>, Capture)] {
        self.usages.get(place).map_or(&[], |captures| captures)
    }
}

/// A snapshot scene's text has no file around it; it is coloured as a fragment of its language.
fn alone(path: &FilePath, text: &str) -> Captures {
    let extension = path
        .as_str()
        .rsplit_once('.')
        .map(|(_, extension)| extension);
    highlight_snippet(extension, text)
        .first()
        .map_or_else(Captures::default, |spans| {
            spans
                .iter()
                .map(|span| (span.range(), span.capture))
                .collect()
        })
}

/// One file at a commit, highlighted whole.
struct HighlightedFile {
    source: String,
    /// The spans of each line, byte ranges into that line.
    lines: Vec<Vec<super::highlight::Span>>,
    line_starts: Vec<usize>,
    /// Every span as byte ranges into `source`, built when a signature needs them.
    whole: OnceCell<Vec<(Range<usize>, Capture)>>,
}

impl HighlightedFile {
    fn read(code: &dyn CodeSource, sha: &Sha, path: &FilePath) -> Option<Self> {
        let source = code.file_at(sha, path).ok()?;
        let lines = highlight_file_lines(path.as_str(), &source)?;
        let line_starts = std::iter::once(0)
            .chain(source.match_indices('\n').map(|(at, _)| at + 1))
            .collect();
        Some(Self {
            source,
            lines,
            line_starts,
            whole: OnceCell::new(),
        })
    }

    /// The spans of line `number` (from 1) as ranges into its trimmed text; `None` when the line
    /// is not `text`, so a colour never lands on other text.
    fn line(&self, number: u32, text: &str) -> Option<Captures> {
        let index = usize::try_from(number).ok()?.checked_sub(1)?;
        let start = *self.line_starts.get(index)?;
        let end = self
            .line_starts
            .get(index + 1)
            .map_or(self.source.len(), |next| next - 1);
        let raw = self.source.get(start..end)?;
        if raw.trim() != text {
            return None;
        }
        let lead = raw.len() - raw.trim_start().len();
        Some(
            self.lines
                .get(index)?
                .iter()
                .filter_map(|span| {
                    let range = span.range();
                    let (from, to) = (
                        range.start.saturating_sub(lead),
                        range.end.saturating_sub(lead).min(text.len()),
                    );
                    (from < to).then_some((from..to, span.capture))
                })
                .collect(),
        )
    }

    /// The captures of a signature cut from this file: each byte takes the capture of the source
    /// byte it came from, and the space that joined two lines takes none.
    fn signature(&self, text: &str, origin: &[u32]) -> Captures {
        if origin.len() != text.len() {
            return Captures::default();
        }
        let whole = self.whole.get_or_init(|| {
            let mut whole: Vec<(Range<usize>, Capture)> = Vec::new();
            for (spans, start) in self.lines.iter().zip(&self.line_starts) {
                for span in spans {
                    let range = span.range();
                    whole.push((start + range.start..start + range.end, span.capture));
                }
            }
            whole.sort_by_key(|(range, _)| range.start);
            whole
        });
        let capture_at = |byte: usize| {
            let after = whole.partition_point(|(range, _)| range.start <= byte);
            whole
                .get(after.checked_sub(1)?)
                .filter(|(range, _)| byte < range.end)
                .map(|(_, capture)| *capture)
        };
        let mut runs: Vec<(Range<usize>, Capture)> = Vec::new();
        for (at, c) in text.char_indices() {
            let source_byte = origin.get(at).copied().filter(|byte| *byte != NO_ORIGIN);
            let Some(capture) = source_byte
                .and_then(|byte| usize::try_from(byte).ok())
                .and_then(capture_at)
            else {
                continue;
            };
            let end = at + c.len_utf8();
            match runs.last_mut() {
                Some((range, last)) if range.end == at && *last == capture => range.end = end,
                _ => runs.push((at..end, capture)),
            }
        }
        runs.into()
    }
}

/// How many usages the card lists; the rest are counted.
pub const USAGES_SHOWN: usize = 20;

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
