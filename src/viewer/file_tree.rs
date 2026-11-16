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
        /// Every file under it is read.
        is_read: bool,
    },
    File {
        path: FilePath,
        label: String,
        is_current: bool,
        is_read: bool,
    },
}

#[derive(Debug, Default)]
pub struct FileTree {
    root: Folder,
}

#[derive(Debug, Default)]
struct Folder {
    folders: BTreeMap<Name, Self>,
    files: BTreeMap<Name, FilePath>,
}

/// A file or folder name, ordered as an IDE lists them: by its lowercase form first, so `a.txt`
/// comes before `B.txt`, and by the exact name when only the case differs.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Name {
    lowercase: String,
    exact: String,
}

impl Name {
    fn of(name: &str) -> Self {
        Self {
            lowercase: name.to_lowercase(),
            exact: name.to_owned(),
        }
    }

    fn as_str(&self) -> &str {
        &self.exact
    }
}

impl FileTree {
    pub fn of<'a>(paths: impl IntoIterator<Item = &'a FilePath>) -> Self {
        let mut root = Folder::default();
        for path in paths {
            let mut folder = &mut root;
            let mut names = path.as_str().split('/').peekable();
            while let Some(name) = names.next() {
                if names.peek().is_some() {
                    folder = folder.folders.entry(Name::of(name)).or_default();
                } else {
                    folder.files.insert(Name::of(name), path.clone());
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
    pub fn rows(
        &self,
        folded: &BTreeSet<FolderPath>,
        current: Option<&FilePath>,
        is_read: &dyn Fn(&FilePath) -> bool,
    ) -> Vec<TreeRow> {
        let mut rows = Vec::new();
        let look = Look {
            folded,
            current,
            is_read,
        };
        self.root.rows_into(None, 0, &look, &mut rows);
        rows
    }
}

/// What a row's look depends on, besides where it sits in the tree.
struct Look<'a> {
    folded: &'a BTreeSet<FolderPath>,
    current: Option<&'a FilePath>,
    is_read: &'a dyn Fn(&FilePath) -> bool,
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
        look: &Look<'_>,
        rows: &mut Vec<TreeRow>,
    ) {
        for (name, first) in &self.folders {
            let (label, path, folder) = first.joined_with(name.as_str(), parent);
            let is_folded = look.folded.contains(&path);
            rows.push(TreeRow {
                depth,
                kind: TreeRowKind::Folder {
                    path: path.clone(),
                    label,
                    is_folded,
                    is_read: folder.is_read(look.is_read),
                },
            });
            if !is_folded {
                folder.rows_into(Some(&path), depth + 1, look, rows);
            }
        }
        for (name, path) in &self.files {
            rows.push(TreeRow {
                depth,
                kind: TreeRowKind::File {
                    path: path.clone(),
                    label: name.as_str().to_owned(),
                    is_current: look.current == Some(path),
                    is_read: (look.is_read)(path),
                },
            });
        }
    }

    /// Every file under the folder, however deep, is read.
    fn is_read(&self, is_read: &dyn Fn(&FilePath) -> bool) -> bool {
        self.files.values().all(is_read)
            && self.folders.values().all(|inner| inner.is_read(is_read))
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
            label.push_str(only.as_str());
            folder = inner;
        }
        let path = match parent {
            Some(parent) => FolderPath::new(&format!("{parent}/{label}")),
            None => FolderPath::new(&label),
        };
        (label, path, folder)
    }
}
