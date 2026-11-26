use std::fmt;

use crate::code_source::FileStatus;
use crate::file_path::FilePath;
use crate::ide::{Ide, IdeLaunch, NoIde};
use crate::line_number::LineNumber;
use crate::pieces::ChangedFile;
use crate::pull_request::{HostSite, RepoName, Sha};
use crate::url_path;

use super::line_location::{LineLocation, Side};

/// A file or a line of the PR's code that the reader can open. Where it opens is only the
/// `CodeOpener`'s call, so it holds no URL a view could open on its own.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CodeLink {
    /// `None` for a local review: its code is on no code host.
    on_host: Option<String>,
    /// `Err` when the working copy cannot have this line.
    in_working_copy: Result<WorkingCopyLine, NotInIde>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct WorkingCopyLine {
    path: FilePath,
    line: LineNumber,
}

/// Why code opens on the code host and not in the IDE.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum NotInIde {
    NoIdeTemplate,
    NoClone(RepoName),
    NotInWorkingCopy,
    /// A removed line, or a line of a deleted file: only the base has it.
    RemovedLine,
    /// As `RemovedLine`, in a local review, whose base is the branch's base and which has no PR.
    RemovedFromBranch,
    /// The review's own folder, which no IDE was pointed at.
    NoLocalFolder,
    /// A place found at the review's base, which the working copy is not at.
    ReadAtBase,
}

impl fmt::Display for NotInIde {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoIdeTemplate => f.write_str("no ide template in config.toml"),
            Self::NoClone(repo) => {
                write!(f, "no clone set for {repo} in config.toml")
            }
            Self::NotInWorkingCopy => f.write_str("the clone's working copy has no such file"),
            Self::RemovedLine => f.write_str("removed lines exist only at the PR's base"),
            Self::RemovedFromBranch => f.write_str("removed lines exist only at the branch's base"),
            Self::NoLocalFolder => f.write_str("no ide is set for the review's folder"),
            Self::ReadAtBase => f.write_str("the code was read at the review's base"),
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
    /// Code that could not open in the IDE, on the PR's code host.
    OnHost {
        site: HostSite,
        url: String,
        because: NotInIde,
    },
    /// A link that is not to the PR's code.
    Browser(String),
    /// Code of a local review that the IDE cannot open, and that has no code host to open on.
    NotOpenable { because: NotInIde },
}

/// What opening a destination came to, for the IDE's failure to reach the reviewer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Opened {
    InIde,
    IdeNotStarted(String),
    Elsewhere,
    /// Nothing could open it: the destination says why.
    Nowhere,
}

impl Destination {
    /// A URL is handed to `open_url`, because only the caller knows the system's opener.
    pub fn open(&self, open_url: impl FnOnce(&str)) -> Opened {
        match self {
            Self::Ide { launch, .. } => match launch.run(open_url) {
                Ok(()) => Opened::InIde,
                Err(error) => Opened::IdeNotStarted(error.to_string()),
            },
            Self::OnHost { url, .. } | Self::Browser(url) => {
                open_url(url);
                Opened::Elsewhere
            }
            Self::NotOpenable { .. } => Opened::Nowhere,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Ide { .. } => "Open in IDE",
            Self::OnHost { site, .. } => site.open_on(),
            Self::Browser(_) => "Open link",
            Self::NotOpenable { .. } => "Cannot open",
        }
    }

    /// For a tooltip: where it opens, and what to know about that.
    pub fn explanation(&self) -> String {
        match self {
            Self::Ide {
                caveat: Some(caveat),
                ..
            } => format!("{} ({caveat})", self.label()),
            Self::OnHost { site, because, .. } => format!("Opens on {}: {because}", site.name()),
            Self::NotOpenable { because } => format!("Cannot open: {because}"),
            Self::Ide { caveat: None, .. } | Self::Browser(_) => self.label().to_owned(),
        }
    }
}

/// Where code that the IDE cannot open goes: a PR's repo on its code host, or nowhere for a local review.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodeOnWeb {
    Host { site: HostSite, repo: RepoName },
    LocalClone,
}

/// Which commit the local clone has checked out, compared with the tour's head.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkingCopy {
    AtTourHead,
    /// Another commit, or no clone to ask.
    Elsewhere,
}

impl WorkingCopy {
    pub fn of(is_at_tour_head: bool) -> Self {
        if is_at_tour_head {
            Self::AtTourHead
        } else {
            Self::Elsewhere
        }
    }
}

/// The one place that decides where code opens: in the reviewer's IDE when the file is in the
/// working copy of a local clone, else on the PR's code host.
#[derive(Debug, Clone)]
pub struct CodeOpener {
    ide: Result<Ide, NoIde>,
    working_copy: WorkingCopy,
    on_web: CodeOnWeb,
    head: Sha,
}

impl CodeOpener {
    pub(super) fn new(
        ide: Result<Ide, NoIde>,
        working_copy: WorkingCopy,
        on_web: &CodeOnWeb,
        head: &Sha,
    ) -> Self {
        Self {
            ide,
            working_copy,
            on_web: on_web.clone(),
            head: head.clone(),
        }
    }

    pub fn destination(&self, link: &CodeLink) -> Destination {
        self.code_destination(link.in_working_copy.clone(), link.on_host.as_deref())
    }

