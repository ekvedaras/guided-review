use std::fmt::Write;

use crate::pieces::{ChangedFile, PieceId};
use crate::tour::{BasicTour, GuidedTour, Tour};

use std::collections::HashMap;

use super::ask::{Place, ShownOn};
use super::code_link::CodeLinks;
use super::diff_display::{DiffDisplay, FilesAtHead, MoreContext};
use super::highlight::Highlights;
use super::row_selection::RowSelection;
use super::step_page::{
    DiffLine, FilePage, LineLocation, PieceDiff, SafetyNetPage, Side, StepPage,
};

pub(super) fn system_prompt(pr: &str) -> String {
    format!(
        "You help a reviewer understand one place in the pull request {pr}. \
         You only read: you cannot change code or the tour. \
         Each question starts with pointers and the diff lines the reviewer is looking at. \
         To read more of the changed code, run `guided-review show {pr} <piece ids> --around`; \
         piece ids look like `<file>#<symbol>` or `<file>`. \
         Answer briefly and concretely, and name files and lines."
    )
}

pub(super) fn context_for(
    tour: &Tour,
    files: &[ChangedFile],
    links: &CodeLinks,
    place: &Place,
) -> Option<String> {
    match tour {
        Tour::Guided(guided) => guided_context(guided, files, links, place),
        Tour::Basic(basic) => basic_context(basic, files, links, place),
    }
}

/// A basic tour has no steps to point at: the AI is told so, and gets the changed files or the lines.
fn basic_context(
    tour: &BasicTour,
    files: &[ChangedFile],
    links: &CodeLinks,
    place: &Place,
) -> Option<String> {
    let mut text = format!(
        "PR: {}\nBase commit: {}\nHead commit: {}\n\
         The reviewer reads this PR as a basic tour: its changed files one by one, with no written walkthrough.\n",
        tour.pin.subject_key(),
        tour.pin.base,
        tour.pin.head
    );
    let plain = ChangedLinesOnly::default();
    let display = plain.display(links);
    match place {
        Place::Tour => {
            writeln!(text, "Place: the whole PR \"{}\"", tour.pr_title).ok()?;
            writeln!(text, "Changed files:").ok()?;
            for file in files {
                writeln!(text, "- {}", file.path).ok()?;
            }
        }
        Place::File { path } => {
            writeln!(text, "Place: the file {path}").ok()?;
            let file = files.iter().find(|file| file.path == *path)?;
            for piece in FilePage::of(file, files, &display).pieces {
                piece_lines(&mut text, &piece)?;
            }
        }
        Place::Lines {
            shown_on: ShownOn::File { path },
            start,
            end,
            pieces,
        } => {
            writeln!(text, "Place: a few lines of the file {path}").ok()?;
            let file = files.iter().find(|file| file.path == *path)?;
            let shown = FilePage::of(file, files, &display).pieces;
            selected_lines(&mut text, &shown, start, end, pieces)?;
        }
        Place::Step { .. }
        | Place::Piece { .. }
        | Place::Lines {
            shown_on: ShownOn::Step { .. } | ShownOn::SafetyNet,
            ..
        } => return None,
    }
    Some(text)
}

fn guided_context(
    tour: &GuidedTour,
    files: &[ChangedFile],
    links: &CodeLinks,
    place: &Place,
) -> Option<String> {
    let mut text = pointers(tour);
    let plain = ChangedLinesOnly::default();
    let display = plain.display(links);
    match place {
        Place::Tour => {
            writeln!(text, "Place: the whole tour \"{}\"", tour.title).ok()?;
            for bullet in &tour.summary {
                writeln!(text, "- {bullet}").ok()?;
            }
            writeln!(text, "Steps:").ok()?;
            for step in tour.steps() {
                writeln!(text, "- {}: {}", step.id, step.title).ok()?;
            }
        }
        Place::Step { step: id } => {
            let step = tour.find_step(id)?;
            step_heading(&mut text, step.title.as_str(), step.why.as_str(), "step")?;
            for piece in StepPage::of(step, tour, files, &display, false).pieces {
                piece_lines(&mut text, &piece)?;
            }
        }
        Place::Piece { step, piece } => {
            let step = tour.find_step(step)?;
            step_heading(&mut text, &step.title, &step.why, "piece")?;
            let page = StepPage::of(step, tour, files, &display, false);
            let piece = page
                .pieces
                .iter()
                .find(|p| p.id == *piece)
                .or_else(|| page.pieces.iter().find(|p| p.id.may_be(piece)))?;
            piece_lines(&mut text, piece)?;
        }
        Place::File { .. } => return None,
        Place::Lines {
            shown_on,
            start,
            end,
            pieces,
        } => {
            let shown = match shown_on {
                ShownOn::Step { step: id } => {
                    let step = tour.find_step(id)?;
                    step_heading(&mut text, &step.title, &step.why, "few lines")?;
                    StepPage::of(step, tour, files, &display, false).pieces
                }
                ShownOn::SafetyNet => {
                    writeln!(text, "Place: a few lines no step of the tour holds").ok()?;
                    SafetyNetPage::of(tour, files, &display).pieces
                }
                ShownOn::File { .. } => return None,
            };
            selected_lines(&mut text, &shown, start, end, pieces)?;
        }
    }
    Some(text)
}

/// The AI is told the changed lines only; unchanged context it can read itself.
#[derive(Default)]
struct ChangedLinesOnly {
    highlights: Highlights,
    files_at_head: FilesAtHead,
    more_context: HashMap<MoreContext, usize>,
}

impl ChangedLinesOnly {
    fn display<'a>(&'a self, links: &'a CodeLinks<'a>) -> DiffDisplay<'a> {
        DiffDisplay {
            highlights: &self.highlights,
            files_at_head: &self.files_at_head,
            more_context: &self.more_context,
            ignores_whitespace: false,
            links,
        }
    }
}

fn selected_lines(
    text: &mut String,
    shown: &[PieceDiff],
    start: &LineLocation,
    end: &LineLocation,
    pieces: &[PieceId],
) -> Option<()> {
    let mut selection = RowSelection::start_at(start.clone());
    selection.extend_to(end.clone());
    let selected = selection.resolve(shown.iter().flat_map(PieceDiff::lines))?;
    writeln!(
        text,
        "\nSelected lines of {}",
        pieces
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    )
    .ok()?;
    for line in selected.lines() {
        diff_line(text, line)?;
    }
    Some(())
}

fn pointers(tour: &GuidedTour) -> String {
    format!(
        "PR: {}\nBase commit: {}\nHead commit: {}\n",
        tour.pin.subject_key(),
        tour.pin.base,
        tour.pin.head
    )
}

fn step_heading(text: &mut String, title: &str, why: &str, kind: &str) -> Option<()> {
    writeln!(
        text,
        "Place: a {kind} of the tour\nStep title: {title}\nWhy: {why}"
    )
    .ok()
}

fn piece_lines(text: &mut String, piece: &PieceDiff) -> Option<()> {
    writeln!(text, "\nPiece {}", piece.id).ok()?;
    for line in piece.lines() {
        diff_line(text, line)?;
    }
    Some(())
}

fn diff_line(text: &mut String, line: &DiffLine) -> Option<()> {
    let (sign, side) = match line.location.side {
        Side::Base => ('-', "base"),
        Side::Head => ('+', "head"),
    };
    writeln!(
        text,
        "{sign} {}:{side}:{}: {}",
        line.location.path, line.location.line, line.text
    )
    .ok()
}
