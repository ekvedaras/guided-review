use std::fmt::Write;

use crate::pieces::ChangedFile;
use crate::tour::Tour;

use std::collections::HashMap;

use super::ask::Place;
use super::code_link::CodeLinks;
use super::diff_display::{DiffDisplay, FilesAtHead};
use super::highlight::Highlights;
use super::row_selection::RowSelection;
use super::step_page::{DiffLine, PieceDiff, SafetyNetPage, Side, StepPage};

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
    let mut text = pointers(tour);
    // The AI is told the changed lines only; unchanged context it can read itself.
    let (highlights, files_at_head, more_context) = (
        Highlights::default(),
        FilesAtHead::default(),
        HashMap::new(),
    );
    let display = DiffDisplay {
        highlights: &highlights,
        files_at_head: &files_at_head,
        more_context: &more_context,
        ignores_whitespace: false,
        links,
    };
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
        Place::Lines {
            step,
            start,
            end,
            pieces,
        } => {
            let shown = if let Some(id) = step {
                let step = tour.find_step(id)?;
                step_heading(&mut text, &step.title, &step.why, "few lines")?;
                StepPage::of(step, tour, files, &display, false).pieces
            } else {
                writeln!(text, "Place: a few lines no step of the tour holds").ok()?;
                SafetyNetPage::of(tour, files, &display).pieces
            };
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
                diff_line(&mut text, line)?;
            }
        }
    }
    Some(text)
}

fn pointers(tour: &Tour) -> String {
    format!(
        "PR: {}\nBase commit: {}\nHead commit: {}\n",
        tour.pr, tour.base, tour.head
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
