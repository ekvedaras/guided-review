//! A GitHub issue a review is about.

use std::fmt;
use std::str::FromStr;

use anyhow::{Context, bail};

use crate::pull_request::RepoName;
use crate::review_subject::BranchName;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct IssueUrl {
    repo: RepoName,
    number: u64,
}

/// Words a branch puts before an issue's number, as in `issue-12` or `gh-12`.
const ISSUE_WORDS: [&str; 3] = ["issues", "issue", "gh"];

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
        let not_an_issue = || format!("not a GitHub issue link: {url}");
        let path = url
            .strip_prefix("https://github.com/")
            .with_context(not_an_issue)?;
        let segments: Vec<&str> = path.trim_end_matches('/').split('/').collect();
        let [owner, name, "issues", number] = segments.as_slice() else {
            bail!(not_an_issue());
        };
        let number = number.parse().with_context(not_an_issue)?;
        Ok(Self::new(RepoName::new(owner, name), number))
    }
}

impl fmt::Display for IssueUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "https://github.com/{}/issues/{}", self.repo, self.number)
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
