use std::fmt;
use std::path::Path;

use crate::code_source::FileStatus;
use crate::ide::{Ide, IdeLaunch, NoIde};
use crate::pieces::ChangedFile;
use crate::pull_request::{PullRequestUrl, RepoName, Sha};

use super::step_page::{LineLocation, Side};

/// A file or a line of the PR's code that the reader can open. Where it opens is only the
/// `CodeOpener`'s call, so it holds no URL a view could open on its own.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CodeLink {
    on_github: String,
    /// `Err` when the working copy cannot have this line.
    in_working_copy: Result<WorkingCopyLine, NotInIde>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct WorkingCopyLine {
    path: String,
    line: usize,
}

/// Why code opens on GitHub and not in the IDE.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum NotInIde {
    NoIdeTemplate,
    NoClone(RepoName),
    NotInWorkingCopy,
    /// A removed line, or a line of a deleted file: only the base has it.
    RemovedLine,
}

impl fmt::Display for NotInIde {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoIdeTemplate => f.write_str("no ide template in ~/.guided-review/config.toml"),
            Self::NoClone(repo) => {
                write!(f, "no clone set for {repo} in ~/.guided-review/config.toml")
            }
            Self::NotInWorkingCopy => f.write_str("the clone's working copy has no such file"),
            Self::RemovedLine => f.write_str("removed lines exist only at the PR's base"),
        }
    }
}

/// Why code that opens in the IDE may not look there as the tour shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdeCaveat {
    /// Checked out at another commit, or with uncommitted changes: its lines may have moved.
    CloneNotAtTourHead,
}

impl fmt::Display for IdeCaveat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CloneNotAtTourHead => {
                f.write_str("clone is not at the tour head, lines may differ")
            }
        }
    }
}

/// Where opening a code link, or any link of the text, takes the reader.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Destination {
    Ide {
        launch: IdeLaunch,
        caveat: Option<IdeCaveat>,
    },
    /// Code that could not open in the IDE.
    GitHub { url: String, because: NotInIde },
    /// A link that is not to the PR's code.
    Browser(String),
}

impl Destination {
    /// A URL is handed to `open_url`, because only the caller knows the system's opener.
    pub fn open(&self, open_url: impl FnOnce(&str)) {
        match self {
            Self::Ide { launch, .. } => launch.run(open_url),
            Self::GitHub { url, .. } | Self::Browser(url) => open_url(url),
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Ide { .. } => "Open in IDE",
            Self::GitHub { .. } => "Open on GitHub",
            Self::Browser(_) => "Open link",
        }
    }

    /// For a tooltip: where it opens, and what to know about that.
    pub fn explanation(&self) -> String {
        match self {
            Self::Ide {
                caveat: Some(caveat),
                ..
            } => format!("{} ({caveat})", self.label()),
            Self::GitHub { because, .. } => format!("Opens on GitHub: {because}"),
            Self::Ide { caveat: None, .. } | Self::Browser(_) => self.label().to_string(),
        }
    }
}

/// The one place that decides where code opens: in the reviewer's IDE when the file is in the
/// working copy of a local clone, else on GitHub.
#[derive(Debug, Clone)]
pub struct CodeOpener {
    ide: Result<Ide, NoIde>,
    working_copy_is_at_tour_head: bool,
    repo: RepoName,
    head: Sha,
}

impl CodeOpener {
    pub(super) fn new(
        ide: Result<Ide, NoIde>,
        working_copy_is_at_tour_head: bool,
        pr: &PullRequestUrl,
        head: &Sha,
    ) -> Self {
        Self {
            ide,
            working_copy_is_at_tour_head,
            repo: pr.repo.clone(),
            head: head.clone(),
        }
    }

    pub fn destination(&self, link: &CodeLink) -> Destination {
        self.code_destination(link.in_working_copy.clone(), &link.on_github)
    }

    /// For links in markdown text: a GitHub link to a file of this repo, or a repo-relative path,
    /// is a code link; every other link opens as it is written.
    pub fn destination_of_url(&self, url: &str) -> Destination {
        if let Some(at) = self.github_blob(url) {
            return self.code_destination(Ok(at), url);
        }
        match repo_relative(url) {
            Some(at) => {
                let on_github = format!(
                    "{}#L{}",
                    blob_url(&self.repo, &self.head, &at.path),
                    at.line
                );
                self.code_destination(Ok(at), &on_github)
            }
            None => Destination::Browser(url.to_string()),
        }
    }

    fn code_destination(
        &self,
        in_working_copy: Result<WorkingCopyLine, NotInIde>,
        on_github: &str,
    ) -> Destination {
        match in_working_copy.and_then(|at| self.ide_launch(&at)) {
            Ok(launch) => Destination::Ide {
                launch,
                caveat: (!self.working_copy_is_at_tour_head)
                    .then_some(IdeCaveat::CloneNotAtTourHead),
            },
            Err(because) => Destination::GitHub {
                url: on_github.to_string(),
                because,
            },
        }
    }

    fn ide_launch(&self, at: &WorkingCopyLine) -> Result<IdeLaunch, NotInIde> {
        let ide = self.ide.as_ref().map_err(|missing| match missing {
            NoIde::NoTemplate => NotInIde::NoIdeTemplate,
            NoIde::NoClone => NotInIde::NoClone(self.repo.clone()),
        })?;
        let path = Path::new(&at.path);
        if !ide.holds(path) {
            return Err(NotInIde::NotInWorkingCopy);
        }
        Ok(ide.launch(path, at.line))
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
                Err(NotInIde::RemovedLine),
            ),
            Side::Head => (
                self.blob(self.head, &file.path),
                Ok(WorkingCopyLine {
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
                in_working_copy: Err(NotInIde::RemovedLine),
            },
            _ => CodeLink {
                on_github: self.blob(self.head, &file.path),
                in_working_copy: Ok(WorkingCopyLine {
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
