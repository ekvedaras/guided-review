use std::collections::HashMap;
use std::process::Command;
use std::sync::{Arc, Mutex};

use anyhow::Context;
use serde::Deserialize;

use crate::code_source::{CodeSource, FileDiff, parse_unified_diff};
use crate::pull_request::{PullRequestTips, PullRequestUrl, RepoName, Sha};
use crate::viewer::{
    ReviewThread, Side, ThreadComment, ViewedFile, ViewedState, ViewedSyncUnavailable,
};

#[derive(Clone)]
pub struct GitHub {
    api: String,
    token: Option<String>,
    /// GraphQL mutations address a PR by node id, so it is looked up once per PR.
    pull_request_ids: Arc<Mutex<HashMap<String, String>>>,
}

#[derive(Deserialize)]
struct PullJson {
    base: CommitRefJson,
    head: CommitRefJson,
}

#[derive(Deserialize)]
struct CommitRefJson {
    sha: String,
}

#[derive(Deserialize)]
struct FilesPageJson {
    #[serde(rename = "pageInfo")]
    page_info: PageInfoJson,
    nodes: Vec<FileNodeJson>,
}

#[derive(Deserialize)]
struct PageInfoJson {
    #[serde(rename = "hasNextPage")]
    has_next_page: bool,
    #[serde(rename = "endCursor")]
    end_cursor: String,
}

#[derive(Deserialize)]
struct FileNodeJson {
    path: String,
    #[serde(rename = "viewerViewedState")]
    viewer_viewed_state: String,
}

fn pull_request_variables(pr: &PullRequestUrl, after: &serde_json::Value) -> serde_json::Value {
    let repo = pr.repo.to_string();
    let (owner, name) = repo.split_once('/').expect("a repo name is owner/name");
    serde_json::json!({"owner": owner, "name": name, "number": pr.number, "after": after})
}

fn capitalized(word: &str) -> String {
    let mut chars = word.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().collect::<String>() + chars.as_str())
        .unwrap_or_default()
}

impl GitHub {
    pub fn from_env() -> Self {
        Self {
            api: std::env::var("GUIDED_REVIEW_GITHUB_API")
                .unwrap_or_else(|_| "https://api.github.com".to_string()),
            token: token_from_gh_cli().or_else(|| std::env::var("GITHUB_TOKEN").ok()),
            pull_request_ids: Arc::default(),
        }
    }

    /// For a GitHub-like server at another address. GraphQL needs a token, so it gets a placeholder.
    pub fn at(api: &str) -> Self {
        Self {
            api: api.to_string(),
            token: Some("placeholder".to_string()),
            pull_request_ids: Arc::default(),
        }
    }

    pub fn pull_request_tips(&self, pr: &PullRequestUrl) -> anyhow::Result<PullRequestTips> {
        let pull: PullJson = self
            .get(
                &format!("repos/{}/pulls/{}", pr.repo, pr.number),
                "application/vnd.github+json",
            )
            .and_then(|mut response| Ok(response.body_mut().read_json()?))
            .with_context(|| format!("could not read {pr} from GitHub"))?;
        Ok(PullRequestTips {
            base_branch_tip: Sha::new(&pull.base.sha),
            head: Sha::new(&pull.head.sha),
        })
    }

