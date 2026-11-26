//! GitHub's web addresses: a PR's page and checks, a file at a commit, an issue, a git remote, and
//! the ref a PR's head is fetched by. `host_web` asks them of a PR on GitHub; the GitHub adapter
//! (`github`) sits above the viewer, these below it, so they are a module of their own.

use anyhow::{Context, bail};

use crate::file_path::FilePath;
use crate::line_number::LineNumber;
use crate::pull_request::{RepoName, Sha};
use crate::url_path;

const SITE: &str = "https://github.com/";

pub fn is_link(url: &str) -> bool {
    url.starts_with(SITE)
}

/// `https://github.com/{owner}/{repo}/pull/{number}`, with any page of the PR after it: the PR's repo
/// and number.
pub fn pull_request_of(url: &str) -> anyhow::Result<(RepoName, u64)> {
    let not_a_pull_request = || format!("not a GitHub PR link: {url}");
    let path = url.strip_prefix(SITE).with_context(not_a_pull_request)?;
    let segments: Vec<&str> = path.split('/').collect();
    let [owner, name, "pull", number, ..] = segments.as_slice() else {
        bail!(not_a_pull_request());
    };
    let number = number.parse().with_context(not_a_pull_request)?;
    Ok((RepoName::new(owner, name), number))
}

pub fn pull_request_page(repo: &RepoName, number: u64) -> String {
    format!("{SITE}{repo}/pull/{number}")
}

/// `owner/repo#7`, as GitHub links a PR in text.
pub fn pull_request_reference(repo: &RepoName, number: u64) -> String {
    format!("{repo}#{number}")
}

pub fn checks_page(repo: &RepoName, number: u64) -> String {
    format!("{}/checks", pull_request_page(repo, number))
}

/// GitHub keeps every PR's head under this ref of the base repo, a fork's PR too.
pub fn head_ref(number: u64) -> String {
    format!("refs/pull/{number}/head")
}

pub fn file_page(repo: &RepoName, sha: &Sha, path: &FilePath, line: Option<LineNumber>) -> String {
    let page = format!(
        "{SITE}{repo}/blob/{sha}/{}",
        url_path::encode(path.as_str())
    );
    match line {
        Some(line) => format!("{page}#L{line}"),
        None => page,
    }
}

/// `https://github.com/{owner}/{repo}/blob/{ref}/{path}#L{n}` of `repo`; a ref with a slash in it is
/// read as its first segment, as GitHub cannot tell those apart either.
pub fn file_of_link(repo: &RepoName, url: &str) -> Option<(FilePath, LineNumber)> {
    let rest = url.strip_prefix(SITE)?;
    let (address, fragment) = rest.split_once('#').unwrap_or((rest, ""));
    let mut segments = address.splitn(5, '/');
    let (owner, name, kind, _ref, path) = (
        segments.next()?,
        segments.next()?,
        segments.next()?,
        segments.next()?,
        segments.next()?,
    );
    let is_this_repo =
        owner.eq_ignore_ascii_case(repo.namespace()) && name.eq_ignore_ascii_case(repo.name());
    if !(is_this_repo && kind == "blob") {
        return None;
    }
    Some((
        FilePath::new(&url_path::decode(path)).ok()?,
        LineNumber::of_fragment(fragment),
    ))
}

/// The repo a git remote on GitHub points at, over ssh or https.
pub fn repo_of_remote(url: &str) -> Option<RepoName> {
    let after_host = url
        .strip_prefix("git@github.com:")
        .or_else(|| url.split_once("github.com/").map(|(_, rest)| rest))?;
    let mut segments = after_host.trim_end_matches('/').split('/');
    let (owner, name) = (segments.next()?, segments.next()?);
    if segments.next().is_some() || owner.is_empty() || name.is_empty() {
        return None;
    }
    Some(RepoName::new(owner, name))
}

/// `owner/name`, the only repo path GitHub has.
pub fn repo_named(text: &str) -> Option<RepoName> {
    let repo: RepoName = text.parse().ok()?;
    (!repo.namespace().contains('/')).then_some(repo)
}

pub fn issue_page(repo: &RepoName, number: u64) -> String {
    format!("{SITE}{repo}/issues/{number}")
}

/// `https://github.com/{owner}/{repo}/issues/{number}`: the issue's repo and number.
pub fn issue_of_link(url: &str) -> Option<(RepoName, u64)> {
    let path = url.strip_prefix(SITE)?;
    let segments: Vec<&str> = path.trim_end_matches('/').split('/').collect();
    let [owner, name, "issues", number] = segments.as_slice() else {
        return None;
    };
    Some((RepoName::new(owner, name), number.parse().ok()?))
}
