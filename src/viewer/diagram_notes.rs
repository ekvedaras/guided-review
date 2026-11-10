use std::collections::BTreeMap;
use std::sync::Arc;

use crate::pull_request::PullRequestUrl;
use crate::tour::StepId;

use super::ports::PullRequestDescriptionPort;

const NOTES_HEADING: &str = "## Notes";

/// What the button next to a step's diagram offers.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum NotesButton {
    Add,
    Adding,
    /// The PR description holds the diagram already.
    InNotes,
    Failed(Arc<str>),
}

#[derive(Debug, Clone)]
enum Attempt {
    Adding,
    Failed(Arc<str>),
}

/// The adds of diagrams to the PR description that are running or failed, by step. Not saved.
#[derive(Debug, Default)]
pub(super) struct DiagramNotes(BTreeMap<StepId, Attempt>);

impl DiagramNotes {
    pub(super) fn button(&self, step: &StepId, description: &str, diagram: &str) -> NotesButton {
        match self.0.get(step) {
            Some(Attempt::Adding) => NotesButton::Adding,
            _ if holds(description, diagram) => NotesButton::InNotes,
            Some(Attempt::Failed(reason)) => NotesButton::Failed(Arc::clone(reason)),
            None => NotesButton::Add,
        }
    }

    pub(super) fn start(
        &mut self,
        pr: &PullRequestUrl,
        step: &StepId,
        title: &str,
        diagram: &str,
    ) -> Option<DiagramToNotes> {
        if matches!(self.0.get(step), Some(Attempt::Adding)) {
            return None;
        }
        self.0.insert(step.clone(), Attempt::Adding);
        Some(DiagramToNotes {
            pr: pr.clone(),
            step: step.clone(),
            title: title.to_owned(),
            diagram: diagram.to_owned(),
        })
    }

    /// The description GitHub now holds, when the add went through.
    pub(super) fn finish(&mut self, outcome: DiagramToNotesOutcome) -> Option<String> {
        match outcome.result {
            Ok(description) => {
                self.0.remove(&outcome.step);
                Some(description)
            }
            Err(reason) => {
                self.0.insert(outcome.step, Attempt::Failed(reason.into()));
                None
            }
        }
    }
}

/// Adding one step's diagram to the PR description, owned so it can run off the UI thread.
pub struct DiagramToNotes {
    pr: PullRequestUrl,
    step: StepId,
    title: String,
    diagram: String,
}

pub struct DiagramToNotesOutcome {
    step: StepId,
    result: Result<String, String>,
}

impl DiagramToNotes {
    /// Reads the description just before writing it, so an edit made on the web since is kept.
    pub fn run(self, github: &(impl PullRequestDescriptionPort + ?Sized)) -> DiagramToNotesOutcome {
        let result = self.add(github).map_err(|error| format!("{error:#}"));
        DiagramToNotesOutcome {
            step: self.step,
            result,
        }
    }

    fn add(&self, github: &(impl PullRequestDescriptionPort + ?Sized)) -> anyhow::Result<String> {
        let description = github.pull_request_description(&self.pr)?;
        let Some(changed) = with_diagram_in_notes(&description, &self.title, &self.diagram) else {
            return Ok(description);
        };
        github.set_pull_request_description(&self.pr, &changed)?;
        Ok(changed)
    }
}

/// The description with the diagram, under its title, at the end of its `Notes` section; a description with
/// no such section gets one at its end. `None` when the description holds the diagram already.
pub fn with_diagram_in_notes(description: &str, title: &str, diagram: &str) -> Option<String> {
    if holds(description, diagram) {
        return None;
    }
    let line_end = if description.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let lines: Vec<&str> = description.lines().collect();
    let block = format!("**{title}**\n\n```mermaid\n{}\n```", diagram.trim());
    let mut out: Vec<&str> = Vec::with_capacity(lines.len() + 8);
    let ends_with_line_end = match notes_section(&lines) {
        Some(section) if section.end < lines.len() => {
            out.extend(lines.get(..last_written(&lines, section.start, section.end))?);
            out.push("");
            out.extend(block.lines());
            out.push("");
            out.extend(lines.get(section.end..)?);
            description.ends_with('\n')
        }
        Some(section) => {
            out.extend(lines.get(..last_written(&lines, section.start, section.end))?);
            out.push("");
            out.extend(block.lines());
            true
        }
        None => {
            out.extend(lines.get(..last_written(&lines, 0, lines.len()))?);
            if !out.is_empty() {
                out.push("");
            }
            out.push(NOTES_HEADING);
            out.push("");
            out.extend(block.lines());
            true
        }
    };
    let mut changed = out.join(line_end);
    if ends_with_line_end {
        changed.push_str(line_end);
    }
    Some(changed)
}

fn holds(description: &str, diagram: &str) -> bool {
    let diagram = diagram.trim();
    !diagram.is_empty() && description.replace("\r\n", "\n").contains(diagram)
}

/// The notes heading's line and the line after its section's last.
struct Section {
    start: usize,
    end: usize,
}

fn notes_section(lines: &[&str]) -> Option<Section> {
    let headings = headings_outside_code(lines);
    let (start, level) = headings
        .iter()
        .find(|(_, _, text)| text.eq_ignore_ascii_case("notes"))
        .map(|(index, level, _)| (*index, *level))?;
    let end = headings
        .iter()
        .find(|(index, other, _)| *index > start && *other <= level)
        .map_or(lines.len(), |(index, _, _)| *index);
    Some(Section { start, end })
}

/// Past the last line in `start..end` that is not blank, so new text follows it after one blank line.
fn last_written(lines: &[&str], start: usize, end: usize) -> usize {
    lines
        .get(start..end)
        .and_then(|section| section.iter().rposition(|line| !line.trim().is_empty()))
        .map_or(start, |last| start + last + 1)
}

/// Each heading's line, level and text. A `#` line in a fenced code block is code, not a heading.
fn headings_outside_code<'a>(lines: &[&'a str]) -> Vec<(usize, usize, &'a str)> {
    let mut in_code = false;
    let mut headings = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_code = !in_code;
            continue;
        }
        if in_code {
            continue;
        }
        let level = trimmed.chars().take_while(|c| *c == '#').count();
        let Some(rest) = trimmed.get(level..) else {
            continue;
        };
        if (1..=6).contains(&level) && rest.starts_with([' ', '\t']) {
            headings.push((index, level, rest.trim().trim_end_matches('#').trim_end()));
        }
    }
    headings
}
