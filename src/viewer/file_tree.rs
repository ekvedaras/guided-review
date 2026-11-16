//! The changed files as a folder tree, like an IDE's: it fixes the order a basic tour reads them in
//! and the rows its sidebar draws.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::Arc;

use crate::file_path::FilePath;

/// A folder's path in the repo, with `/` separators. A folder that holds only one folder is joined
/// with it (`src/viewer`) and stands for the inner one, so the path names the row.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FolderPath(Arc<str>);

impl FolderPath {
    pub fn new(path: &str) -> Self {
        Self(path.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for FolderPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// One line of the sidebar, indented by `depth`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeRow {
    pub depth: usize,
    pub kind: TreeRowKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TreeRowKind {
    Folder {
        path: FolderPath,
        /// The folder's own name, or the joined names (`src/viewer`).
        label: String,
        is_folded: bool,
    },
    File {
        path: FilePath,
        label: String,
        is_current: bool,
    },
}

#[derive(Debug, Default)]
pub struct FileTree {
    root: Folder,
}

#[derive(Debug, Default)]
struct Folder {
    folders: BTreeMap<String, Self>,
    files: BTreeMap<String, FilePath>,
}

impl FileTree {
    pub fn of<'a>(paths: impl IntoIterator<Item = &'a FilePath>) -> Self {
        let mut root = Folder::default();
        for path in paths {
            let mut folder = &mut root;
            let mut names = path.as_str().split('/').peekable();
            while let Some(name) = names.next() {
                if names.peek().is_some() {
                    folder = folder.folders.entry(name.to_owned()).or_default();
                } else {
                    folder.files.insert(name.to_owned(), path.clone());
                }
            }
        }
        Self { root }
    }

    /// Folders before files, each by name, depth first: the order an IDE lists them in.
    pub fn reading_order(&self) -> Vec<FilePath> {
        let mut order = Vec::new();
        self.root.read_into(&mut order);
        order
    }

    /// Every folder and file in reading order, minus what sits under a folded folder.
    pub fn rows(&self, folded: &BTreeSet<FolderPath>, current: Option<&FilePath>) -> Vec<TreeRow> {
        let mut rows = Vec::new();
        self.root.rows_into(None, 0, folded, current, &mut rows);
        rows
    }
}

impl Folder {
    fn read_into(&self, order: &mut Vec<FilePath>) {
        for folder in self.folders.values() {
            folder.read_into(order);
        }
        order.extend(self.files.values().cloned());
    }

    fn rows_into(
        &self,
        parent: Option<&FolderPath>,
        depth: usize,
        folded: &BTreeSet<FolderPath>,
        current: Option<&FilePath>,
        rows: &mut Vec<TreeRow>,
    ) {
        for (name, first) in &self.folders {
            let (label, path, folder) = first.joined_with(name, parent);
            let is_folded = folded.contains(&path);
            rows.push(TreeRow {
                depth,
                kind: TreeRowKind::Folder {
                    path: path.clone(),
                    label,
                    is_folded,
                },
            });
            if !is_folded {
                folder.rows_into(Some(&path), depth + 1, folded, current, rows);
            }
        }
        for (name, path) in &self.files {
            rows.push(TreeRow {
                depth,
                kind: TreeRowKind::File {
                    path: path.clone(),
                    label: name.clone(),
                    is_current: current == Some(path),
                },
            });
        }
    }

    /// Follows folders that hold nothing but one folder, as an IDE joins them into one row.
    fn joined_with(&self, name: &str, parent: Option<&FolderPath>) -> (String, FolderPath, &Self) {
        let mut label = name.to_owned();
        let mut folder = self;
        while let (true, Some((only, inner))) =
            (folder.files.is_empty(), folder.folders.iter().next())
            && folder.folders.len() == 1
        {
            label.push('/');
            label.push_str(only);
            folder = inner;
        }
        let path = match parent {
            Some(parent) => FolderPath::new(&format!("{parent}/{label}")),
            None => FolderPath::new(&label),
        };
        (label, path, folder)
    }
}
