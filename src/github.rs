use std::collections::HashMap;
use std::process::Command;
use std::sync::{Arc, Mutex};

use anyhow::Context;
use serde::Deserialize;

use crate::code_source::{BlobInfo, CodeSource, FileDiff, parse_unified_diff};
use crate::pull_request::{PullRequestTips, PullRequestUrl, RepoName, Sha};
use crate::viewer::{
    CommentPlace, GitHubPort, PendingReviewExists, Reaction, ReactionCount, ReviewThread,
    ReviewVerdict, SendMode, Side, ThreadComment, ThreadNotCreated, ViewedFile, ViewedState,
    ViewedSyncUnavailable, percent_encode_path,
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

    /// Returns the id of the pending review the thread went into.
    pub fn add_thread_to_pending_review(
        &self,
        pr: &PullRequestUrl,
        place: &CommentPlace,
        body: &str,
    ) -> anyhow::Result<String> {
        let mut input = serde_json::json!({
            "pullRequestId": self.pull_request_id(pr)?,
            "path": place.path(),
            "body": body,
        });
        if place.is_file_comment() {
            input["subjectType"] = "FILE".into();
        } else {
            input["line"] = place.end.line.into();
            input["side"] = graphql_side(place.end.side).into();
            input["subjectType"] = "LINE".into();
            if !place.is_single_line() {
                input["startLine"] = place.start.line.into();
                input["startSide"] = graphql_side(place.start.side).into();
            }
        }
        let data = self
            .graphql(
                "mutation($input: AddPullRequestReviewThreadInput!) {
                    addPullRequestReviewThread(input: $input) {
                        thread { id comments(first: 1) { nodes { pullRequestReview { id } } } }
                    }
                }",
                serde_json::json!({"input": input}),
            )
            .map_err(one_pending_review_error)?;
        let thread = &data["addPullRequestReviewThread"]["thread"];
        if thread.is_null() {
            return Err(ThreadNotCreated(format!(
                "GitHub took no comment on {} line {}",
                place.path(),
                place.end.line
            ))
            .into());
        }
        Ok(thread["comments"]["nodes"][0]["pullRequestReview"]["id"]
            .as_str()
            .unwrap_or_default()
            .to_string())
    }

    fn pending_review_id(&self, pr: &PullRequestUrl) -> anyhow::Result<Option<String>> {
        let data = self
            .graphql(
                "query($owner: String!, $name: String!, $number: Int!) {
                    repository(owner: $owner, name: $name) { pullRequest(number: $number) {
                        reviews(states: PENDING, first: 10) { nodes { id viewerDidAuthor } }
                    } }
                }",
                pull_request_variables(pr, &serde_json::Value::Null),
            )
            .with_context(|| format!("could not look for a pending review on {pr}"))?;
        Ok(data["repository"]["pullRequest"]["reviews"]["nodes"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|review| review["viewerDidAuthor"].as_bool() == Some(true))
            .and_then(|review| review["id"].as_str())
            .map(str::to_string))
    }

    fn submit_pending_review(
        &self,
        review_id: &str,
        verdict: ReviewVerdict,
        body: &str,
    ) -> anyhow::Result<()> {
        self.graphql(
            "mutation($input: SubmitPullRequestReviewInput!) {
                submitPullRequestReview(input: $input) { clientMutationId }
            }",
            serde_json::json!({"input": {
                "pullRequestReviewId": review_id,
                "event": verdict.github_name(),
                "body": body,
            }}),
        )?;
        Ok(())
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

impl GitHubPort for GitHub {
    fn add_review_thread(
        &self,
        pr: &PullRequestUrl,
        place: &CommentPlace,
        body: &str,
    ) -> anyhow::Result<()> {
        self.add_thread_to_pending_review(pr, place, body)
            .map(|_| ())
    }

