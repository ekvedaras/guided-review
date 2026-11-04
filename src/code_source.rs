use std::fmt;
use std::path::Path;

use crate::pull_request::Sha;

pub trait CodeSource: Send + Sync {
    fn file_at(&self, sha: &Sha, path: &str) -> anyhow::Result<String>;
    fn blob_at(&self, sha: &Sha, path: &str) -> anyhow::Result<Vec<u8>>;
    /// What a file is at `sha`, without reading it: the blob id names its content, so equal ids are equal bytes.
    fn blob_info_at(&self, sha: &Sha, path: &str) -> anyhow::Result<BlobInfo>;
    fn diff(&self, base: &Sha, head: &Sha) -> anyhow::Result<Vec<FileDiff>>;
    fn merge_base(&self, a: &Sha, b: &Sha) -> anyhow::Result<Sha>;
    /// True when the checked-out files are exactly `head`, so they can be opened in an IDE.
    fn working_tree_is_at(&self, head: &Sha) -> bool;
    /// The clone this source reads, so a file can be opened in an IDE. `None` for GitHub.
    fn local_clone(&self) -> Option<&Path> {
        None
    }

    /// Says which files will be read next, so a source that can fetch them together does.
    fn read_ahead(&self, _files: &[FileAt]) {}
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlobInfo {
    pub id: String,
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FileAt {
    pub sha: Sha,
    pub path: String,
}

#[derive(Debug, Default)]
pub struct FileDiff {
    pub old_path: Option<String>,
    pub new_path: Option<String>,
    pub binary: bool,
    pub removed: Vec<Line>,
    pub added: Vec<Line>,
    pub hunks: Vec<Hunk>,
}

#[derive(Debug, Clone)]
pub struct Hunk {
    /// `@@ -2 +2 @@`
    pub header: String,
    /// As git writes it: the first added line, or the line above the cut when nothing was added.
    pub head_start: usize,
    pub removed: Vec<Line>,
    pub added: Vec<Line>,
}

impl Hunk {
    /// The last unchanged line at head above this hunk; 0 at the top of the file.
    pub fn head_line_above(&self) -> usize {
        if self.added.is_empty() {
            self.head_start
        } else {
            self.head_start - 1
        }
    }

    /// The first unchanged line at head below this hunk.
    pub fn head_line_below(&self) -> usize {
        self.head_line_above() + self.added.len() + 1
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileStatus {
    Added,
    Modified,
    Renamed,
    Deleted,
}

impl fmt::Display for FileStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Added => "added",
            Self::Modified => "modified",
            Self::Renamed => "renamed",
            Self::Deleted => "deleted",
        })
    }
}

#[derive(Debug, Clone)]
pub struct Line {
    pub number: usize,
    pub text: String,
}

impl FileDiff {
    pub fn path(&self) -> &str {
        self.new_path
            .as_deref()
            .or(self.old_path.as_deref())
            .unwrap_or_default()
    }

    pub fn status(&self) -> FileStatus {
        match (&self.old_path, &self.new_path) {
            (None, _) => FileStatus::Added,
            (_, None) => FileStatus::Deleted,
            (Some(old), Some(new)) if old != new => FileStatus::Renamed,
            _ => FileStatus::Modified,
        }
    }
}

