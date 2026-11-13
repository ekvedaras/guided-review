use std::collections::HashMap;

use crate::file_path::FilePath;
use crate::pull_request::Sha;

use super::code_link::CodeLinks;
use super::diff_display::{DiffDisplay, FilesAtHead, HiddenLines, MoreContext};
use super::focused_change::Focus;
use super::highlight::Highlights;
use super::image_preview::ImagePreview;
use super::more_lines::MoreLines;
use super::piece_labels::{FileLabels, PieceLabels};
use super::rendered_view::{FileView, FileViews, RenderedBlock, Rendering};
use super::step_page::PieceDiff;
use crate::pieces::ChangedFile;
use std::sync::Arc;

/// How the review's lines look: colours, context, whitespace, pictures and each file's view. None of it changes
/// pieces, coverage or ticks, and none of it is saved. What is read off the main thread for
/// another base or head, e.g. before a re-pin, is dropped.
pub struct DisplayOptions {
    base: Sha,
    head: Sha,
    highlights: Highlights,
    files_at_head: FilesAtHead,
    more_context: HashMap<MoreContext, usize>,
    more_lines: MoreLines,
    ignores_whitespace: bool,
    wraps_lines: bool,
    image_previews: HashMap<FilePath, ImagePreview>,
    file_views: FileViews,
    piece_labels: FileLabels,
    focused_change: Option<Focus>,
}

impl DisplayOptions {
    pub(super) fn for_code(base: Sha, head: Sha) -> Self {
        Self {
            base,
            head,
            highlights: Highlights::default(),
            files_at_head: FilesAtHead::default(),
            more_context: HashMap::new(),
            more_lines: MoreLines::default(),
            ignores_whitespace: false,
            wraps_lines: false,
            image_previews: HashMap::new(),
            file_views: FileViews::default(),
            piece_labels: FileLabels::default(),
            focused_change: None,
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

    pub fn more_lines(&self) -> MoreLines {
        self.more_lines
    }

    pub fn use_more_lines(&mut self, more_lines: MoreLines) {
        self.more_lines = more_lines;
    }

    pub fn show_more_context(&mut self, more: &MoreContext) {
        *self.more_context.entry(more.clone()).or_default() += self.more_lines.get();
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

    /// The change the change keys act on, when it is on the screen shown.
    pub fn focused_change(&self) -> Option<&Focus> {
        self.focused_change.as_ref()
    }

    pub(super) fn focus_change(&mut self, change: Focus) {
        self.focused_change = Some(change);
    }

    pub fn ignores_whitespace(&self) -> bool {
        self.ignores_whitespace
    }

    pub fn toggle_ignore_whitespace(&mut self) {
        self.ignores_whitespace.toggle();
    }

    pub fn wraps_lines(&self) -> bool {
        self.wraps_lines
    }

    pub fn toggle_wrap_lines(&mut self) {
        self.wraps_lines.toggle();
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

    /// The view a file box that can be rendered shows.
    pub fn file_view(&self, path: &FilePath, rendering: Rendering) -> FileView {
        self.file_views.view(path, rendering)
    }

    pub fn show_file_as(&mut self, path: &FilePath, view: FileView) {
        self.file_views.show_as(path, view);
    }

    /// Whether a file box of several pieces shows its piece labels: the reviewer's choice, else the
    /// starting rule over the pieces the box shows.
    pub fn piece_labels(&self, path: &FilePath, pieces: &[PieceDiff]) -> PieceLabels {
        self.piece_labels.of(path, pieces)
    }

    pub fn show_piece_labels(&mut self, path: &FilePath, labels: PieceLabels) {
        self.piece_labels.show_as(path, labels);
    }

    /// The blocks of a Markdown file as its rendered view shows them; `None` until its text is read.
    pub fn rendered_markdown(&self, path: &FilePath) -> Option<&Arc<[Arc<RenderedBlock>]>> {
        self.file_views.markdown(path)
    }

    /// Cuts the Markdown files whose text is known now into the blocks their rendered view shows.
    pub(super) fn cut_markdown(&mut self, files: &[ChangedFile]) {
        let files_at_head = &self.files_at_head;
        self.file_views
            .cut_markdown(files, |path| files_at_head.text(path));
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