    pub fn viewed_files(&self, pr: &PullRequestUrl) -> anyhow::Result<Vec<ViewedFile>> {
        let mut files = Vec::new();
        let mut after = serde_json::Value::Null;
        loop {
            let data = self.graphql(
                "query($owner: String!, $name: String!, $number: Int!, $after: String) {
                    repository(owner: $owner, name: $name) { pullRequest(number: $number) {
                        id
                        files(first: 100, after: $after) {
                            pageInfo { hasNextPage endCursor }
                            nodes { path viewerViewedState }
                        }
                    } }
                }",
                pull_request_variables(pr, &after),
            )?;
            let pull = &data["repository"]["pullRequest"];
            if let Some(id) = pull["id"].as_str() {
                self.remember_pull_request_id(pr, id);
            }
            let page: FilesPageJson = serde_json::from_value(pull["files"].clone())
                .with_context(|| format!("could not read the files of {pr} from GitHub"))?;
            files.extend(page.nodes.into_iter().map(|file| ViewedFile {
                path: file.path,
                state: match file.viewer_viewed_state.as_str() {
                    "VIEWED" => ViewedState::Viewed,
                    "DISMISSED" => ViewedState::Dismissed,
                    _ => ViewedState::Unviewed,
                },
            }));
            if !page.page_info.has_next_page {
                return Ok(files);
            }
            after = page.page_info.end_cursor.into();
        }
    }

    pub fn set_file_viewed(
        &self,
        pr: &PullRequestUrl,
        path: &str,
        viewed: bool,
    ) -> anyhow::Result<()> {
        let mutation = if viewed {
            "markFileAsViewed"
        } else {
            "unmarkFileAsViewed"
        };
        let pull_request_id = self.pull_request_id(pr)?;
        self.graphql(
            &format!(
                "mutation($input: {}Input!) {{ {mutation}(input: $input) {{ clientMutationId }} }}",
                capitalized(mutation)
            ),
            serde_json::json!({"input": {"pullRequestId": pull_request_id, "path": path}}),
        )
        .with_context(|| format!("could not change the viewed state of {path} in {pr}"))?;
        Ok(())
    }

    fn pull_request_id(&self, pr: &PullRequestUrl) -> anyhow::Result<String> {
        if let Some(id) = self.pull_request_ids.lock().unwrap().get(&pr.to_string()) {
            return Ok(id.clone());
        }
        let data = self.graphql(
            "query($owner: String!, $name: String!, $number: Int!) {
                repository(owner: $owner, name: $name) { pullRequest(number: $number) { id } }
            }",
            pull_request_variables(pr, &serde_json::Value::Null),
        )?;
        let id = data["repository"]["pullRequest"]["id"]
            .as_str()
            .with_context(|| format!("GitHub did not know {pr}"))?
            .to_string();
        self.remember_pull_request_id(pr, &id);
        Ok(id)
    }

    fn remember_pull_request_id(&self, pr: &PullRequestUrl, id: &str) {
        self.pull_request_ids
            .lock()
            .unwrap()
            .insert(pr.to_string(), id.to_string());
    }

    /// Returns the `data` of the answer. A missing or refused token is `ViewedSyncUnavailable`.
    fn graphql(
        &self,
        query: &str,
        variables: serde_json::Value,
    ) -> anyhow::Result<serde_json::Value> {
        let token = self.token.as_ref().ok_or_else(|| {
            ViewedSyncUnavailable("no GitHub token: run `gh auth login` or set GITHUB_TOKEN".into())
        })?;
        let response = ureq::post(format!("{}/graphql", self.api))
            .header("Authorization", format!("Bearer {token}"))
            .header("User-Agent", "guided-review")
            .send_json(serde_json::json!({"query": query, "variables": variables}));
        let mut answer: serde_json::Value = match response {
            Ok(mut response) => response.body_mut().read_json()?,
            Err(ureq::Error::StatusCode(401 | 403)) => {
                return Err(ViewedSyncUnavailable(
                    "GitHub refused the token: it needs the `repo` scope".into(),
                )
                .into());
            }
            Err(error) => return Err(error.into()),
        };
        if let Some(errors) = answer.get("errors") {
            let refused = errors.as_array().is_some_and(|errors| {
                errors.iter().any(|error| {
                    matches!(
                        error["type"].as_str(),
                        Some("FORBIDDEN" | "INSUFFICIENT_SCOPES")
                    )
                })
            });
            if refused {
                return Err(ViewedSyncUnavailable(
                    "GitHub refused the token: it needs the `repo` scope".into(),
                )
                .into());
            }
            anyhow::bail!("GitHub answered with errors: {errors}");
        }
        Ok(answer["data"].take())
    }

    /// Every review thread of the PR, over GraphQL (REST shows no resolved or outdated state).
    pub fn review_threads(&self, pr: &PullRequestUrl) -> anyhow::Result<Vec<ReviewThread>> {
        let mut threads = Vec::new();
        let mut after = serde_json::Value::Null;
        loop {
            let data = self
                .graphql(REVIEW_THREADS_QUERY, pull_request_variables(pr, &after))
                .with_context(|| {
                    format!("could not read the review threads of {pr} from GitHub")
                })?;
            let page = parse_review_threads(&serde_json::json!({"data": data}))?;
            threads.extend(page.threads);
            match page.next_cursor {
                Some(next) => after = next.into(),
                None => return Ok(threads),
            }
        }
    }

    fn get(&self, path: &str, accept: &str) -> anyhow::Result<ureq::http::Response<ureq::Body>> {
        let mut request = ureq::get(format!("{}/{path}", self.api))
            .header("Accept", accept)
            .header("User-Agent", "guided-review");
        if let Some(token) = &self.token {
            request = request.header("Authorization", format!("Bearer {token}"));
        }
        Ok(request.call()?)
    }

    pub fn code_source(&self, repo: &RepoName) -> GitHubCodeSource {
        GitHubCodeSource {
            github: self.clone(),
            repo: repo.clone(),
        }
    }
}