    /// A file git does not hold, in the IDE when one is set.
    pub fn destination_of_file(&self, path: &std::path::Path, line: LineNumber) -> Destination {
        match &self.ide {
            Ok(ide) => Destination::Ide {
                launch: ide.launch_file(path, line),
                caveat: None,
            },
            Err(NoIde::NoTemplate) => Destination::NotOpenable {
                because: NotInIde::NoIdeTemplate,
            },
            Err(NoIde::NoClone) => Destination::NotOpenable {
                because: NotInIde::NoLocalFolder,
            },
        }
    }

    /// For links in markdown text: a link to a file of this repo on its code host, or a repo-relative
    /// path, is a code link; every other link opens as it is written.
    pub fn destination_of_url(&self, url: &str) -> Destination {
        if let Some(at) = self.file_on_host(url) {
            return self.code_destination(Ok(at), Some(url));
        }
        match repo_relative(url) {
            Some(at) => {
                let on_host = match &self.on_web {
                    CodeOnWeb::Host { site, repo } => {
                        Some(site.file_page(repo, &self.head, &at.path, Some(at.line)))
                    }
                    CodeOnWeb::LocalClone => None,
                };
                self.code_destination(Ok(at), on_host.as_deref())
            }
            None => Destination::Browser(url.to_owned()),
        }
    }

    fn code_destination(
        &self,
        in_working_copy: Result<WorkingCopyLine, NotInIde>,
        on_host: Option<&str>,
    ) -> Destination {
        match in_working_copy.and_then(|at| self.ide_launch(&at)) {
            Ok(launch) => Destination::Ide {
                launch,
                caveat: (self.working_copy == WorkingCopy::Elsewhere)
                    .then_some(IdeCaveat::CloneNotAtTourHead),
            },
            Err(because) => match (on_host, &self.on_web) {
                (Some(url), CodeOnWeb::Host { site, .. }) => Destination::OnHost {
                    site: *site,
                    url: url.to_owned(),
                    because,
                },
                _ => Destination::NotOpenable { because },
            },
        }
    }

    fn ide_launch(&self, at: &WorkingCopyLine) -> Result<IdeLaunch, NotInIde> {
        let ide = self
            .ide
            .as_ref()
            .map_err(|missing| match (missing, &self.on_web) {
                (NoIde::NoTemplate, _) => NotInIde::NoIdeTemplate,
                (NoIde::NoClone, CodeOnWeb::Host { repo, .. }) => NotInIde::NoClone(repo.clone()),
                (NoIde::NoClone, CodeOnWeb::LocalClone) => NotInIde::NoLocalFolder,
            })?;
        if !ide.holds(&at.path) {
            return Err(NotInIde::NotInWorkingCopy);
        }
        Ok(ide.launch(&at.path, at.line))
    }

    fn file_on_host(&self, url: &str) -> Option<WorkingCopyLine> {
        let CodeOnWeb::Host { site, repo } = &self.on_web else {
            return None;
        };
        let (path, line) = site.file_of_link(repo, url)?;
        Some(WorkingCopyLine { path, line })
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
    Some(WorkingCopyLine {
        path: FilePath::new(&url_path::decode(path)).ok()?,
        line: LineNumber::of_fragment(fragment),
    })
}

/// Links point at the commits the tour is pinned to, so a line stays the same line
/// even after the PR has moved on.
pub(super) struct CodeLinks<'a> {
    pub on_web: &'a CodeOnWeb,
    pub base: &'a Sha,
    pub head: &'a Sha,
}

impl CodeLinks<'_> {
    pub fn to_line(&self, file: &ChangedFile, location: &LineLocation) -> CodeLink {
        let line = Some(location.line);
        let (on_host, in_working_copy) = match location.side {
            Side::Base => (
                self.page(self.base, file.path_on(Side::Base), line),
                Err(self.removed_line()),
            ),
            Side::Head => (
                self.page(self.head, &file.path, line),
                Ok(WorkingCopyLine {
                    path: file.path.clone(),
                    line: location.line,
                }),
            ),
        };
        CodeLink {
            on_host,
            in_working_copy,
        }
    }

    /// Any place at one side's commit, as a definition or a usage found there.
    pub fn to_place(&self, side: Side, path: &FilePath, line: LineNumber) -> CodeLink {
        match side {
            Side::Base => CodeLink {
                on_host: self.page(self.base, path, Some(line)),
                in_working_copy: Err(NotInIde::ReadAtBase),
            },
            Side::Head => CodeLink {
                on_host: self.page(self.head, path, Some(line)),
                in_working_copy: Ok(WorkingCopyLine {
                    path: path.clone(),
                    line,
                }),
            },
        }
    }

    /// A deleted file only exists at base.
    pub fn to_file(&self, file: &ChangedFile) -> CodeLink {
        match file.status {
            FileStatus::Deleted => CodeLink {
                on_host: self.page(self.base, &file.path, None),
                in_working_copy: Err(self.removed_line()),
            },
            _ => CodeLink {
                on_host: self.page(self.head, &file.path, None),
                in_working_copy: Ok(WorkingCopyLine {
                    path: file.path.clone(),
                    line: LineNumber::FIRST,
                }),
            },
        }
    }

    fn removed_line(&self) -> NotInIde {
        match self.on_web {
            CodeOnWeb::Host { .. } => NotInIde::RemovedLine,
            CodeOnWeb::LocalClone => NotInIde::RemovedFromBranch,
        }
    }

    fn page(&self, sha: &Sha, path: &FilePath, line: Option<LineNumber>) -> Option<String> {
        match self.on_web {
            CodeOnWeb::Host { site, repo } => Some(site.file_page(repo, sha, path, line)),
            CodeOnWeb::LocalClone => None,
        }
    }
}
