use std::collections::HashMap;

use crate::file_path::FilePath;
use crate::pull_request::Sha;

use super::code_link::CodeLinks;
use super::diff_display::{DiffDisplay, FilesAtHead, MoreContext};
use super::highlight::Highlights;
use super::image_preview::ImagePreview;

/// How the review's lines look: colours, context, whitespace and pictures. None of it changes
/// pieces, coverage or ticks, and none of it is saved.
#[derive(Default)]
pub(super) struct DisplayOptions {
    highlights: Highlights,
    files_at_head: FilesAtHead,
    more_context: HashMap<MoreContext, usize>,
    ignores_whitespace: bool,
    wraps_lines: bool,
    image_previews: HashMap<FilePath, ImagePreview>,
}

impl DisplayOptions {
    pub(super) fn set_highlights(&mut self, highlights: Highlights) {
        self.highlights = highlights;
    }

    pub(super) fn files_at_head(&self) -> &FilesAtHead {
        &self.files_at_head
    }

    pub(super) fn set_files_at_head(&mut self, files_at_head: FilesAtHead) {
        self.files_at_head = files_at_head;
    }

    pub(super) fn show_more_context(&mut self, more: &MoreContext) {
        DiffDisplay::more_lines(&mut self.more_context, more);
    }

    pub(super) fn show_lines(&mut self, more: &MoreContext, count: usize) {
        *self.more_context.entry(more.clone()).or_default() += count;
    }

    pub(super) fn ignores_whitespace(&self) -> bool {
        self.ignores_whitespace
    }

    pub(super) fn toggle_ignore_whitespace(&mut self) {
        self.ignores_whitespace = !self.ignores_whitespace;
    }

    pub(super) fn wraps_lines(&self) -> bool {
        self.wraps_lines
    }

    pub(super) fn toggle_wrap_lines(&mut self) {
        self.wraps_lines = !self.wraps_lines;
    }

    pub(super) fn image_preview(&self, path: &str) -> Option<&ImagePreview> {
        self.image_previews.get(path)
    }

    /// A preview loaded for another base or head, e.g. before a re-open, is dropped. Says whether it was kept.
    pub(super) fn set_image_preview(
        &mut self,
        preview: ImagePreview,
        base: &Sha,
        head: &Sha,
    ) -> bool {
        if !preview.is_of(base, head) {
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
