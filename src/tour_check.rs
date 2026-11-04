use std::fmt;

use crate::coverage;
use crate::language::Language;
use crate::pieces::{ChangedFile, PieceId};
use crate::tour::{PieceRef, Step, StepId, Tour};

pub const MAX_SUMMARY_BULLETS: usize = 3;
pub const MAX_WORDS_PER_BULLET: usize = 12;
pub const MAX_WHY_LINES: usize = 2;
pub const MAX_PIECES_PER_STEP: usize = 8;
pub const MAX_TOP_LEVEL_STEPS: usize = 7;
const MAX_FILES_LISTED: usize = 3;

#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    UnknownPiece { step: StepId, pointer: PieceRef },
}

#[derive(Debug, PartialEq, Eq)]
pub enum Warning {
    Uncovered(String),
    LinesUncovered {
        piece: String,
        lines: usize,
    },
    LinesOnParsableFile {
        step: StepId,
        file: String,
        lines: [u32; 2],
    },
    SummaryTooLong(usize),
    BulletTooLong {
        words: usize,
        bullet: String,
    },
    WhyTooLong {
        step: StepId,
        lines: usize,
    },
    TooManyPieces {
        step: StepId,
        pieces: usize,
    },
    TooManyTopLevelSteps(usize),
}

#[derive(Debug, Default)]
pub struct Report {
    pub errors: Vec<Error>,
    pub warnings: Vec<Warning>,
    pub rest_notes: Vec<String>,
}

pub fn check(tour: &Tour, files: &[ChangedFile]) -> Report {
    let mut report = Report::default();

    for step in tour.steps() {
        check_step(step, files, &mut report);
    }
    for part in coverage::uncovered(tour, files) {
        let piece = part.piece.id.to_string();
        report.warnings.push(if part.is_whole() {
            Warning::Uncovered(piece)
        } else {
            Warning::LinesUncovered {
                piece,
                lines: part.removed_lines.len() + part.added_lines.len(),
            }
        });
    }
    if tour.summary.len() > MAX_SUMMARY_BULLETS {
        report
            .warnings
            .push(Warning::SummaryTooLong(tour.summary.len()));
    }
    for bullet in &tour.summary {
        let words = bullet.split_whitespace().count();
        if words > MAX_WORDS_PER_BULLET {
            report.warnings.push(Warning::BulletTooLong {
                words,
                bullet: bullet.clone(),
            });
        }
    }
    report.rest_notes = coverage::rest_taken(tour, files)
        .iter()
        .map(rest_note)
        .collect();
    if tour.steps.len() > MAX_TOP_LEVEL_STEPS {
        report
            .warnings
            .push(Warning::TooManyTopLevelSteps(tour.steps.len()));
    }
    report
}

fn rest_note(taken: &coverage::RestTaken) -> String {
    let listed: Vec<&str> = taken.files.iter().copied().take(MAX_FILES_LISTED).collect();
    let more = taken.files.len() - listed.len();
    format!(
        "rest \"{}\" takes {} in {}{}{}{}",
        taken.folder,
        counted(taken.pieces, "piece"),
        counted(taken.files.len(), "file"),
        if listed.is_empty() { "" } else { ": " },
        listed.join(", "),
        if more > 0 {
            format!(", +{more} more")
        } else {
            String::new()
        }
    )
}

fn check_step(step: &Step, files: &[ChangedFile], report: &mut Report) {
    let mut pieces = 0;
    for pointer in &step.pieces {
        if let PieceRef::Lines { file, lines } = pointer
            && Language::for_path(file).is_some()
        {
            report.warnings.push(Warning::LinesOnParsableFile {
                step: step.id.clone(),
                file: file.clone(),
                lines: *lines,
            });
        }
        match coverage::named_pieces(pointer, files) {
            Some(named) => pieces += named.len(),
            None => report.errors.push(Error::UnknownPiece {
                step: step.id.clone(),
                pointer: pointer.clone(),
            }),
        }
    }
    if pieces > MAX_PIECES_PER_STEP {
        report.warnings.push(Warning::TooManyPieces {
            step: step.id.clone(),
            pieces,
        });
    }
    let lines = step.why.lines().count();
    if lines > MAX_WHY_LINES {
        report.warnings.push(Warning::WhyTooLong {
            step: step.id.clone(),
            lines,
        });
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownPiece { step, pointer } => {
                write!(f, "step \"{step}\": unknown piece {}", Pointing(pointer))
            }
        }
    }
}

impl fmt::Display for Warning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Uncovered(piece) => write!(f, "no step covers {piece}"),
            Self::LinesUncovered { piece, lines } => {
                write!(
                    f,
                    "{} of {piece} are in no step",
                    counted(*lines, "changed line")
                )
            }
            Self::LinesOnParsableFile { step, file, lines } => write!(
                f,
                "step \"{step}\": {file} lines {}-{}: the tool can parse this file, use file + symbol",
                lines[0], lines[1]
            ),
            Self::SummaryTooLong(bullets) => write!(
                f,
                "summary has {bullets} bullets, keep it to {MAX_SUMMARY_BULLETS}"
            ),
            Self::BulletTooLong { words, bullet } => write!(
                f,
                "summary bullet has {words} words, keep it to about {MAX_WORDS_PER_BULLET}: {bullet}"
            ),
            Self::WhyTooLong { step, lines } => write!(
                f,
                "step \"{step}\": why has {lines} lines, keep it to {MAX_WHY_LINES}"
            ),
            Self::TooManyPieces { step, pieces } => write!(
                f,
                "step \"{step}\": {pieces} pieces, split it into steps of at most {MAX_PIECES_PER_STEP}"
            ),
            Self::TooManyTopLevelSteps(steps) => write!(
                f,
                "{steps} top-level steps, nest details to keep it to {MAX_TOP_LEVEL_STEPS}"
            ),
        }
    }
}

impl fmt::Display for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for error in &self.errors {
            writeln!(f, "error: {error}")?;
        }
        for warning in &self.warnings {
            writeln!(f, "warning: {warning}")?;
        }
        for note in &self.rest_notes {
            writeln!(f, "note: {note}")?;
        }
        writeln!(
            f,
            "{}, {}",
            counted(self.errors.len(), "error"),
            counted(self.warnings.len(), "warning")
        )
    }
}

fn counted(count: usize, noun: &str) -> String {
    format!("{count} {noun}{}", if count == 1 { "" } else { "s" })
}

struct Pointing<'a>(&'a PieceRef);

impl fmt::Display for Pointing<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            PieceRef::Symbol { file, symbol } => write!(
                f,
                "{}",
                PieceId {
                    path: file.clone(),
                    symbol: Some(symbol.clone())
                }
            ),
            PieceRef::Lines { file, lines } => write!(f, "{file} lines {}-{}", lines[0], lines[1]),
            PieceRef::WholeFile { file } => f.write_str(file),
            PieceRef::Rest { rest } => write!(f, "rest {rest}"),
        }
    }
}
