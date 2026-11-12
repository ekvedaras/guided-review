//! The rendered view of a file box: a Markdown file drawn as formatted text with its changed blocks
//! marked, or a picture drawn as its pictures, instead of the file's diff lines.

use std::collections::{BTreeSet, HashMap};
use std::ops::Range;
use std::sync::Arc;

use pulldown_cmark::{Event, Parser};

use super::markdown::OPTIONS;
use crate::code_source::{FileStatus, Hunk};
use crate::file_path::FilePath;
use crate::pieces::ChangedFile;

/// How a file box shows its file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FileView {
    /// The diff lines, which take line comments.
    Code,
    /// The file as it reads: Markdown formatted, a picture drawn. Takes no line comments.
    Rendered,
}

/// What a file can be rendered as.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Rendering {
    Markdown,
    Pictures,
}

impl Rendering {
    /// `None` for a file the rendered view has nothing to add to: code, and a picture with no lines
    /// of code, which shows its pictures anyway.
    pub(super) fn of(path: &FilePath, is_image: bool, has_lines: bool) -> Option<Self> {
        let extension = path
            .as_str()
            .rsplit_once('.')
            .map(|(_, extension)| extension.to_ascii_lowercase());
        if matches!(extension.as_deref(), Some("md" | "markdown")) {
            Some(Self::Markdown)
        } else if is_image && has_lines {
            Some(Self::Pictures)
        } else {
            None
        }
    }

    /// Markdown is reviewed as code first, like any text the PR changes; a picture's code is its
    /// drawing's coordinates, which nobody reviews by reading.
    fn first_view(self) -> FileView {
        match self {
            Self::Markdown => FileView::Code,
            Self::Pictures => FileView::Rendered,
        }
    }
}

/// The Code / Rendered switch in the header of a file box that offers it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ViewSwitch {
    pub path: FilePath,
    pub view: FileView,
}

/// How a block of a rendered Markdown file differs from the base.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BlockChange {
    /// Every line of it is new.
    Added,
    Changed,
    /// Only at base: shown where it was.
    Removed,
}

/// One top-level block of a rendered Markdown file: a heading, a paragraph, a list, a fence.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RenderedBlock {
    /// Tells this block's folds apart from every other block's.
    pub key: Arc<str>,
    /// The block's own Markdown, without the line break after it.
    pub source: Arc<str>,
    pub change: Option<BlockChange>,
}

/// The view each file box shows, and the Markdown files cut into blocks once, when their text is read.
#[derive(Default)]
pub(super) struct FileViews {
    chosen: HashMap<FilePath, FileView>,
    markdown: HashMap<FilePath, Arc<[Arc<RenderedBlock>]>>,
}

impl FileViews {
    pub(super) fn view(&self, path: &FilePath, rendering: Rendering) -> FileView {
        self.chosen
            .get(path)
            .copied()
            .unwrap_or_else(|| rendering.first_view())
    }

    pub(super) fn show_as(&mut self, path: &FilePath, view: FileView) {
        self.chosen.insert(path.clone(), view);
    }

    /// `None` until the file's text at head is read.
    pub(super) fn markdown(&self, path: &FilePath) -> Option<&Arc<[Arc<RenderedBlock>]>> {
        self.markdown.get(path)
    }

    /// Cuts every Markdown file of the PR whose text is known: a deleted file from its removed lines,
    /// any other from `text_at_head`.
    pub(super) fn cut_markdown<'a>(
        &mut self,
        files: &[ChangedFile],
        text_at_head: impl Fn(&FilePath) -> Option<&'a str>,
    ) {
        for file in files {
            if Rendering::of(&file.path, false, false) != Some(Rendering::Markdown) {
                continue;
            }
            let blocks = if file.status == FileStatus::Deleted {
                Some(deleted_blocks(file))
            } else {
                text_at_head(&file.path).map(|text| marked_blocks(text, &file.hunks))
            };
            if let Some(blocks) = blocks {
                let keyed = blocks
                    .into_iter()
                    .enumerate()
                    .map(|(index, (source, change))| {
                        Arc::new(RenderedBlock {
                            key: format!("rendered-{}-{index}", file.path).into(),
                            source,
                            change,
                        })
                    })
                    .collect();
                self.markdown.insert(file.path.clone(), keyed);
            }
        }
    }
}

type Cut = Vec<(Arc<str>, Option<BlockChange>)>;

/// A deleted file is its base: its removed lines, every block of it removed.
fn deleted_blocks(file: &ChangedFile) -> Cut {
    let mut lines: Vec<_> = file.hunks.iter().flat_map(|hunk| &hunk.removed).collect();
    lines.sort_by_key(|line| line.number);
    let base = lines
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    top_level_blocks(&base)
        .into_iter()
        .map(|block| (block.source, Some(BlockChange::Removed)))
        .collect()
}

