use std::collections::HashMap;

use crate::file_path::FilePath;
use crate::pull_request::Sha;

use super::code_link::CodeLinks;
use super::diff_display::{DiffDisplay, FilesAtHead, HiddenLines, MoreContext};
use super::highlight::Highlights;
use super::image_preview::ImagePreview;

/// How the review's lines look: colours, context, whitespace and pictures. None of it changes
/// pieces, coverage or ticks, and none of it is saved. What is read off the main thread for
/// another base or head, e.g. before a re-pin, is dropped.
pub struct DisplayOptions {
    base: Sha,
    head: Sha,
    highlights: Highlights,
    files_at_head: FilesAtHead,
    more_context: HashMap<MoreContext, usize>,
    ignores_whitespace: bool,
    wraps_lines: bool,
    image_previews: HashMap<FilePath, ImagePreview>,
}

impl DisplayOptions {
    pub(super) fn for_code(base: Sha, head: Sha) -> Self {
        Self {
            base,
            head,
            highlights: Highlights::default(),
            files_at_head: FilesAtHead::default(),
            more_context: HashMap::new(),
            ignores_whitespace: false,
            wraps_lines: false,
            image_previews: HashMap::new(),
        }
    }

    /// Says whether they were kept.
    pub fn set_highlights(&mut self, highlights: Highlights) -> bool {
        if !highlights.is_of(&self.base, &self.head) {
            return false;
        }
        self.highlights = highlights;
        true
    }

    pub(super) fn files_at_head(&self) -> &FilesAtHead {
        &self.files_at_head
    }

    /// Says whether they were kept. The review places its threads again after, so only it calls this.
    pub(super) fn set_files_at_head(&mut self, files_at_head: FilesAtHead) -> bool {
        if !files_at_head.is_at(&self.head) {
            return false;
        }
        self.files_at_head = files_at_head;
        true
    }

    pub fn show_more_context(&mut self, more: &MoreContext) {
        DiffDisplay::more_lines(&mut self.more_context, more);
    }

    /// Every line of the gap, from the side it can be shown from; a gap that does not say how many
    /// lines it hides shows the usual few more.
    pub fn show_hidden_lines(&mut self, gap: &HiddenLines) {
        let either_side = gap.after_above.as_ref().or(gap.before_below.as_ref());
        match (either_side, gap.count) {
            (Some(more), Some(count)) => {
                *self.more_context.entry(more.clone()).or_default() += count;
            }
            (Some(more), None) => self.show_more_context(more),
            (None, _) => {}
        }
    }

    pub fn ignores_whitespace(&self) -> bool {
        self.ignores_whitespace
    }

    pub fn toggle_ignore_whitespace(&mut self) {
        self.ignores_whitespace = !self.ignores_whitespace;
    }

    pub fn wraps_lines(&self) -> bool {
        self.wraps_lines
    }

    pub fn toggle_wrap_lines(&mut self) {
        self.wraps_lines = !self.wraps_lines;
    }

    pub fn image_preview(&self, path: &FilePath) -> Option<&ImagePreview> {
        self.image_previews.get(path)
    }

    /// Says whether it was kept.
    pub fn set_image_preview(&mut self, preview: ImagePreview) -> bool {
        if !preview.is_of(&self.base, &self.head) {
            return false;
        }
        self.image_previews.insert(preview.path.clone(), preview);
        true
    }

    pub(super) fn display<'a>(&'a self, links: &'a CodeLinks<'a>) -> DiffDisplay<'a> {
        DiffDisplay {
            highlights: &self.highlights,
            files_at_head: &self.files_at_head,
            more_context: &self.more_context,
            ignores_whitespace: self.ignores_whitespace,
            links,
        }
    }
}
