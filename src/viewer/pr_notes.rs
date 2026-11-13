use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Context as _;
use sha2::{Digest, Sha256};

use crate::hex;
use crate::pull_request::PullRequestUrl;
use crate::tour::StepId;

use super::markdown::local_files;
use super::ports::{PictureUploadPort, PullRequestDescriptionPort};

const NOTES_HEADING: &str = "## Notes";

/// What the reader can put under Notes in the PR description.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum NoteSubject {
    StepDiagram(StepId),
    /// A picture of the reviewer summary, by its path in the tour's folder.
    SummaryPicture(PathBuf),
    ReviewerSummary,
}

/// What the button that puts a subject under Notes offers.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum NotesButton {
    Add,
    Adding,
    /// The PR description holds it already.
    InNotes,
    Failed(Arc<str>),
}

#[derive(Debug, Clone)]
enum Attempt {
    Adding,
    Failed(Arc<str>),
}

/// The adds to the PR description that are running or failed, by subject. Not saved.
#[derive(Debug, Default)]
pub(super) struct PrNotes(BTreeMap<NoteSubject, Attempt>);

impl PrNotes {
    pub(super) fn button(
        &self,
        subject: &NoteSubject,
        description: &str,
        note: &Note,
    ) -> NotesButton {
        match self.0.get(subject) {
            Some(Attempt::Adding) => NotesButton::Adding,
            _ if note.is_in(description) => NotesButton::InNotes,
            Some(Attempt::Failed(reason)) => NotesButton::Failed(Arc::clone(reason)),
            None => NotesButton::Add,
        }
    }

    pub(super) fn start(
        &mut self,
        pr: &PullRequestUrl,
        subject: &NoteSubject,
        note: Note,
    ) -> Option<ToNotes> {
        if matches!(self.0.get(subject), Some(Attempt::Adding)) {
            return None;
        }
        self.0.insert(subject.clone(), Attempt::Adding);
        Some(ToNotes {
            pr: pr.clone(),
            subject: subject.clone(),
            note,
        })
    }

    /// The description GitHub now holds, when the add went through.
    pub(super) fn finish(&mut self, outcome: ToNotesOutcome) -> Option<String> {
        match outcome.result {
            Ok(description) => {
                self.0.remove(&outcome.subject);
                Some(description)
            }
            Err(reason) => {
                self.0
                    .insert(outcome.subject, Attempt::Failed(reason.into()));
                None
            }
        }
    }
}

/// What goes under Notes for one subject: a fold under its title, its pictures still on this machine.
#[derive(Debug, Clone)]
pub(super) struct Note {
    title: String,
    mark: Mark,
    body: Vec<Part>,
}

/// How the description shows it holds a note.
#[derive(Debug, Clone)]
enum Mark {
    /// The note's own text, so a diagram added before marks were written is still found.
    Text(String),
    /// A hidden comment, as a picture's address on GitHub is new on every upload.
    Hidden(String),
}

#[derive(Debug, Clone)]
enum Part {
    Text(String),
    Picture { alt: String, path: PathBuf },
}

impl Note {
    pub(super) fn diagram(title: &str, diagram: &str) -> Self {
        let diagram = diagram.trim();
        Self {
            title: title.to_owned(),
            mark: Mark::Text(diagram.to_owned()),
            body: vec![Part::Text(format!("```mermaid\n{diagram}\n```"))],
        }
    }

    /// `None` when the picture cannot be read.
    pub(super) fn picture(path: &Path, alt: &str) -> Option<Self> {
        let digest = digest(&[std::fs::read(path).ok()?.as_slice()]);
        let title = if alt.trim().is_empty() {
            path.file_stem()?.to_string_lossy().into_owned()
        } else {
            alt.to_owned()
        };
        Some(Self {
            title,
            mark: Mark::Hidden(format!("guided-review picture {digest}")),
            body: vec![Part::Picture {
                alt: alt.to_owned(),
                path: path.to_owned(),
            }],
        })
    }

    /// The summary as written, its pictures uploaded and its links to other files of the tour's
    /// `folder` left as their text, since GitHub cannot open those.
    pub(super) fn reviewer_summary(summary: &str, folder: &Path) -> Option<Self> {
        let files = local_files(summary, folder);
        let mut contents = vec![summary.as_bytes().to_vec()];
        let mut body = Vec::new();
        let mut written = 0;
        for file in files {
            body.push(Part::Text(
                summary.get(written..file.range.start)?.to_owned(),
            ));
            if file.is_picture {
                contents.push(std::fs::read(&file.path).ok()?);
                body.push(Part::Picture {
                    alt: file.text,
                    path: file.path,
                });
            } else {
                body.push(Part::Text(file.text));
            }
            written = file.range.end;
        }
        body.push(Part::Text(summary.get(written..)?.to_owned()));
        let contents: Vec<&[u8]> = contents.iter().map(Vec::as_slice).collect();
        Some(Self {
            title: "Reviewer summary".to_owned(),
            mark: Mark::Hidden(format!("guided-review summary {}", digest(&contents))),
            body,
        })
    }