/// Parses `git diff` output. Context lines are skipped, so any `-U<n>` works.
pub fn parse_unified_diff(patch: &str) -> Vec<FileDiff> {
    let mut files: Vec<FileDiff> = Vec::new();
    let mut old_line = 0;
    let mut new_line = 0;
    let mut old_left = 0;
    let mut new_left = 0;

    for raw in patch.lines() {
        if old_left > 0 || new_left > 0 {
            let Some(file) = files.last_mut() else {
                break;
            };
            if let Some(text) = raw.strip_prefix('-') {
                let line = Line {
                    number: old_line,
                    text: text.to_string(),
                };
                if let Some(hunk) = file.hunks.last_mut() {
                    hunk.removed.push(line.clone());
                }
                file.removed.push(line);
                old_line += 1;
                old_left -= 1;
            } else if let Some(text) = raw.strip_prefix('+') {
                let line = Line {
                    number: new_line,
                    text: text.to_string(),
                };
                if let Some(hunk) = file.hunks.last_mut() {
                    hunk.added.push(line.clone());
                }
                file.added.push(line);
                new_line += 1;
                new_left -= 1;
            } else if raw.starts_with(' ') || raw.is_empty() {
                old_line += 1;
                new_line += 1;
                old_left -= 1;
                new_left -= 1;
            }
            continue;
        }

        if let Some(paths) = raw.strip_prefix("diff --git ") {
            let path = same_path_in_git_header(paths);
            files.push(FileDiff {
                old_path: path.clone(),
                new_path: path,
                ..FileDiff::default()
            });
            continue;
        }
        let Some(file) = files.last_mut() else {
            continue;
        };
        if raw.starts_with("new file mode") {
            file.old_path = None;
        } else if raw.starts_with("deleted file mode") {
            file.new_path = None;
        } else if let Some(path) = raw.strip_prefix("rename from ") {
            file.old_path = Some(unquote(path));
        } else if let Some(path) = raw.strip_prefix("rename to ") {
            file.new_path = Some(unquote(path));
        } else if let Some(path) = raw.strip_prefix("--- ") {
            file.old_path = path_after_prefix(path, "a/");
        } else if let Some(path) = raw.strip_prefix("+++ ") {
            file.new_path = path_after_prefix(path, "b/");
        } else if raw.starts_with("Binary files ") {
            file.binary = true;
        } else if let Some((old, new)) = hunk_ranges(raw) {
            (old_line, old_left) = old;
            (new_line, new_left) = new;
            file.hunks.push(Hunk {
                header: hunk_header(raw),
                head_start: new.0,
                removed: Vec::new(),
                added: Vec::new(),
            });
        }
    }
    files
}

/// `a/<path> b/<path>` when the path did not change; renames get their paths from `rename from/to`.
fn same_path_in_git_header(paths: &str) -> Option<String> {
    let paths = unquote_pair(paths);
    let half = paths.len().checked_sub(5)? / 2;
    let (old, new) = (paths.get(2..2 + half)?, paths.get(half + 5..)?);
    (old == new).then(|| old.to_string())
}

/// Git ends `---` and `+++` paths that hold a space with a tab.
fn path_after_prefix(path: &str, prefix: &str) -> Option<String> {
    unquote(path.trim_end_matches('\t'))
        .strip_prefix(prefix)
        .map(str::to_string)
}

fn unquote_pair(paths: &str) -> String {
    if !paths.starts_with('"') {
        return paths.to_string();
    }
    match paths[1..].find("\" ") {
        Some(end) => format!(
            "{} {}",
            unquote(&paths[..end + 2]),
            unquote(&paths[end + 3..])
        ),
        None => paths.to_string(),
    }
}

fn unquote(path: &str) -> String {
    let Some(inner) = path.strip_prefix('"').and_then(|p| p.strip_suffix('"')) else {
        return path.to_string();
    };
    let mut out = String::new();
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('t') => out.push('\t'),
            Some('n') => out.push('\n'),
            Some(escaped) => out.push(escaped),
            None => {}
        }
    }
    out
}

/// `@@ -2 +2 @@`: the hunk header without git's trailing function context.
fn hunk_header(raw: &str) -> String {
    match raw.split_once(" @@") {
        Some((ranges, _)) => format!("{ranges} @@"),
        None => raw.to_string(),
    }
}

/// `@@ -start[,count] +start[,count] @@` → ((old start, old count), (new start, new count))
fn hunk_ranges(raw: &str) -> Option<((usize, usize), (usize, usize))> {
    let ranges = raw.strip_prefix("@@ -")?.split(" @@").next()?;
    let (old, new) = ranges.split_once(" +")?;
    Some((range(old)?, range(new)?))
}

fn range(text: &str) -> Option<(usize, usize)> {
    match text.split_once(',') {
        Some((start, count)) => Some((start.parse().ok()?, count.parse().ok()?)),
        None => Some((text.parse().ok()?, 1)),
    }
}
