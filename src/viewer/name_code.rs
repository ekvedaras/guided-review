//! The code the Definition card shows beside its lists: a definition's body, or the lines around a
//! usage, coloured like the diff. Built when the lookup lands, so drawing it parses nothing.

use std::ops::Range;
use std::sync::Arc;

use super::highlight::Capture;

/// A definition's body is cut after this many lines, with a "…" line.
pub const BODY_LINES_AT_MOST: usize = 40;
/// Lines shown above and below a usage.
pub const USAGE_CONTEXT_LINES: usize = 3;
const TAB_WIDTH: usize = 4;

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
pub struct PaneLine {
    /// The line's number in its file, for the gutter.
    pub number: Arc<str>,
    pub text: Arc<str>,
    pub runs: Arc<[CodeRun]>,
}

/// Some lines of one file at the commit the lookup read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodePane {
    /// `src/cart.ts:4`: where the first line is.
    pub place: Arc<str>,
    pub lines: Vec<PaneLine>,
    /// The body goes on past the last line shown.
    pub cut: bool,
    /// The line the pane is about, for a usage; its index in `lines`.
    pub marked: Option<usize>,
}

impl PaneLine {
    /// `raw` is the line without its newline; `captures` are byte ranges into it.
    pub(super) fn of(number: usize, raw: &str, captures: &[(Range<usize>, Capture)]) -> Self {
        let raw = raw.trim_end();
        let tabs = raw.chars().take_while(|c| *c == '\t').count();
        let moved = |byte: usize| {
            if byte >= tabs {
                byte + tabs * (TAB_WIDTH - 1)
            } else {
                byte * TAB_WIDTH
            }
        };
        let text = format!(
            "{}{}",
            " ".repeat(tabs * TAB_WIDTH),
            raw.get(tabs..).unwrap_or("")
        );
        let captures: Vec<(Range<usize>, Capture)> = captures
            .iter()
            .filter(|(range, _)| range.start < range.end && range.end <= raw.len())
            .map(|(range, capture)| (moved(range.start)..moved(range.end), *capture))
            .collect();
        Self {
            number: number.to_string().into(),
            runs: CodeRun::compose(&captures, None),
            text: text.into(),
        }
    }
}
