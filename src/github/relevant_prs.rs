use std::collections::HashMap;

use anyhow::Context;
use serde::Deserialize;

use super::GitHub;
use crate::json::at;
use crate::pull_request::PullRequestUrl;
use crate::viewer::{
    GitHubError, RelevantPullRequest, RelevantPullRequests, RelevantPullRequestsPort, WhyListed,
};

const TEAM_SEARCH_LIMIT: usize = 10;
const SEARCH_SIZE: u32 = 100;
const OPEN_PRS: &str = "is:open is:pr archived:false";

const TEAMS_QUERY: &str = "query {
    viewer {
        login
        organizations(first: 10) {
            nodes { teams(first: 50, role: MEMBER) { nodes { combinedSlug } } }
        }
    }
}";

const PULL_REQUEST_FIELDS: &str = "fragment pr on PullRequest {
    number
    url
    title
    isDraft
    updatedAt
    author { login }
    reviewRequests(first: 20) {
        nodes { requestedReviewer { ... on User { login } ... on Team { combinedSlug } } }
    }
}";

#[derive(Deserialize)]
struct PullRequestJson {
    url: String,
    title: String,
    #[serde(rename = "isDraft")]
    is_draft: bool,
    #[serde(rename = "updatedAt")]
    updated_at: String,
    author: Option<LoginJson>,
    #[serde(rename = "reviewRequests")]
    review_requests: ReviewRequestsJson,
}

#[derive(Deserialize)]
struct LoginJson {
    login: String,
}

#[derive(Deserialize)]
struct ReviewRequestsJson {
    nodes: Vec<ReviewRequestJson>,
}

#[derive(Deserialize)]
struct ReviewRequestJson {
    #[serde(rename = "requestedReviewer")]
    requested_reviewer: Option<ReviewerJson>,
}

#[derive(Deserialize)]
struct ReviewerJson {
    login: Option<String>,
    #[serde(rename = "combinedSlug")]
    combined_slug: Option<String>,
}

impl RelevantPullRequestsPort for GitHub {
    fn relevant_pull_requests(&self) -> Result<RelevantPullRequests, GitHubError> {
        let teams = self.teams_of_viewer()?;
        let (searched, left_out) = teams.split_at(teams.len().min(TEAM_SEARCH_LIMIT));

        let mut searches = vec![format!("involves:@me {OPEN_PRS}")];
        searches.extend(
            searched
                .iter()
                .map(|team| format!("team-review-requested:{team} {OPEN_PRS}")),
        );
        let declarations: Vec<String> = (0..searches.len())
            .map(|index| format!("$s{index}: String!"))
            .collect();
        let aliases: Vec<String> = (0..searches.len())
            .map(|index| {
                format!(
                    "s{index}: search(query: $s{index}, type: ISSUE, first: {SEARCH_SIZE}) {{ nodes {{ ...pr }} }}"
                )
            })
            .collect();
        let query = format!(
            "query({}) {{ viewer {{ login }} {} }} {PULL_REQUEST_FIELDS}",
            declarations.join(", "),
            aliases.join(" ")
        );
        let variables: serde_json::Map<String, serde_json::Value> = searches
            .into_iter()
            .enumerate()
            .map(|(index, search)| (format!("s{index}"), search.into()))
            .collect();

        let data = self
            .graphql(&query, &variables.into())
            .map_err(|error| error.context("could not list the relevant pull requests"))?;

        let me = at(&data, "/viewer/login").as_str().unwrap_or_default();
        let mut by_url: HashMap<String, RelevantPullRequest> = HashMap::new();
        for search in 0..=searched.len() {
            let nodes = at(&data, &format!("/s{search}/nodes"));
            for node in nodes.as_array().into_iter().flatten() {
                // A search that is not about a PR answers `{}` for the fragment.
                let Ok(json) = PullRequestJson::deserialize(node) else {
                    continue;
                };
                if by_url.contains_key(&json.url) {
                    continue;
                }
                let url = json
                    .url
                    .parse::<PullRequestUrl>()
                    .with_context(|| format!("GitHub listed {}", json.url))?;
                let why_listed = why_listed(&json, me, teams.as_slice());
                by_url.insert(
                    json.url,
                    RelevantPullRequest {
                        url,
                        title: json.title,
                        author: json.author.map_or_else(|| "ghost".to_owned(), |a| a.login),
                        is_draft: json.is_draft,
                        updated_at: json.updated_at,
                        why_listed,
                    },
                );
            }
        }
        let mut pull_requests: Vec<_> = by_url.into_values().collect();
        pull_requests.sort_by(|a, b| {
            b.updated_at
                .cmp(&a.updated_at)
                .then_with(|| a.url.to_string().cmp(&b.url.to_string()))
        });
        Ok(RelevantPullRequests {
            pull_requests,
            teams_left_out: left_out.to_vec(),
        })
    }
}

fn why_listed(pr: &PullRequestJson, me: &str, my_teams: &[String]) -> WhyListed {
    if pr.author.as_ref().is_some_and(|author| author.login == me) {
        return WhyListed::Mine;
    }
    let asked_me_or_my_team = pr
        .review_requests
        .nodes
        .iter()
        .filter_map(|request| request.requested_reviewer.as_ref())
        .any(|reviewer| {
            reviewer.login.as_deref() == Some(me)
                || reviewer
                    .combined_slug
                    .as_ref()
                    .is_some_and(|slug| my_teams.contains(slug))
        });
    if asked_me_or_my_team {
        WhyListed::ReviewRequested
    } else {
        WhyListed::Involved
    }
}

impl GitHub {
    /// The teams the viewer is a member of, read from GitHub once per app run.
    fn teams_of_viewer(&self) -> Result<Vec<String>, GitHubError> {
        let mut known = self
            .viewer_teams
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(teams) = known.as_ref() {
            return Ok(teams.clone());
        }
        let data = self
            .graphql(TEAMS_QUERY, &serde_json::Value::Null)
            .map_err(|error| error.context("could not read your teams from GitHub"))?;
        let teams: Vec<String> = at(&data, "/viewer/organizations/nodes")
            .as_array()
            .into_iter()
            .flatten()
            .flat_map(|organization| {
                at(organization, "/teams/nodes")
                    .as_array()
                    .into_iter()
                    .flatten()
            })
            .filter_map(|team| at(team, "/combinedSlug").as_str().map(str::to_owned))
            .collect();
        *known = Some(teams.clone());
        Ok(teams)
    }
}