/// Reads one repo through the GitHub API, for PRs without a local clone.
pub struct GitHubCodeSource {
    github: GitHub,
    repo: RepoName,
}

#[derive(Deserialize)]
struct CompareJson {
    merge_base_commit: CommitRefJson,
}

impl GitHubCodeSource {
    fn compare_path(&self, base: &Sha, head: &Sha) -> String {
        format!("repos/{}/compare/{base}...{head}", self.repo)
    }
}

impl CodeSource for GitHubCodeSource {
    fn file_at(&self, sha: &Sha, path: &str) -> anyhow::Result<String> {
        Ok(String::from_utf8_lossy(&self.blob_at(sha, path)?).into_owned())
    }

    fn blob_at(&self, sha: &Sha, path: &str) -> anyhow::Result<Vec<u8>> {
        let url = format!(
            "repos/{}/contents/{}?ref={sha}",
            self.repo,
            percent_encode_path(path)
        );
        let mut response = self
            .github
            .get(&url, "application/vnd.github.raw+json")
            .with_context(|| format!("could not read {path} at {sha} from GitHub"))?;
        Ok(response.body_mut().read_to_vec()?)
    }

    fn diff(&self, base: &Sha, head: &Sha) -> anyhow::Result<Vec<FileDiff>> {
        let patch = self
            .github
            .get(
                &self.compare_path(base, head),
                "application/vnd.github.diff",
            )
            .and_then(|mut response| Ok(response.body_mut().read_to_string()?))
            .map_err(|error| match error.downcast_ref::<ureq::Error>() {
                Some(ureq::Error::StatusCode(406 | 422)) => anyhow::anyhow!(
                    "GitHub refuses to send the diff {base}...{head} because it is too large; \
                     use a local clone of {} (run from inside it, or list it under [clones] in the config)",
                    self.repo
                ),
                _ => error.context(format!("could not read the diff {base}...{head} from GitHub")),
            })?;
        Ok(parse_unified_diff(&patch))
    }

    fn merge_base(&self, a: &Sha, b: &Sha) -> anyhow::Result<Sha> {
        let compare: CompareJson = self
            .github
            .get(&self.compare_path(a, b), "application/vnd.github+json")
            .and_then(|mut response| Ok(response.body_mut().read_json()?))
            .with_context(|| format!("could not find the merge base of {a} and {b} on GitHub"))?;
        Ok(Sha::new(&compare.merge_base_commit.sha))
    }

