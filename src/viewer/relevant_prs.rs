use super::pr_origin::PrAuthor;
use crate::pull_request::PullRequestUrl;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RelevantPullRequests {
    pub pull_requests: Vec<RelevantPullRequest>,
    /// Teams whose review requests were not searched, because only so many searches are made.
    pub teams_left_out: Vec<String>,
}

impl RelevantPullRequests {
    /// The lists of every code host as one, newest update first. A host that fails is left out, since
    /// a reviewer need not have an account on every host; the list fails only when every host does.
    pub fn merged<E>(answers: impl IntoIterator<Item = Result<Self, E>>) -> Result<Self, E> {
        let mut merged: Option<Self> = None;
        let mut first_error = None;
        for answer in answers {
            match answer {
                Ok(list) => {
                    let into = merged.get_or_insert_with(Self::default);
                    into.pull_requests.extend(list.pull_requests);
                    into.teams_left_out.extend(list.teams_left_out);
                }
                Err(error) => {
                    first_error.get_or_insert(error);
                }
            }
        }
        match (merged, first_error) {
            (Some(mut merged), _) => {
                merged
                    .pull_requests
                    .sort_by(|newer, older| older.updated_at.cmp(&newer.updated_at));
                Ok(merged)
            }
            (None, Some(error)) => Err(error),
            (None, None) => Ok(Self::default()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelevantPullRequest {
    pub url: PullRequestUrl,
    pub title: String,
    pub author: PrAuthor,
    pub is_draft: bool,
    /// As GitHub writes it, `2026-11-05T13:35:33Z`, which sorts as time does.
    pub updated_at: String,
    pub why_listed: WhyListed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WhyListed {
    /// The viewer wrote it.
    Mine,
    /// The viewer or one of their teams is asked to review it.
    ReviewRequested,
    /// The viewer is in it some other way: reviewed, commented, assigned or mentioned.
    Involved,
}
