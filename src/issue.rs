//! The issue a review is about: a GitHub issue, or a ticket elsewhere.

use std::fmt;
use std::str::FromStr;

use anyhow::{Context, bail};

use crate::github_web;
use crate::pull_request::RepoName;
use crate::review_subject::BranchName;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct IssueUrl {
    repo: RepoName,
    number: u64,
}

/// Words a branch puts before an issue's number, as in `issue-12` or `gh-12`.
const ISSUE_WORDS: [&str; 3] = ["issues", "issue", "gh"];
/// What a description writes before the issues it names, lower case: GitHub's closing keywords,
/// `part of`, and `ticket` for a ticket elsewhere.
const NAMING_WORDS: [&str; 11] = [
    "part of", "close", "closes", "closed", "fix", "fixes", "fixed", "resolve", "resolves",
    "resolved", "ticket",
];

impl IssueUrl {
    pub const fn new(repo: RepoName, number: u64) -> Self {
        Self { repo, number }
    }

    pub const fn repo(&self) -> &RepoName {
        &self.repo
    }

    pub const fn number(&self) -> u64 {
        self.number
    }

    /// The issue a branch is named after: a number that starts the branch's last part
    /// (`feat/123-totals`), or follows `issue`, `issues` or `gh` there (`issue-123`). A number
    /// followed by another, as a date's year, names none.
    pub fn of_branch(repo: &RepoName, branch: &BranchName) -> Option<Self> {
        let last_part = branch.as_str().rsplit('/').next()?.to_ascii_lowercase();
        let after_word = ISSUE_WORDS
            .iter()
            .find_map(|word| {
                last_part
                    .strip_prefix(word)?
                    .strip_prefix(['-', '_'])
                    .filter(|rest| rest.starts_with(|c: char| c.is_ascii_digit()))
            })
            .unwrap_or(&last_part);
        let digits_end = after_word
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(after_word.len());
        let (digits, rest) = after_word.split_at_checked(digits_end)?;
        let mut after_digits = rest.chars();
        let ends_the_number = match after_digits.next() {
            None => true,
            Some('-' | '_') => !after_digits.next().is_some_and(|c| c.is_ascii_digit()),
            Some(_) => false,
        };
        let number: u64 = digits.parse().ok()?;
        (ends_the_number && number > 0).then(|| Self::new(repo.clone(), number))
    }
}

impl FromStr for IssueUrl {
    type Err = anyhow::Error;

    fn from_str(url: &str) -> anyhow::Result<Self> {
        let (repo, number) = github_web::issue_of_link(url)
            .with_context(|| format!("not a GitHub issue link: {url}"))?;
        Ok(Self::new(repo, number))
    }
}

impl fmt::Display for IssueUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&github_web::issue_page(&self.repo, self.number))
    }
}

impl serde::Serialize for IssueUrl {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> serde::Deserialize<'de> for IssueUrl {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let url = String::deserialize(deserializer)?;
        url.parse().map_err(serde::de::Error::custom)
    }
}

/// A link to a ticket outside GitHub, as a team's own tracker: any http or https link.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TicketUrl(String);

impl FromStr for TicketUrl {
    type Err = anyhow::Error;

    fn from_str(url: &str) -> anyhow::Result<Self> {
        let rest = url
            .strip_prefix("https://")
            .or_else(|| url.strip_prefix("http://"))
            .with_context(|| format!("not a link: {url}"))?;
        if rest.is_empty() || rest.contains(char::is_whitespace) {
            bail!("not a link: {url}");
        }
        Ok(Self(url.to_owned()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum IssueLink {
    GitHub(IssueUrl),
    Ticket(TicketUrl),
}

impl IssueLink {
    /// What a button shows of it: `#12`, or `Ticket`.
    pub fn label(&self) -> String {
        match self {
            Self::GitHub(url) => format!("#{}", url.number()),
            Self::Ticket(_) => "Ticket".to_owned(),
        }
    }

    /// The issues a PR's description names, in its order, once each: after GitHub's closing
    /// keywords or `Part of` as `#12`, `owner/repo#12` or a link, several joined by commas or
    /// `and`; after `Ticket:` as any link. A bare `#12` is of `repo`.
    pub fn named_in(description: &str, repo: &RepoName) -> Vec<Self> {
        let mut named: Vec<(usize, Self)> = Vec::new();
        let mut line_start = 0;
        for line in description.split_inclusive('\n') {
            let lowered = line.to_ascii_lowercase();
            for word in NAMING_WORDS {
                for (at, _) in lowered.match_indices(word) {
                    let starts_a_word = lowered.get(..at).is_some_and(|before| {
                        !before.ends_with(|c: char| c.is_ascii_alphanumeric())
                    });
                    let Some(after) = line.get(at + word.len()..) else {
                        continue;
                    };
                    let ends_the_word =
                        after.starts_with(|c: char| c.is_whitespace() || c == ':' || c == '*');
                    if starts_a_word && ends_the_word {
                        named.extend(
                            Self::listed_at_start_of(after, repo)
                                .into_iter()
                                .map(|link| (line_start + at, link)),
                        );
                    }
                }
            }
            line_start += line.len();
        }
        named.sort_by_key(|(at, _)| *at);
        let mut once = Vec::new();
        for (_, link) in named {
            if !once.contains(&link) {
                once.push(link);
            }
        }
        once
    }

    /// The references a text starts with, as `#1, owner/repo#2 and <link>`.
    fn listed_at_start_of(text: &str, repo: &RepoName) -> Vec<Self> {
        text.split(|c: char| c.is_whitespace() || c == ',')
            .map(|word| {
                let target = word.split_once("](").map_or(word, |(_, target)| target);
                target.trim_matches(|c: char| "*_.:;()[]<>".contains(c))
            })
            .filter(|word| !word.is_empty() && !word.eq_ignore_ascii_case("and"))
            .map_while(|word| Self::of_reference(word, repo))
            .collect()
    }

    fn of_reference(word: &str, repo: &RepoName) -> Option<Self> {
        if let Ok(link) = word.parse() {
            return Some(link);
        }
        let (named_repo, number) = word.split_once('#')?;
        let number: u64 = number.parse().ok()?;
        let repo = if named_repo.is_empty() {
            repo.clone()
        } else {
            named_repo.parse().ok()?
        };
        (number > 0).then(|| Self::GitHub(IssueUrl::new(repo, number)))
    }
}

/// A GitHub issue's page is a GitHub issue; any other link is a ticket.
impl FromStr for IssueLink {
    type Err = anyhow::Error;

    fn from_str(url: &str) -> anyhow::Result<Self> {
        match url.parse() {
            Ok(issue) => Ok(Self::GitHub(issue)),
            Err(_) => Ok(Self::Ticket(url.parse()?)),
        }
    }
}

impl fmt::Display for IssueLink {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::GitHub(url) => url.fmt(f),
            Self::Ticket(TicketUrl(url)) => f.write_str(url),
        }
    }
}

impl serde::Serialize for IssueLink {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> serde::Deserialize<'de> for IssueLink {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let url = String::deserialize(deserializer)?;
        url.parse().map_err(serde::de::Error::custom)
    }
}
