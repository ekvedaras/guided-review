use std::fmt::Write;

use crate::pieces::ChangedFile;
use crate::tour::Tour;

use super::ask::Place;
use super::github_link::GitHubLinks;
use super::highlight::Highlights;
use super::step_page::{PieceDiff, Side, StepPage};

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
    links: &GitHubLinks,
    place: &Place,
) -> Option<String> {
    let mut text = pointers(tour);
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
        Place::Step(id) => {
            let step = tour
                .steps()
                .into_iter()
                .find(|step| step.id == id.to_string())?;
            step_heading(&mut text, step.title.as_str(), step.why.as_str(), "step")?;
            for piece in
                StepPage::of(step, tour, files, links, &Highlights::default(), false).pieces
            {
                piece_lines(&mut text, &piece)?;
            }
        }
        Place::Piece { step, piece } => {
            let step = tour
                .steps()
                .into_iter()
                .find(|s| s.id == step.to_string())?;
            step_heading(&mut text, &step.title, &step.why, "piece")?;
            let page = StepPage::of(step, tour, files, links, &Highlights::default(), false);
            let piece = page.pieces.iter().find(|p| p.id.to_string() == *piece)?;
            piece_lines(&mut text, piece)?;
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
    for line in &piece.lines {
        let (sign, side) = match line.location.side {
            Side::Base => ('-', "base"),
            Side::Head => ('+', "head"),
        };
        writeln!(
            text,
            "{sign} {}:{side}:{}: {}",
            line.location.path, line.location.line, line.text
        )
        .ok()?;
    }
    Some(())
}
