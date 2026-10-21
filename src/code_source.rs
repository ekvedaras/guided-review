use std::fmt;

use crate::pull_request::Sha;

pub trait CodeSource {
    fn file_at(&self, sha: &Sha, path: &str) -> anyhow::Result<String>;
    fn diff(&self, base: &Sha, head: &Sha) -> anyhow::Result<Vec<FileDiff>>;
    fn merge_base(&self, a: &Sha, b: &Sha) -> anyhow::Result<Sha>;
}

#[derive(Debug, Default)]
pub struct FileDiff {
    pub old_path: Option<String>,
    pub new_path: Option<String>,
    pub binary: bool,
    pub removed: Vec<Line>,
    pub added: Vec<Line>,
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

#[derive(Debug)]
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

/// Parses `git diff -U0` output.
pub fn parse_unified_diff(patch: &str) -> Vec<FileDiff> {
    let mut files: Vec<FileDiff> = Vec::new();
    let mut old_line = 0;
    let mut new_line = 0;
    let mut old_left = 0;
    let mut new_left = 0;

    for raw in patch.lines() {
        if old_left > 0 || new_left > 0 {
            let file = files.last_mut().expect("hunk inside a file");
            if let Some(text) = raw.strip_prefix('-') {
                file.removed.push(Line {
                    number: old_line,
                    text: text.to_string(),
                });
                old_line += 1;
                old_left -= 1;
            } else if let Some(text) = raw.strip_prefix('+') {
                file.added.push(Line {
                    number: new_line,
                    text: text.to_string(),
                });
                new_line += 1;
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