    fn pull_request_tips(&self, pr: &PullRequestUrl) -> anyhow::Result<PullRequestTips> {
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

    fn viewed_files(&self, pr: &PullRequestUrl) -> anyhow::Result<Vec<ViewedFile>> {
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

    fn set_file_viewed(&self, pr: &PullRequestUrl, path: &str, viewed: bool) -> anyhow::Result<()> {
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

    /// Every review thread of the PR, over GraphQL (REST shows no resolved or outdated state).
    fn review_threads(&self, pr: &PullRequestUrl) -> anyhow::Result<Vec<ReviewThread>> {
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

    fn has_pending_review(&self, pr: &PullRequestUrl) -> anyhow::Result<bool> {
        let data = self
            .graphql(
                "query($owner: String!, $name: String!, $number: Int!) {
                    repository(owner: $owner, name: $name) { pullRequest(number: $number) {
                        reviews(states: PENDING, first: 10) { nodes { viewerDidAuthor } }
                    } }
                }",
                pull_request_variables(pr, &serde_json::Value::Null),
            )
            .with_context(|| format!("could not look for a pending review on {pr}"))?;
        let reviews = data["repository"]["pullRequest"]["reviews"]["nodes"]
            .as_array()
            .with_context(|| format!("could not read the reviews of {pr} from GitHub"))?;
        Ok(reviews
            .iter()
            .any(|review| review["viewerDidAuthor"].as_bool() == Some(true)))
    }

    /// A thread in a new pending review, posted right away. GitHub posts nothing past an existing
    /// pending review, so the caller checks for one first.
    fn comment_now(
        &self,
        pr: &PullRequestUrl,
        place: &CommentPlace,
        body: &str,
    ) -> anyhow::Result<()> {
        let review_id = self.add_thread_to_pending_review(pr, place, body)?;
        let posted = self.graphql(
            "mutation($input: SubmitPullRequestReviewInput!) {
                submitPullRequestReview(input: $input) { clientMutationId }
            }",
            serde_json::json!({"input": {"pullRequestReviewId": review_id, "event": "COMMENT"}}),
        );
        if let Err(error) = posted {
            let _ = self.graphql(
                "mutation($input: DeletePullRequestReviewInput!) {
                    deletePullRequestReview(input: $input) { clientMutationId }
                }",
                serde_json::json!({"input": {"pullRequestReviewId": review_id}}),
            );
            return Err(error.context("could not post the comment"));
        }
        Ok(())
    }

    fn viewer_authored(&self, pr: &PullRequestUrl) -> anyhow::Result<bool> {
        let data = self
            .graphql(
                "query($owner: String!, $name: String!, $number: Int!) {
                    repository(owner: $owner, name: $name) { pullRequest(number: $number) { viewerDidAuthor } }
                }",
                pull_request_variables(pr, &serde_json::Value::Null),
            )
            .with_context(|| format!("could not tell who wrote {pr}"))?;
        data["repository"]["pullRequest"]["viewerDidAuthor"]
            .as_bool()
            .with_context(|| format!("GitHub did not say who wrote {pr}"))
    }