/// The blocks of the file at head, each marked by the lines the PR added to it, with the runs of
/// removed lines that took no block with them shown as removed blocks where they were.
fn marked_blocks(text: &str, hunks: &[Hunk]) -> Cut {
    let added: BTreeSet<usize> = hunks
        .iter()
        .flat_map(|hunk| &hunk.added)
        .map(|line| usize::from(line.number))
        .collect();
    let blocks = top_level_blocks(text);
    let (taken_in, between): (Vec<_>, Vec<_>) = removed_runs(hunks, &added)
        .into_iter()
        .partition(|run| blocks.iter().any(|block| run.is_in(block, &added)));
    let mut cut = Cut::new();
    let mut between = between.into_iter().peekable();
    for block in &blocks {
        while let Some(run) = between.next_if(|run| run.below_head_line < block.first) {
            cut.extend(run.blocks());
        }
        let added_lines = (block.first..=block.last)
            .filter(|line| added.contains(line))
            .count();
        let lost_lines = taken_in.iter().any(|run| run.is_in(block, &added));
        let change = if lost_lines || (added_lines > 0 && added_lines <= block.last - block.first) {
            Some(BlockChange::Changed)
        } else if added_lines > 0 {
            Some(BlockChange::Added)
        } else {
            None
        };
        cut.push((Arc::clone(&block.source), change));
    }
    cut.extend(between.flat_map(|run| run.blocks()));
    cut
}

/// Base lines the PR removed one after another, and the line at head they were under.
struct RemovedRun {
    below_head_line: usize,
    text: String,
}

impl RemovedRun {
    /// Cut out of the block, or replaced by its first lines.
    fn is_in(&self, block: &SourceBlock, added: &BTreeSet<usize>) -> bool {
        let next = self.below_head_line + 1;
        block.holds(next) && (next > block.first || added.contains(&next))
    }

    fn blocks(&self) -> Cut {
        top_level_blocks(&self.text)
            .into_iter()
            .map(|block| (block.source, Some(BlockChange::Removed)))
            .collect()
    }
}

/// Walks base and head side by side, as a unified diff reads, so each removed line learns the
/// head line it was under, however much context the hunks were cut with.
fn removed_runs(hunks: &[Hunk], added: &BTreeSet<usize>) -> Vec<RemovedRun> {
    let mut removed: Vec<_> = hunks.iter().flat_map(|hunk| &hunk.removed).collect();
    removed.sort_by_key(|line| line.number);
    let mut runs: Vec<RemovedRun> = Vec::new();
    let mut base_line = 1;
    let mut head_line = 1;
    let mut last_removed = 0;
    for line in removed {
        let number = usize::from(line.number);
        while base_line < number {
            while added.contains(&head_line) {
                head_line += 1;
            }
            base_line += 1;
            head_line += 1;
        }
        let below_head_line = head_line - 1;
        match runs.last_mut() {
            Some(run) if last_removed + 1 == number && run.below_head_line == below_head_line => {
                run.text.push('\n');
                run.text.push_str(&line.text);
            }
            _ => runs.push(RemovedRun {
                below_head_line,
                text: line.text.clone(),
            }),
        }
        last_removed = number;
        base_line = number + 1;
    }
    runs
}

/// A top-level block of a Markdown text and the lines it spans, 1-based, both ends included.
struct SourceBlock {
    source: Arc<str>,
    first: usize,
    last: usize,
}

impl SourceBlock {
    fn holds(&self, line: usize) -> bool {
        (self.first..=self.last).contains(&line)
    }
}

/// The blocks are cut by the parser's own source ranges, so a block is what the renderer draws as one.
fn top_level_blocks(text: &str) -> Vec<SourceBlock> {
    let line_starts: Vec<usize> = std::iter::once(0)
        .chain(text.match_indices('\n').map(|(at, _)| at + 1))
        .collect();
    let line_of = |byte: usize| line_starts.partition_point(|start| *start <= byte);
    let mut blocks = Vec::new();
    let mut depth = 0_usize;
    for (event, range) in Parser::new_ext(text, OPTIONS).into_offset_iter() {
        let top_level = match event {
            Event::Start(_) => {
                depth += 1;
                (depth == 1).then_some(range)
            }
            Event::End(_) => {
                depth = depth.saturating_sub(1);
                None
            }
            _ => (depth == 0).then_some(range),
        };
        let Some(Range { start, end }) = top_level else {
            continue;
        };
        let Some(source) = text.get(start..end).map(str::trim_end) else {
            continue;
        };
        if source.trim().is_empty() {
            continue;
        }
        blocks.push(SourceBlock {
            source: source.into(),
            first: line_of(start),
            last: line_of(start + source.len().saturating_sub(1)),
        });
    }
    blocks
}
