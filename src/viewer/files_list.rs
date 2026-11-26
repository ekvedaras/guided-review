//! The files list: every changed file as a folder tree, with its line counts, its read tick and the
//! screens that show it, so the reviewer sees the shape of the change and goes to a file.

use std::collections::BTreeSet;

use super::file_tree::{FileTree, FolderPath, TreeRowKind};
use super::jump::JumpTarget;
use super::screen::Screen;
use crate::file_path::FilePath;

/// A screen that shows a file: a step, the safety net, or a basic tour's file screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilePlace {
    pub screen: Screen,
    /// "Step 2 · title", "Safety net", or the file's path for a basic tour's file screen.
    pub heading: String,
    pub is_read: bool,
    /// The screen is the one on show.
    pub is_current: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListedFile {
    pub path: FilePath,
    pub added: usize,
    pub removed: usize,
    /// Every step that shows it is read; a basic tour's file is read by its own tick.
    pub is_read: bool,
    /// In reading order.
    pub places: Vec<FilePlace>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FilesListRowKind {
    Folder {
        path: FolderPath,
        /// The folder's own name, or the joined names of folders that hold only one folder.
        label: String,
        is_folded: bool,
        added: usize,
        removed: usize,
        /// Every file under it is read.
        is_read: bool,
    },
    File {
        path: FilePath,
        label: String,
        added: usize,
        removed: usize,
        is_read: bool,
        place_count: usize,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilesListRow {
    pub depth: usize,
    pub kind: FilesListRowKind,
    pub is_under_cursor: bool,
}

/// What the cursor is on, kept by name so a fold or a refresh does not move it to another row.
#[derive(Debug, Clone, PartialEq, Eq)]
enum RowKey {
    Folder(FolderPath),
    File(FilePath),
}

#[derive(Debug, Default, Clone)]
pub struct FilesList {
    tree: FileTree,
    /// In the tree's reading order.
    files: Vec<ListedFile>,
    folded: BTreeSet<FolderPath>,
    cursor: Option<RowKey>,
    /// Lowercase words a shown file's path must all hold; none shows every file.
    query: Vec<String>,
}

impl FilesList {
    /// The cursor starts on the first file the screen on show shows, else on the first file.
    pub(super) fn of(files: Vec<ListedFile>) -> Self {
        let tree = FileTree::of(files.iter().map(|file| &file.path));
        let order = tree.reading_order();
        let mut files = files;
        files.sort_by_key(|file| order.iter().position(|path| *path == file.path));
        let cursor = files
            .iter()
            .find(|file| file.places.iter().any(|place| place.is_current))
            .or_else(|| files.first())
            .map(|file| RowKey::File(file.path.clone()));
        Self {
            tree,
            files,
            folded: BTreeSet::new(),
            cursor,
            query: Vec::new(),
        }
    }

    pub fn file_count(&self) -> usize {
        self.files.len()
    }

    /// Lines added and removed over every file.
    pub fn line_counts(&self) -> (usize, usize) {
        (
            self.files.iter().map(|file| file.added).sum(),
            self.files.iter().map(|file| file.removed).sum(),
        )
    }

    /// Folders and files in reading order, minus what sits under a folded folder.
    pub fn rows(&self) -> Vec<FilesListRow> {
        self.keyed_rows().into_iter().map(|(_, row)| row).collect()
    }

    /// The file under the cursor; `None` while it is on a folder.
    pub fn picked(&self) -> Option<&ListedFile> {
        match &self.cursor {
            Some(RowKey::File(path)) => self.file(path),
            Some(RowKey::Folder(_)) | None => None,
        }
    }

    /// Down from the last row goes to the first.
    pub fn cursor_down(&mut self) {
        self.move_cursor(1);
    }

    /// Up from the first row goes to the last.
    pub fn cursor_up(&mut self) {
        self.move_cursor(-1);
    }

    /// Puts the cursor on a row by its place in `rows`, as a click does.
    pub fn put_cursor_on(&mut self, row: usize) {
        if let Some((key, _)) = self.keyed_rows().into_iter().nth(row) {
            self.cursor = Some(key);
        }
    }

    /// Folds the folder under the cursor, or the folder of the file under it, which the cursor moves to.
    pub fn fold_under_cursor(&mut self) {
        let is_on_folded_folder =
            matches!(&self.cursor, Some(RowKey::Folder(folder)) if self.folded.contains(folder));
        if (is_on_folded_folder || matches!(self.cursor, Some(RowKey::File(_))))
            && let Some(folder) = self.folder_of_cursor()
        {
            self.cursor = Some(RowKey::Folder(folder));
        }
        if let Some(RowKey::Folder(folder)) = &self.cursor {
            self.folded.insert(folder.clone());
        }
    }

    pub fn unfold_under_cursor(&mut self) {
        if let Some(RowKey::Folder(folder)) = &self.cursor {
            self.folded.remove(folder);
        }
    }

    pub fn toggle_fold_under_cursor(&mut self) {
        if let Some(RowKey::Folder(folder)) = &self.cursor
            && !self.folded.remove(folder)
        {
            self.folded.insert(folder.clone());
        }
    }

    /// Keeps the files whose path holds every word of `query`, ignoring case.
    /// The cursor moves to the first file shown when its row is not.
    pub fn set_query(&mut self, query: &str) {
        self.query = query.split_whitespace().map(str::to_lowercase).collect();
        if !self.cursor.as_ref().is_some_and(|key| self.has_row(key)) {
            let rows = self.keyed_rows();
            self.cursor = rows
                .iter()
                .map(|(key, _)| key)
                .find(|key| matches!(key, RowKey::File(_)))
                .or_else(|| rows.first().map(|(key, _)| key))
                .cloned();
        }
    }

    pub fn is_on_folder(&self) -> bool {
        matches!(self.cursor, Some(RowKey::Folder(_)))
    }

    /// Where going to the picked file lands: its `nth` place (from 0), on the file's box.
    pub fn target_of_picked(&self, nth: usize) -> Option<JumpTarget> {
        let file = self.picked()?;
        let place = file.places.get(nth)?;
        Some(JumpTarget::file_on(
            place.screen.clone(),
            file.path.clone(),
            place.heading.clone(),
        ))
    }

    /// The files read again while the list is open: the folds and the cursor stay where they were,
    /// unless the cursor's row is gone.
    pub fn refreshed_to(&self, list: &Self) -> Self {
        let mut refreshed = list.clone();
        refreshed.folded.clone_from(&self.folded);
        refreshed.query.clone_from(&self.query);
        if self
            .cursor
            .as_ref()
            .is_some_and(|key| refreshed.has_row(key))
        {
            refreshed.cursor.clone_from(&self.cursor);
        }
        refreshed
    }

    fn file(&self, path: &FilePath) -> Option<&ListedFile> {
        self.files.iter().find(|file| file.path == *path)
    }

    fn is_shown(&self, file: &ListedFile) -> bool {
        let path = file.path.as_str().to_lowercase();
        self.query.iter().all(|word| path.contains(word.as_str()))
    }

    fn files_under<'a>(
        &'a self,
        folder: &FolderPath,
    ) -> impl Iterator<Item = &'a ListedFile> + use<'a> {
        let prefix = format!("{folder}/");
        self.files
            .iter()
            .filter(move |file| file.path.as_str().starts_with(&prefix) && self.is_shown(file))
    }

    /// The nearest folder above the file under the cursor, as the rows show it.
    fn folder_of_cursor(&self) -> Option<FolderPath> {
        let rows = self.keyed_rows();
        let at = rows
            .iter()
            .position(|(key, _)| Some(key) == self.cursor.as_ref())?;
        let depth = rows.get(at)?.1.depth;
        rows.iter()
            .take(at)
            .rev()
            .find(|(_, row)| row.depth < depth)
            .and_then(|(key, _)| match key {
                RowKey::Folder(folder) => Some(folder.clone()),
                RowKey::File(_) => None,
            })
    }

    fn has_row(&self, key: &RowKey) -> bool {
        self.keyed_rows().iter().any(|(row, _)| row == key)
    }

    fn move_cursor(&mut self, by: isize) {
        let keys: Vec<RowKey> = self.keyed_rows().into_iter().map(|(key, _)| key).collect();
        if keys.is_empty() {
            return;
        }
        let count = keys.len().cast_signed();
        let at = self
            .cursor
            .as_ref()
            .and_then(|cursor| keys.iter().position(|key| key == cursor))
            .map_or(if by > 0 { -1 } else { 0 }, usize::cast_signed);
        let next = (at + by).rem_euclid(count).cast_unsigned();
        self.cursor = keys.into_iter().nth(next);
    }

    fn keyed_rows(&self) -> Vec<(RowKey, FilesListRow)> {
        // A search shows every match, so it builds its own tree and ignores the folds, which stay for after.
        let searched;
        let (tree, folded) = if self.query.is_empty() {
            (&self.tree, &self.folded)
        } else {
            searched = FileTree::of(
                self.files
                    .iter()
                    .filter(|file| self.is_shown(file))
                    .map(|file| &file.path),
            );
            (&searched, &BTreeSet::new())
        };
        tree.rows(folded, None, &|_| false, &|_| None)
            .into_iter()
            .filter_map(|row| {
                let (key, kind) = match row.kind {
                    TreeRowKind::Folder {
                        path,
                        label,
                        is_folded,
                        ..
                    } => (
                        RowKey::Folder(path.clone()),
                        FilesListRowKind::Folder {
                            added: self.files_under(&path).map(|file| file.added).sum(),
                            removed: self.files_under(&path).map(|file| file.removed).sum(),
                            is_read: self.files_under(&path).all(|file| file.is_read),
                            path,
                            label,
                            is_folded,
                        },
                    ),
                    TreeRowKind::File { path, label, .. } => {
                        let file = self.file(&path)?;
                        (
                            RowKey::File(path.clone()),
                            FilesListRowKind::File {
                                added: file.added,
                                removed: file.removed,
                                is_read: file.is_read,
                                place_count: file.places.len(),
                                path,
                                label,
                            },
                        )
                    }
                };
                let is_under_cursor = self.cursor.as_ref() == Some(&key);
                Some((
                    key,
                    FilesListRow {
                        depth: row.depth,
                        kind,
                        is_under_cursor,
                    },
                ))
            })
            .collect()
    }
}