    fn working_tree_is_at(&self, _head: &Sha) -> bool {
        false
    }
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

fn token_from_gh_cli() -> Option<String> {
    let output = Command::new("gh").args(["auth", "token"]).output().ok()?;
    let token = String::from_utf8(output.stdout).ok()?.trim().to_string();
    (output.status.success() && !token.is_empty()).then_some(token)
}

const REVIEW_THREADS_QUERY: &str = "query($owner: String!, $name: String!, $number: Int!, $after: String) {
  repository(owner: $owner, name: $name) { pullRequest(number: $number) {
    reviewThreads(first: 100, after: $after) {
      pageInfo { hasNextPage endCursor }
      nodes { id isResolved isOutdated path line startLine originalLine originalStartLine diffSide subjectType
        comments(first: 50) { nodes { url body originalCommit { oid } author { login __typename } } } }
    }
  } }
}";

pub(crate) struct ReviewThreadsPage {
    pub threads: Vec<ReviewThread>,
    pub next_cursor: Option<String>,
}

pub(crate) fn parse_review_threads(
    answer: &serde_json::Value,
) -> anyhow::Result<ReviewThreadsPage> {
    #[derive(Deserialize)]
    struct Answer {
        data: Option<Data>,
    }
    #[derive(Deserialize)]
    struct Data {
        repository: Repository,
    }
    #[derive(Deserialize)]
    struct Repository {
        #[serde(rename = "pullRequest")]
        pull_request: PullRequest,
    }
    #[derive(Deserialize)]
    struct PullRequest {
        #[serde(rename = "reviewThreads")]
        review_threads: Threads,
    }
    #[derive(Deserialize)]
    struct Threads {
        #[serde(rename = "pageInfo")]
        page_info: PageInfo,
        nodes: Vec<ThreadJson>,
    }
    #[derive(Deserialize)]
    struct PageInfo {
        #[serde(rename = "hasNextPage")]
        has_next_page: bool,
        #[serde(rename = "endCursor")]
        end_cursor: Option<String>,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct ThreadJson {
        id: String,
        is_resolved: bool,
        is_outdated: bool,
        path: String,
        line: Option<usize>,
        start_line: Option<usize>,
        original_line: Option<usize>,
        original_start_line: Option<usize>,
        diff_side: String,
        subject_type: String,
        comments: Comments,
    }
    #[derive(Deserialize)]
    struct Comments {
        nodes: Vec<CommentJson>,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct CommentJson {
        url: String,
        body: String,
        original_commit: Option<CommitJson>,
        /// GitHub answers `null` for a deleted account.
        author: Option<AuthorJson>,
    }
    #[derive(Deserialize)]
    struct CommitJson {
        oid: String,
    }
    #[derive(Deserialize)]
    struct AuthorJson {
        login: String,
        #[serde(rename = "__typename")]
        kind: String,
    }

    let answer: Answer = serde_json::from_value(answer.clone())?;
    let threads = answer
        .data
        .context("GitHub answered the review threads query with an error")?
        .repository
        .pull_request
        .review_threads;
    Ok(ReviewThreadsPage {
        next_cursor: threads
            .page_info
            .has_next_page
            .then_some(threads.page_info.end_cursor)
            .flatten(),
        threads: threads
            .nodes
            .into_iter()
            .map(|thread| ReviewThread {
                id: thread.id,
                is_resolved: thread.is_resolved,
                is_outdated: thread.is_outdated,
                path: thread.path,
                line: thread.line,
                start_line: thread.start_line,
                side: if thread.diff_side == "LEFT" {
                    Side::Base
                } else {
                    Side::Head
                },
                is_file_comment: thread.subject_type == "FILE",
                original_line: thread.original_line,
                original_start_line: thread.original_start_line,
                original_commit: thread
                    .comments
                    .nodes
                    .iter()
                    .find_map(|comment| comment.original_commit.as_ref())
                    .map(|commit| commit.oid.clone()),
                comments: thread
                    .comments
                    .nodes
                    .into_iter()
                    .map(|comment| ThreadComment {
                        is_bot: comment
                            .author
                            .as_ref()
                            .is_some_and(|author| author.kind == "Bot"),
                        author: comment
                            .author
                            .map_or_else(|| "ghost".to_string(), |author| author.login),
                        body: comment.body,
                        url: comment.url,
                    })
                    .collect(),
            })
            .collect(),
    })
}