    /// A reply joins the viewer's pending review. With `CommentNow` the review it ended up in is sent at once.
    /// GitHub starts a pending review when there is none; that case has not run against real GitHub.
    fn reply_to_thread(
        &self,
        pr: &PullRequestUrl,
        thread_id: &str,
        body: &str,
        mode: SendMode,
    ) -> anyhow::Result<()> {
        let mut input = serde_json::json!({"pullRequestReviewThreadId": thread_id, "body": body});
        if let Some(review_id) = self.pending_review_id(pr)? {
            input["pullRequestReviewId"] = review_id.into();
        }
        let data = self
            .graphql(
                "mutation($input: AddPullRequestReviewThreadReplyInput!) {
                    addPullRequestReviewThreadReply(input: $input) {
                        comment { pullRequestReview { id state } }
                    }
                }",
                serde_json::json!({"input": input}),
            )
            .map_err(one_pending_review_error)
            .context("could not reply")?;
        let review = &data["addPullRequestReviewThreadReply"]["comment"]["pullRequestReview"];
        if mode == SendMode::CommentNow && review["state"] == "PENDING" {
            let review_id = review["id"].as_str().unwrap_or_default();
            self.submit_pending_review(review_id, ReviewVerdict::Comment, "")
                .context("could not post the reply")?;
        }
        Ok(())
    }

    fn set_thread_resolved(&self, thread_id: &str, resolved: bool) -> anyhow::Result<()> {
        let mutation = if resolved {
            "resolveReviewThread"
        } else {
            "unresolveReviewThread"
        };
        self.graphql(
            &format!(
                "mutation($input: {}Input!) {{ {mutation}(input: $input) {{ clientMutationId }} }}",
                capitalized(mutation)
            ),
            serde_json::json!({"input": {"threadId": thread_id}}),
        )
        .with_context(|| format!("could not change the thread {thread_id}"))?;
        Ok(())
    }

    fn set_reaction(&self, comment_id: &str, reaction: Reaction, on: bool) -> anyhow::Result<()> {
        let mutation = if on { "addReaction" } else { "removeReaction" };
        self.graphql(
            &format!(
                "mutation($input: {}Input!) {{ {mutation}(input: $input) {{ clientMutationId }} }}",
                capitalized(mutation)
            ),
            serde_json::json!({"input": {"subjectId": comment_id, "content": reaction.github_name()}}),
        )
        .context("could not change the reaction")?;
        Ok(())
    }

    /// Sends the viewer's pending review, or a new review with just the verdict and body when there is none.
    fn submit_review(
        &self,
        pr: &PullRequestUrl,
        verdict: ReviewVerdict,
        body: &str,
    ) -> anyhow::Result<()> {
        if let Some(review_id) = self.pending_review_id(pr)? {
            return self
                .submit_pending_review(&review_id, verdict, body)
                .context("could not send the review");
        }
        self.graphql(
            "mutation($input: AddPullRequestReviewInput!) {
                addPullRequestReview(input: $input) { clientMutationId }
            }",
            serde_json::json!({"input": {
                "pullRequestId": self.pull_request_id(pr)?,
                "event": verdict.github_name(),
                "body": body,
            }}),
        )
        .context("could not send the review")?;
        Ok(())
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

    fn blob_info_at(&self, sha: &Sha, path: &str) -> anyhow::Result<BlobInfo> {
        #[derive(Deserialize)]
        struct ContentsJson {
            sha: String,
            size: u64,
        }
        let url = format!(
            "repos/{}/contents/{}?ref={sha}",
            self.repo,
            percent_encode_path(path)
        );
        let contents: ContentsJson = self
            .github
            .get(&url, "application/vnd.github.object+json")
            .and_then(|mut response| Ok(response.body_mut().read_json()?))
            .with_context(|| format!("could not look up {path} at {sha} on GitHub"))?;
        Ok(BlobInfo {
            id: contents.sha,
            size: contents.size,
        })
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

fn graphql_side(side: Side) -> &'static str {
    match side {
        Side::Base => "LEFT",
        Side::Head => "RIGHT",
    }
}

fn one_pending_review_error(error: anyhow::Error) -> anyhow::Error {
    if format!("{error:#}").contains("one pending review") {
        PendingReviewExists.into()
    } else {
        error
    }
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
        comments(first: 50) { nodes { id url body reactionGroups { content viewerHasReacted reactors { totalCount } } originalCommit { oid } author { login __typename } } } }
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
        id: String,
        url: String,
        #[serde(default)]
        reaction_groups: Vec<ReactionGroupJson>,
        body: String,
        original_commit: Option<CommitJson>,
        /// GitHub answers `null` for a deleted account.
        author: Option<AuthorJson>,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct ReactionGroupJson {
        content: String,
        viewer_has_reacted: bool,
        reactors: ReactorsJson,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct ReactorsJson {
        total_count: usize,
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
                        id: comment.id,
                        body: comment.body,
                        url: comment.url,
                        reactions: comment
                            .reaction_groups
                            .into_iter()
                            .filter(|group| group.reactors.total_count > 0)
                            .filter_map(|group| {
                                Some(ReactionCount {
                                    reaction: Reaction::from_github_name(&group.content)?,
                                    count: group.reactors.total_count,
                                    viewer_has_reacted: group.viewer_has_reacted,
                                })
                            })
                            .collect(),
                    })
                    .collect(),
            })
            .collect(),
    })
}