    pub(super) fn is_in(&self, description: &str) -> bool {
        match &self.mark {
            Mark::Text(text) => {
                !text.is_empty() && description.replace("\r\n", "\n").contains(text.as_str())
            }
            Mark::Hidden(mark) => description.contains(&format!("<!-- {mark} -->")),
        }
    }

    fn pictures(&self) -> impl Iterator<Item = &Path> {
        self.body.iter().filter_map(|part| match part {
            Part::Picture { path, .. } => Some(path.as_path()),
            Part::Text(_) => None,
        })
    }

    /// GitHub renders Markdown inside `<details>` only after a blank line.
    fn folded(&self, uploaded: &HashMap<&Path, String>) -> String {
        let summary = self
            .title
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;");
        let mark = match &self.mark {
            Mark::Hidden(mark) => format!("<!-- {mark} -->\n"),
            Mark::Text(_) => String::new(),
        };
        let body: String = self
            .body
            .iter()
            .map(|part| match part {
                Part::Text(text) => text.clone(),
                Part::Picture { alt, path } => format!(
                    "![{alt}]({})",
                    uploaded.get(path.as_path()).map_or("", String::as_str)
                ),
            })
            .collect();
        format!(
            "<details>\n<summary>{summary}</summary>\n{mark}\n{}\n\n</details>",
            body.trim()
        )
    }
}

/// The first eight bytes of the SHA-256 of `contents` one after another, in hex.
fn digest(contents: &[&[u8]]) -> String {
    let mut hasher = Sha256::new();
    for content in contents {
        hasher.update(content);
    }
    hex::lower(hasher.finalize().iter().take(8))
}

/// Adding one note to the PR description, owned so it can run off the UI thread.
pub struct ToNotes {
    pr: PullRequestUrl,
    subject: NoteSubject,
    note: Note,
}

pub struct ToNotesOutcome {
    subject: NoteSubject,
    result: Result<String, String>,
}

impl ToNotes {
    /// Reads the description just before writing it, so an edit made on the web since is kept.
    pub fn run(
        self,
        github: &(impl PullRequestDescriptionPort + PictureUploadPort + ?Sized),
    ) -> ToNotesOutcome {
        let result = self.add(github).map_err(|error| format!("{error:#}"));
        ToNotesOutcome {
            subject: self.subject,
            result,
        }
    }

    fn add(
        &self,
        github: &(impl PullRequestDescriptionPort + PictureUploadPort + ?Sized),
    ) -> anyhow::Result<String> {
        let description = github.pull_request_description(&self.pr)?;
        if self.note.is_in(&description) {
            return Ok(description);
        }
        let mut uploaded = HashMap::new();
        for picture in self.note.pictures() {
            let url = github.upload_picture(&self.pr, picture).with_context(|| {
                format!(
                    "could not upload {}",
                    picture.file_name().unwrap_or_default().display()
                )
            })?;
            uploaded.insert(picture, url);
        }
        let description = if uploaded.is_empty() {
            description
        } else {
            github.pull_request_description(&self.pr)?
        };
        if self.note.is_in(&description) {
            return Ok(description);
        }
        let changed = in_notes(&description, &self.note.folded(&uploaded))
            .context("could not find where Notes ends in the description")?;
        github.set_pull_request_description(&self.pr, &changed)?;
        Ok(changed)
    }
}

/// The description with the diagram, collapsed under its title, at the end of its `Notes` section; a
/// description with no such section gets one at its end. `None` when the description holds the diagram
/// already.
pub fn with_diagram_in_notes(description: &str, title: &str, diagram: &str) -> Option<String> {
    let note = Note::diagram(title, diagram);
    if note.is_in(description) {
        return None;
    }
    in_notes(description, &note.folded(&HashMap::new()))
}

fn in_notes(description: &str, block: &str) -> Option<String> {
    let line_end = if description.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let lines: Vec<&str> = description.lines().collect();
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

/// Each heading's line, level and text. A `#` line in a fenced code block is code, and one in a
/// `<details>` fold belongs to the fold, so neither is a heading of the description.
fn headings_outside_code<'a>(lines: &[&'a str]) -> Vec<(usize, usize, &'a str)> {
    let mut in_code = false;
    let mut folds = 0_usize;
    let mut headings = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_code.toggle();
            continue;
        }
        if in_code {
            continue;
        }
        folds += trimmed.matches("<details").count();
        folds = folds.saturating_sub(trimmed.matches("</details>").count());
        if folds > 0 {
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
