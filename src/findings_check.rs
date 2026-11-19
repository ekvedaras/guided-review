//! `check-findings`: the findings file's anchors must exist in the change, on the side they name.

use std::collections::HashSet;
use std::fmt;

use crate::findings::{Anchor, AnchorSide, FindingId, FindingsFile};
use crate::line_number::LineNumber;
use crate::line_range::LineRange;
use crate::pieces::ChangedFile;

#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    NotAChangedFile {
        file: String,
    },
    LinesNotChanged {
        file: String,
        side: AnchorSide,
        lines: [u32; 2],
    },
    LinesBackwards {
        file: String,
        side: AnchorSide,
        lines: [u32; 2],
    },
    IdTaken,
    BodyEmpty,
}

#[derive(Debug, Default)]
pub struct Report {
    pub findings: usize,
    pub errors: Vec<(FindingId, Error)>,
}

pub fn check(file: &FindingsFile, changed: &[ChangedFile]) -> Report {
    let mut report = Report {
        findings: file.findings.len(),
        errors: Vec::new(),
    };
    let mut seen = HashSet::new();
    for finding in &file.findings {
        let mut fail = |error| report.errors.push((finding.id.clone(), error));
        if !seen.insert(&finding.id) {
            fail(Error::IdTaken);
        }
        if finding.body.trim().is_empty() {
            fail(Error::BodyEmpty);
        }
        match &finding.anchor {
            Anchor::Change => {}
            Anchor::File { file } => {
                if !changed.iter().any(|c| c.path == *file) {
                    fail(Error::NotAChangedFile {
                        file: file.to_string(),
                    });
                }
            }
            Anchor::Lines {
                file,
                side,
                lines: LineRange { start, end },
            } => {
                let named = (file.to_string(), *side, [start.get(), end.get()]);
                let Some(changed_file) = changed.iter().find(|c| c.path == *file) else {
                    fail(Error::NotAChangedFile { file: named.0 });
                    continue;
                };
                if end < start {
                    fail(Error::LinesBackwards {
                        file: named.0,
                        side: named.1,
                        lines: named.2,
                    });
                } else if !touches_a_changed_line(changed_file, *side, *start..=*end) {
                    fail(Error::LinesNotChanged {
                        file: named.0,
                        side: named.1,
                        lines: named.2,
                    });
                }
            }
        }
    }
    report
}

/// A removed line is numbered at base and an added one at head, so a range sits in the diff when it
/// holds one of the side's changed lines.
fn touches_a_changed_line(
    file: &ChangedFile,
    side: AnchorSide,
    range: std::ops::RangeInclusive<LineNumber>,
) -> bool {
    file.hunks.iter().any(|hunk| {
        let lines = match side {
            AnchorSide::Base => &hunk.removed,
            AnchorSide::Head => &hunk.added,
        };
        lines.iter().any(|line| range.contains(&line.number))
    })
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAChangedFile { file } => write!(f, "{file} is not a changed file"),
            Self::LinesNotChanged { file, side, lines } => write!(
                f,
                "{file} {side} lines {}-{} are not changed lines, anchor on lines the diff shows or on the whole file",
                lines[0], lines[1]
            ),
            Self::LinesBackwards { file, side, lines } => write!(
                f,
                "{file} {side} lines {}-{} end before they start",
                lines[0], lines[1]
            ),
            Self::IdTaken => f.write_str("the id is taken by an earlier finding"),
            Self::BodyEmpty => f.write_str("the body is empty"),
        }
    }
}

impl fmt::Display for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (id, error) in &self.errors {
            writeln!(f, "error: finding \"{id}\": {error}")?;
        }
        let noun = if self.findings == 1 {
            "finding"
        } else {
            "findings"
        };
        let errors = if self.errors.len() == 1 {
            "error"
        } else {
            "errors"
        };
        writeln!(
            f,
            "{} {noun}, {} {errors}",
            self.findings,
            self.errors.len()
        )
    }
}
