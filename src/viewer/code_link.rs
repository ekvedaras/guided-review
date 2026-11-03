use std::path::Path;

use crate::code_source::FileStatus;
use crate::ide::{Ide, IdeLaunch};
use crate::pieces::ChangedFile;
use crate::pull_request::{PullRequestUrl, RepoName, Sha};

use super::step_page::{LineLocation, Side};

/// A file or a line of the PR's code that the reader can open. Where it opens is only the
/// `CodeOpener`'s call, so it holds no URL a view could open on its own.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CodeLink {
    on_github: String,
    /// `None` when the working copy cannot have this line: it was removed, or its file deleted.
    in_working_copy: Option<WorkingCopyLine>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct WorkingCopyLine {
    path: String,
    line: usize,
}

/// Where opening a code link, or any link of the text, takes the reader.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Destination {
    Ide(IdeLaunch),
    Browser(String),
}

impl Destination {
    /// A URL is handed to `open_url`, because only the caller knows the system's opener.
    pub fn open(&self, open_url: impl FnOnce(&str)) {
        match self {
            Self::Ide(launch) => launch.run(open_url),
            Self::Browser(url) => open_url(url),
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Ide(_) => "Open in IDE",
            Self::Browser(_) => "Open on GitHub",
        }
    }
}

/// The one place that decides where code opens: in the reviewer's IDE when the file is in the
/// working copy of a local clone, else on GitHub.
#[derive(Debug, Clone)]
pub struct CodeOpener {
    ide: Option<Ide>,
    repo: RepoName,
    head: Sha,
}

impl CodeOpener {
    pub(super) fn new(ide: Option<Ide>, pr: &PullRequestUrl, head: &Sha) -> Self {
        Self {
            ide,
            repo: pr.repo.clone(),
            head: head.clone(),
        }
    }

    pub fn destination(&self, link: &CodeLink) -> Destination {
        link.in_working_copy
            .as_ref()
            .and_then(|at| self.in_ide(&at.path, at.line))
            .unwrap_or_else(|| Destination::Browser(link.on_github.clone()))
    }

    /// For links in markdown text: a GitHub link to a file of this repo, or a repo-relative path,
    /// is a code link; every other link opens as it is written.
    pub fn destination_of_url(&self, url: &str) -> Destination {
        if let Some(at) = self.github_blob(url) {
            return self
                .in_ide(&at.path, at.line)
                .unwrap_or_else(|| Destination::Browser(url.to_string()));
        }
        match repo_relative(url) {
            Some(at) => self.in_ide(&at.path, at.line).unwrap_or_else(|| {
                Destination::Browser(format!(
                    "{}#L{}",
                    blob_url(&self.repo, &self.head, &at.path),
                    at.line
                ))
            }),
            None => Destination::Browser(url.to_string()),
        }
    }

    fn in_ide(&self, path: &str, line: usize) -> Option<Destination> {
        let ide = self.ide.as_ref()?;
        let path = Path::new(path);
        ide.holds(path)
            .then(|| Destination::Ide(ide.launch(path, line)))
    }

    /// `https://github.com/{owner}/{repo}/blob/{ref}/{path}#L{n}`; a ref with a slash in it is
    /// read as its first segment, as GitHub cannot tell those apart either.
    fn github_blob(&self, url: &str) -> Option<WorkingCopyLine> {
        let rest = url.strip_prefix("https://github.com/")?;
        let (address, fragment) = rest.split_once('#').unwrap_or((rest, ""));
        let mut segments = address.splitn(5, '/');
        let (owner, name, kind, _ref, path) = (
            segments.next()?,
            segments.next()?,
            segments.next()?,
            segments.next()?,
            segments.next()?,
        );
        let is_this_repo = owner.eq_ignore_ascii_case(self.repo.owner())
            && name.eq_ignore_ascii_case(self.repo.name());
        (is_this_repo && kind == "blob" && !path.is_empty()).then(|| WorkingCopyLine {
            path: percent_decode(path),
            line: line_of_fragment(fragment),
        })
    }
}

fn repo_relative(url: &str) -> Option<WorkingCopyLine> {
    let has_scheme = url
        .split_once(':')
        .is_some_and(|(scheme, _)| scheme.chars().all(|c| c.is_ascii_alphanumeric()));
    if has_scheme || url.starts_with('#') || url.starts_with("//") {
        return None;
    }
    let (path, fragment) = url.split_once('#').unwrap_or((url, ""));
    let path = path.trim_start_matches("./").trim_start_matches('/');
    (!path.is_empty()).then(|| WorkingCopyLine {
        path: percent_decode(path),
        line: line_of_fragment(fragment),
    })
}

/// `L12` or `L12-L20` opens at line 12; anything else at the top of the file.
fn line_of_fragment(fragment: &str) -> usize {
    fragment
        .strip_prefix('L')
        .map(|lines| lines.split('-').next().unwrap_or_default())
        .and_then(|line| line.parse().ok())
        .unwrap_or(1)
}

/// Links point at the commits the tour is pinned to, so a line stays the same line
/// even after the PR has moved on.
pub(super) struct CodeLinks<'a> {
    pub pr: &'a PullRequestUrl,
    pub base: &'a Sha,
    pub head: &'a Sha,
}

impl CodeLinks<'_> {
    pub fn to_line(&self, file: &ChangedFile, location: &LineLocation) -> CodeLink {
        let (at, in_working_copy) = match location.side {
            Side::Base => (
                self.blob(self.base, file.old_path.as_deref().unwrap_or(&file.path)),
                None,
            ),
            Side::Head => (
                self.blob(self.head, &file.path),
                Some(WorkingCopyLine {
                    path: file.path.clone(),
                    line: location.line,
                }),
            ),
        };
        CodeLink {
            on_github: format!("{at}#L{}", location.line),
            in_working_copy,
        }
    }

    /// A deleted file only exists at base.
    pub fn to_file(&self, file: &ChangedFile) -> CodeLink {
        match file.status {
            FileStatus::Deleted => CodeLink {
                on_github: self.blob(self.base, &file.path),
                in_working_copy: None,
            },
            _ => CodeLink {
                on_github: self.blob(self.head, &file.path),
                in_working_copy: Some(WorkingCopyLine {
                    path: file.path.clone(),
                    line: 1,
                }),
            },
        }
    }

    fn blob(&self, sha: &Sha, path: &str) -> String {
        blob_url(&self.pr.repo, sha, path)
    }
}

fn blob_url(repo: &RepoName, sha: &Sha, path: &str) -> String {
    format!(
        "https://github.com/{repo}/blob/{sha}/{}",
        percent_encode_path(path)
    )
}

pub(crate) fn percent_encode_path(path: &str) -> String {
    path.bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => {
                char::from(byte).to_string()
            }
            _ => format!("%{byte:02X}"),
        })
        .collect()
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let hex = bytes
            .get(index + 1..index + 3)
            .and_then(|hex| std::str::from_utf8(hex).ok())
            .and_then(|hex| u8::from_str_radix(hex, 16).ok());
        match (bytes[index], hex) {
            (b'%', Some(byte)) => {
                decoded.push(byte);
                index += 3;
            }
            (byte, _) => {
                decoded.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&decoded).into_owned()
}
