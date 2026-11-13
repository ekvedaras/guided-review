use std::collections::HashMap;
use std::path::Path;
use std::process::Command;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::code_source::{BlobInfo, CodeSource, FileDiff, parse_unified_diff};
use crate::file_path::FilePath;
use crate::image_facts;
use crate::json::at;
use crate::line_number::LineNumber;
use crate::pull_request::{PullRequestTips, PullRequestUrl, RepoName, Sha};
use crate::update::{PublishedRelease, ReleaseAsset, ReleasesPort};
use crate::viewer::{
    AvatarPort, CommentId, CommentPlace, GitHubError, NoAccess, OutageCause, PictureUploadPort,
    PullRequestDescriptionPort, PullRequestReactionsPort, PullRequestTipsPort, Reaction,
    ReactionChange, ReactionCount, Resolution, ReviewOutcome, ReviewThread, ReviewThreadsPort,
    ReviewVerdict, ReviewWritesPort, SendMode, Side, SubmittedReview, ThreadComment, ThreadId,
    Viewed, ViewedFile, ViewedFilesPort, ViewedState, percent_encode_path,
};

#[derive(Clone)]
pub struct GitHub {
    api: String,
    /// Where pictures go up, a server of its own on github.com.
    uploads: String,
    token: Option<String>,
    /// GraphQL mutations address a PR by node id, so it is looked up once per PR.
    pull_request_ids: Arc<Mutex<HashMap<String, String>>>,
    /// The viewer's teams change rarely, so they are read once per app run.
    viewer_teams: Arc<Mutex<Option<Vec<String>>>>,
}

mod relevant_prs;

impl From<anyhow::Error> for GitHubError {
    /// Every failure of a call to GitHub passes through here, so an outage is told from the rest
    /// in one place, by what the HTTP client reported and not by its text.
    fn from(error: anyhow::Error) -> Self {
        match error.downcast_ref::<ureq::Error>().and_then(outage_cause) {
            Some(cause) => Self::NotAnswering(cause),
            None => Self::Other(error),
        }
    }
}

fn outage_cause(error: &ureq::Error) -> Option<OutageCause> {
    match error {
        ureq::Error::StatusCode(status @ 500..=599) => Some(OutageCause::ServerError(*status)),
        ureq::Error::Timeout(_) => Some(OutageCause::TimedOut),
        ureq::Error::Io(io) if io.kind() == std::io::ErrorKind::TimedOut => {
            Some(OutageCause::TimedOut)
        }
        ureq::Error::Io(_) | ureq::Error::HostNotFound | ureq::Error::ConnectionFailed => {
            Some(OutageCause::NoConnection)
        }
        _ => None,
    }
}

#[derive(Deserialize)]
struct PullJson {
    base: CommitRefJson,
    head: CommitRefJson,
}

#[derive(Deserialize)]
struct PullBodyJson {
    body: Option<String>,
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
    path: FilePath,
    #[serde(rename = "viewerViewedState")]
    viewer_viewed_state: String,
}

fn pull_request_variables(pr: &PullRequestUrl, after: &serde_json::Value) -> serde_json::Value {
    let (owner, name) = (pr.repo.owner(), pr.repo.name());
    serde_json::json!({"owner": owner, "name": name, "number": pr.number, "after": after})
}

fn capitalized(word: &str) -> String {
    let mut chars = word.chars();
    chars
        .next()
        .map_or_default(|first| first.to_uppercase().collect::<String>() + chars.as_str())
}

impl GitHub {
    pub fn from_env() -> Self {
        let token = token_from_gh_cli().or_else(|| std::env::var("GITHUB_TOKEN").ok());
        match std::env::var("GUIDED_REVIEW_GITHUB_API") {
            Ok(api) => Self {
                token,
                ..Self::without_token_at(&api)
            },
            Err(_) => Self {
                api: "https://api.github.com".to_owned(),
                uploads: "https://uploads.github.com".to_owned(),
                token,
                pull_request_ids: Arc::default(),
                viewer_teams: Arc::default(),
            },
        }
    }

    /// For a GitHub-like server at another address. GraphQL needs a token, so it gets a placeholder.
    pub fn at(api: &str) -> Self {
        Self {
            token: Some("placeholder".to_owned()),
            ..Self::without_token_at(api)
        }
    }

    pub fn without_token_at(api: &str) -> Self {
        Self {
            api: api.to_owned(),
            uploads: api.to_owned(),
            token: None,
            pull_request_ids: Arc::default(),
            viewer_teams: Arc::default(),
        }
    }

    /// An id is only ever inserted whole, so a poisoned lock is still safe to use.
    fn known_ids(&self) -> MutexGuard<'_, HashMap<String, String>> {
        self.pull_request_ids
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// `subject_id` is the GitHub node id of what is reacted to: a comment or a PR.
    fn change_reaction(
        &self,
        subject_id: &str,
        reaction: Reaction,
        change: ReactionChange,
    ) -> Result<(), GitHubError> {
        let mutation = match change {
            ReactionChange::Add => "addReaction",
            ReactionChange::Remove => "removeReaction",
        };
        self.graphql(
            &format!(
                "mutation($input: {}Input!) {{ {mutation}(input: $input) {{ clientMutationId }} }}",
                capitalized(mutation)
            ),
            &serde_json::json!({"input": {"subjectId": subject_id, "content": reaction.github_name()}}),
        )?;
        Ok(())
    }

    fn pull_request_id(&self, pr: &PullRequestUrl) -> Result<String, GitHubError> {
        if let Some(id) = self.known_ids().get(&pr.to_string()) {
            return Ok(id.clone());
        }
        let data = self.graphql(
            "query($owner: String!, $name: String!, $number: Int!) {
                repository(owner: $owner, name: $name) { pullRequest(number: $number) { id } }
            }",
            &pull_request_variables(pr, &serde_json::Value::Null),
        )?;
        let id = at(&data, "/repository/pullRequest/id")
            .as_str()
            .with_context(|| format!("GitHub did not know {pr}"))?
            .to_owned();
        self.remember_pull_request_id(pr, &id);
        Ok(id)
    }

    fn remember_pull_request_id(&self, pr: &PullRequestUrl, id: &str) {
        self.known_ids().insert(pr.to_string(), id.to_owned());
    }

    /// Returns the `data` of the answer. Every GitHub failure a caller tells apart is named here.
    fn graphql(
        &self,
        query: &str,
        variables: &serde_json::Value,
    ) -> Result<serde_json::Value, GitHubError> {
        let token = self.token.as_ref().ok_or_else(|| {
            GitHubError::NoAccess(NoAccess::new(
                "no GitHub token: run `gh auth login` or set GITHUB_TOKEN",
            ))
        })?;
        let response = ureq::post(format!("{}/graphql", self.api))
            .header("Authorization", format!("Bearer {token}"))
            .header("User-Agent", "guided-review")
            .send_json(serde_json::json!({"query": query, "variables": variables}));
        let mut answer: serde_json::Value = match response {
            Ok(mut response) => response
                .body_mut()
                .read_json()
                .map_err(anyhow::Error::from)?,
            Err(ureq::Error::StatusCode(401 | 403)) => {
                return Err(GitHubError::NoAccess(NoAccess::new(
                    "GitHub refused the token: it needs the `repo` scope",
                )));
            }
            Err(error) => return Err(anyhow::Error::from(error).into()),
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
                return Err(GitHubError::NoAccess(NoAccess::new(
                    "GitHub refused the token: it needs the `repo` scope",
                )));
            }
            return Err(anyhow::anyhow!("GitHub answered with errors: {errors}").into());
        }
        Ok(answer
            .get_mut("data")
            .map_or_default(serde_json::Value::take))
    }

    /// Returns the id of the pending review the thread went into, when GitHub says it.
    pub fn add_thread_to_pending_review(
        &self,
        pr: &PullRequestUrl,
        place: &CommentPlace,
        body: &str,
    ) -> Result<Option<String>, GitHubError> {
        let pull_request_id = self.pull_request_id(pr)?;
        let input = match place {
            CommentPlace::File(path) => ReviewThreadInput {
                subject_type: "FILE",
                ..ReviewThreadInput::new(&pull_request_id, path.as_str(), body)
            },
            CommentPlace::Lines { start, end } => {
                let is_range = start != end;
                ReviewThreadInput {
                    subject_type: "LINE",
                    line: Some(end.line),
                    side: Some(graphql_side(end.side)),
                    start_line: is_range.then_some(start.line),
                    start_side: is_range.then(|| graphql_side(start.side)),
                    ..ReviewThreadInput::new(&pull_request_id, end.path.as_str(), body)
                }
            }
        };
        let data = self
            .graphql(
                "mutation($input: AddPullRequestReviewThreadInput!) {
                addPullRequestReviewThread(input: $input) {
                    thread { id comments(first: 1) { nodes { pullRequestReview { id } } } }
                }
            }",
                &serde_json::json!({"input": input}),
            )
            .map_err(one_pending_review)?;
        let thread = at(&data, "/addPullRequestReviewThread/thread");
        if thread.is_null() {
            let line = match place {
                CommentPlace::File(_) => String::new(),
                CommentPlace::Lines { end, .. } => format!(" line {}", end.line),
            };
            return Err(GitHubError::ThreadNotCreated(format!(
                "GitHub took no comment on {}{line}",
                place.path()
            )));
        }
        Ok(non_empty_id(at(
            thread,
            "/comments/nodes/0/pullRequestReview/id",
        )))
    }

    /// The review a new comment went into. When GitHub did not say which, it is the viewer's
    /// pending one, as GitHub allows only one. When there is none either, the comment may already
    /// be posted, so the reviewer looks before sending it again.
    fn review_holding_new_comment(
        &self,
        pr: &PullRequestUrl,
        reported: Option<String>,
    ) -> Result<String, GitHubError> {
        if let Some(review_id) = reported {
            return Ok(review_id);
        }
        Ok(self.pending_review_id(pr)?.context(
            "GitHub took the comment but did not say into which review; look for it on GitHub before sending it again",
        )?)
    }

    fn pending_review_id(&self, pr: &PullRequestUrl) -> Result<Option<String>, GitHubError> {
        let data = self
            .graphql(
                "query($owner: String!, $name: String!, $number: Int!) {
                    repository(owner: $owner, name: $name) { pullRequest(number: $number) {
                        reviews(states: PENDING, first: 10) { nodes { id viewerDidAuthor } }
                    } }
                }",
                &pull_request_variables(pr, &serde_json::Value::Null),
            )
            .map_err(|error| {
                error.context(format!("could not look for a pending review on {pr}"))
            })?;
        Ok(at(&data, "/repository/pullRequest/reviews/nodes")
            .as_array()
            .into_iter()
            .flatten()
            .find(|review| at(review, "/viewerDidAuthor").as_bool() == Some(true))
            .and_then(|review| non_empty_id(at(review, "/id"))))
    }

    fn submit_pending_review(
        &self,
        review_id: &str,
        verdict: ReviewVerdict,
        body: &str,
    ) -> Result<(), GitHubError> {
        self.graphql(
            "mutation($input: SubmitPullRequestReviewInput!) {
                submitPullRequestReview(input: $input) { clientMutationId }
            }",
            &serde_json::json!({"input": {
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
            fallback_reason: None,
        }
    }
}

/// A release archive is about 15 MB; the cap only stops a runaway download.
const RELEASE_FILE_LIMIT: u64 = 500 * 1024 * 1024;

#[derive(Deserialize)]
struct ReleaseJson {
    tag_name: String,
    assets: Vec<ReleaseAssetJson>,
}

#[derive(Deserialize)]
struct ReleaseAssetJson {
    name: String,
    browser_download_url: String,
}

impl ReleasesPort for GitHub {
    fn latest_release(&self) -> anyhow::Result<PublishedRelease> {
        let repository = env!("CARGO_PKG_REPOSITORY").trim_start_matches("https://github.com/");
        let release: ReleaseJson = self
            .get(
                &format!("repos/{repository}/releases/latest"),
                "application/vnd.github+json",
            )
            .and_then(|mut response| Ok(response.body_mut().read_json()?))
            .with_context(|| format!("could not read the latest release of {repository}"))?;
        Ok(PublishedRelease {
            tag: release.tag_name,
            assets: release
                .assets
                .into_iter()
                .map(|asset| ReleaseAsset {
                    name: asset.name,
                    url: asset.browser_download_url,
                })
                .collect(),
        })
    }

    /// Release files are public and GitHub sends them from another host, so no token goes along.
    fn download(&self, url: &str) -> anyhow::Result<Vec<u8>> {
        let mut response = ureq::get(url)
            .header("User-Agent", "guided-review")
            .call()
            .with_context(|| format!("could not download {url}"))?;
        response
            .body_mut()
            .with_config()
            .limit(RELEASE_FILE_LIMIT)
            .read_to_vec()
            .with_context(|| format!("could not download {url}"))
    }
}

impl PullRequestTipsPort for GitHub {
    fn pull_request_tips(&self, pr: &PullRequestUrl) -> Result<PullRequestTips, GitHubError> {
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
}

impl PullRequestDescriptionPort for GitHub {
    fn pull_request_description(&self, pr: &PullRequestUrl) -> Result<String, GitHubError> {
        let pull: PullBodyJson = self
            .get(
                &format!("repos/{}/pulls/{}", pr.repo, pr.number),
                "application/vnd.github+json",
            )
            .and_then(|mut response| Ok(response.body_mut().read_json()?))
            .with_context(|| format!("could not read the description of {pr} from GitHub"))?;
        Ok(pull.body.unwrap_or_default())
    }

    fn set_pull_request_description(&self, pr: &PullRequestUrl, body: &str) -> anyhow::Result<()> {
        let id = self.pull_request_id(pr)?;
        self.graphql(
            "mutation($input: UpdatePullRequestInput!) {
                updatePullRequest(input: $input) { pullRequest { id } }
            }",
            &serde_json::json!({"input": {"pullRequestId": id, "body": body}}),
        )
        .with_context(|| format!("could not change the description of {pr} on GitHub"))?;
        Ok(())
    }
}

#[derive(Deserialize)]
struct RepositoryJson {
    id: u64,
}

#[derive(Deserialize)]
struct UploadedJson {
    url: String,
}

/// GitHub's own drag-and-drop store, so the picture is seen only by those who can see the repository.
/// It is not in GitHub's documented API, but it takes the token `gh` holds.
impl PullRequestReactionsPort for GitHub {
    fn set_pull_request_reaction(
        &self,
        pr: &PullRequestUrl,
        reaction: Reaction,
        change: ReactionChange,
    ) -> Result<(), GitHubError> {
        let id = self.pull_request_id(pr)?;
        self.change_reaction(&id, reaction, change)
            .map_err(|error| error.context(format!("could not change the reaction on {pr}")))
    }
}

impl PictureUploadPort for GitHub {
    fn upload_picture(&self, pr: &PullRequestUrl, picture: &Path) -> anyhow::Result<String> {
        let name = picture
            .file_name()
            .context("a picture needs a file name")?
            .to_string_lossy()
            .into_owned();
        let media_type = image_facts::media_type(&name)
            .with_context(|| format!("{name} is no picture GitHub takes"))?;
        let bytes = std::fs::read(picture).with_context(|| format!("could not read {name}"))?;
        let token = self
            .token
            .as_ref()
            .context("no GitHub token: run `gh auth login` or set GITHUB_TOKEN")?;
        let repository: RepositoryJson = self
            .get(&format!("repos/{}", pr.repo), "application/vnd.github+json")?
            .body_mut()
            .read_json()?;
        let uploaded: UploadedJson =
            ureq::post(format!("{}/user-attachments/assets", self.uploads))
                .query("name", &name)
                .query("content_type", media_type)
                .query("repository_id", repository.id.to_string())
                .header("Authorization", format!("Bearer {token}"))
                .header("Accept", "application/json")
                .header("Content-Type", media_type)
                .header("User-Agent", "guided-review")
                .send(bytes.as_slice())
                .map_err(|error| match error {
                    ureq::Error::StatusCode(404) => anyhow::anyhow!(
                        "GitHub refused the upload of {name}: the token needs push access to {}",
                        pr.repo
                    ),
                    error => anyhow::Error::from(error).context(format!("could not upload {name}")),
                })?
                .body_mut()
                .read_json()?;
        Ok(uploaded.url)
    }
}

impl ViewedFilesPort for GitHub {
    fn viewed_files(&self, pr: &PullRequestUrl) -> Result<Vec<ViewedFile>, GitHubError> {
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
                &pull_request_variables(pr, &after),
            )?;
            let pull = at(&data, "/repository/pullRequest");
            if let Some(id) = at(pull, "/id").as_str() {
                self.remember_pull_request_id(pr, id);
            }
            let page: FilesPageJson = serde_json::from_value(at(pull, "/files").clone())
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

    fn set_file_viewed(
        &self,
        pr: &PullRequestUrl,
        path: &FilePath,
        viewed: Viewed,
    ) -> Result<(), GitHubError> {
        let mutation = match viewed {
            Viewed::Yes => "markFileAsViewed",
            Viewed::No => "unmarkFileAsViewed",
        };
        let pull_request_id = self.pull_request_id(pr)?;
        self.graphql(
            &format!(
                "mutation($input: {}Input!) {{ {mutation}(input: $input) {{ clientMutationId }} }}",
                capitalized(mutation)
            ),
            &serde_json::json!({"input": {"pullRequestId": pull_request_id, "path": path}}),
        )
        .map_err(|error| {
            error.context(format!(
                "could not change the viewed state of {path} in {pr}"
            ))
        })?;
        Ok(())
    }
}

impl ReviewThreadsPort for GitHub {
    /// Every review thread of the PR, over GraphQL (REST shows no resolved or outdated state).
    fn review_threads(&self, pr: &PullRequestUrl) -> Result<Vec<ReviewThread>, GitHubError> {
        let mut threads = Vec::new();
        let mut after = serde_json::Value::Null;
        loop {
            let data = self
                .graphql(REVIEW_THREADS_QUERY, &pull_request_variables(pr, &after))
                .map_err(|error| {
                    error.context(format!(
                        "could not read the review threads of {pr} from GitHub"
                    ))
                })?;
            let page = parse_review_threads(&serde_json::json!({"data": data}))?;
            threads.extend(page.threads);
            match page.next_cursor {
                Some(next) => after = next.into(),
                None => return Ok(threads),
            }
        }
    }

    /// A reply joins the viewer's pending review. With `CommentNow` the review it ended up in is sent at once.
    /// GitHub starts a pending review when there is none; that case has not run against real GitHub.
    fn reply_to_thread(
        &self,
        pr: &PullRequestUrl,
        thread_id: &ThreadId,
        body: &str,
        mode: SendMode,
    ) -> Result<CommentId, GitHubError> {
        let input = ReplyInput {
            pull_request_review_thread_id: thread_id.as_str(),
            body,
            pull_request_review_id: self.pending_review_id(pr)?,
        };
        let data = self
            .graphql(
                "mutation($input: AddPullRequestReviewThreadReplyInput!) {
                    addPullRequestReviewThreadReply(input: $input) {
                        comment { id pullRequestReview { id state } }
                    }
                }",
                &serde_json::json!({"input": input}),
            )
            .map_err(|error| one_pending_review(error).context("could not reply"))?;
        let comment = at(&data, "/addPullRequestReviewThreadReply/comment");
        let reply = non_empty_id(at(comment, "/id"))
            .map(|id| CommentId::new(&id))
            .context("GitHub did not say which comment the reply is")?;
        let review = at(comment, "/pullRequestReview");
        if mode == SendMode::CommentNow && at(review, "/state") == "PENDING" {
            let review_id = self
                .review_holding_new_comment(pr, non_empty_id(at(review, "/id")))
                .map_err(|error| error.context("could not post the reply"))?;
            self.submit_pending_review(&review_id, ReviewVerdict::Comment, "")
                .map_err(|error| error.context("could not post the reply"))?;
        }
        Ok(reply)
    }

    fn set_thread_resolved(
        &self,
        thread_id: &ThreadId,
        resolution: Resolution,
    ) -> Result<(), GitHubError> {
        let mutation = match resolution {
            Resolution::Resolved => "resolveReviewThread",
            Resolution::Unresolved => "unresolveReviewThread",
        };
        self.graphql(
            &format!(
                "mutation($input: {}Input!) {{ {mutation}(input: $input) {{ clientMutationId }} }}",
                capitalized(mutation)
            ),
            &serde_json::json!({"input": {"threadId": thread_id}}),
        )
        .map_err(|error| error.context(format!("could not change the thread {thread_id}")))?;
        Ok(())
    }

    fn set_reaction(
        &self,
        comment_id: &CommentId,
        reaction: Reaction,
        change: ReactionChange,
    ) -> Result<(), GitHubError> {
        self.change_reaction(comment_id.as_str(), reaction, change)
            .map_err(|error| error.context("could not change the reaction"))
    }

    fn edit_comment(&self, comment_id: &CommentId, body: &str) -> Result<(), GitHubError> {
        self.graphql(
            "mutation($input: UpdatePullRequestReviewCommentInput!) {
                updatePullRequestReviewComment(input: $input) { pullRequestReviewComment { id } }
            }",
            &serde_json::json!({"input": {"pullRequestReviewCommentId": comment_id, "body": body}}),
        )
        .map_err(|error| error.context("could not edit the comment"))?;
        Ok(())
    }

    fn delete_comment(&self, comment_id: &CommentId) -> Result<(), GitHubError> {
        self.graphql(
            "mutation($input: DeletePullRequestReviewCommentInput!) {
                deletePullRequestReviewComment(input: $input) { clientMutationId }
            }",
            &serde_json::json!({"input": {"id": comment_id}}),
        )
        .map_err(|error| error.context("could not delete the comment"))?;
        Ok(())
    }
}

impl ReviewWritesPort for GitHub {
    fn has_pending_review(&self, pr: &PullRequestUrl) -> Result<bool, GitHubError> {
        let data = self
            .graphql(
                "query($owner: String!, $name: String!, $number: Int!) {
                    repository(owner: $owner, name: $name) { pullRequest(number: $number) {
                        reviews(states: PENDING, first: 10) { nodes { viewerDidAuthor } }
                    } }
                }",
                &pull_request_variables(pr, &serde_json::Value::Null),
            )
            .map_err(|error| {
                error.context(format!("could not look for a pending review on {pr}"))
            })?;
        let reviews = at(&data, "/repository/pullRequest/reviews/nodes")
            .as_array()
            .with_context(|| format!("could not read the reviews of {pr} from GitHub"))?;
        Ok(reviews
            .iter()
            .any(|review| at(review, "/viewerDidAuthor").as_bool() == Some(true)))
    }

    fn viewer_latest_review(
        &self,
        pr: &PullRequestUrl,
    ) -> Result<Option<SubmittedReview>, GitHubError> {
        let data = self
            .graphql(
                "query($owner: String!, $name: String!, $number: Int!) {
                    repository(owner: $owner, name: $name) { pullRequest(number: $number) {
                        viewerLatestReview { state body submittedAt commit { oid } }
                    } }
                }",
                &pull_request_variables(pr, &serde_json::Value::Null),
            )
            .map_err(|error| error.context(format!("could not read your last review of {pr}")))?;
        let review = at(&data, "/repository/pullRequest/viewerLatestReview");
        if review.is_null() {
            return Ok(None);
        }
        let text = |pointer: &str| at(review, pointer).as_str().map(str::to_owned);
        let state = text("/state")
            .with_context(|| format!("GitHub did not say how your last review of {pr} ended"))?;
        let Some(outcome) = ReviewOutcome::from_github_state(&state) else {
            return Ok(None);
        };
        let missing =
            |what: &str| format!("GitHub did not give the {what} of your last review of {pr}");
        Ok(Some(SubmittedReview {
            outcome,
            body: text("/body").unwrap_or_default(),
            submitted_at: text("/submittedAt").with_context(|| missing("time"))?,
            commit: Sha::new(&text("/commit/oid").with_context(|| missing("commit"))?),
        }))
    }

    fn viewer_authored(&self, pr: &PullRequestUrl) -> Result<bool, GitHubError> {
        let data = self
            .graphql(
                "query($owner: String!, $name: String!, $number: Int!) {
                    repository(owner: $owner, name: $name) { pullRequest(number: $number) { viewerDidAuthor } }
                }",
                &pull_request_variables(pr, &serde_json::Value::Null),
            )
            .map_err(|error| error.context(format!("could not tell who wrote {pr}")))?;
        Ok(at(&data, "/repository/pullRequest/viewerDidAuthor")
            .as_bool()
            .with_context(|| format!("GitHub did not say who wrote {pr}"))?)
    }

    fn add_review_thread(
        &self,
        pr: &PullRequestUrl,
        place: &CommentPlace,
        body: &str,
    ) -> Result<(), GitHubError> {
        self.add_thread_to_pending_review(pr, place, body)
            .map(|_| ())
    }

    /// A thread in a new pending review, posted right away. GitHub posts nothing past an existing
    /// pending review, so the caller checks for one first.
    fn comment_now(
        &self,
        pr: &PullRequestUrl,
        place: &CommentPlace,
        body: &str,
    ) -> Result<(), GitHubError> {
        let reported = self.add_thread_to_pending_review(pr, place, body)?;
        let review_id = self
            .review_holding_new_comment(pr, reported)
            .map_err(|error| error.context("could not post the comment"))?;
        let posted = self.graphql(
            "mutation($input: SubmitPullRequestReviewInput!) {
                submitPullRequestReview(input: $input) { clientMutationId }
            }",
            &serde_json::json!({"input": {"pullRequestReviewId": review_id, "event": "COMMENT"}}),
        );
        if let Err(error) = posted {
            let taken_back = self.graphql(
                "mutation($input: DeletePullRequestReviewInput!) {
                    deletePullRequestReview(input: $input) { clientMutationId }
                }",
                &serde_json::json!({"input": {"pullRequestReviewId": review_id}}),
            );
            let error = error.context("could not post the comment");
            return Err(match taken_back {
                Ok(_) => error,
                Err(cleanup) => error.context(format!(
                    "the comment is left in your pending review on GitHub, which could not be deleted ({cleanup:#})"
                )),
            });
        }
        Ok(())
    }

    /// Sends the viewer's pending review, or a new review with just the verdict and body when there is none.
    fn submit_review(
        &self,
        pr: &PullRequestUrl,
        verdict: ReviewVerdict,
        body: &str,
    ) -> Result<(), GitHubError> {
        if let Some(review_id) = self.pending_review_id(pr)? {
            return self
                .submit_pending_review(&review_id, verdict, body)
                .map_err(|error| error.context("could not send the review"));
        }
        self.graphql(
            "mutation($input: AddPullRequestReviewInput!) {
                addPullRequestReview(input: $input) { clientMutationId }
            }",
            &serde_json::json!({"input": {
                "pullRequestId": self.pull_request_id(pr)?,
                "event": verdict.github_name(),
                "body": body,
            }}),
        )
        .map_err(|error| error.context("could not send the review"))?;
        Ok(())
    }
}

/// Reads one repo through the GitHub API, for PRs without a local clone.
pub struct GitHubCodeSource {
    github: GitHub,
    repo: RepoName,
    fallback_reason: Option<String>,
}

#[derive(Deserialize)]
struct CompareJson {
    merge_base_commit: CommitRefJson,
}

impl GitHubCodeSource {
    /// Read from GitHub because the local clone could not give the commits.
    pub fn instead_of_clone_because(self, reason: String) -> Self {
        Self {
            fallback_reason: Some(reason),
            ..self
        }
    }

    fn compare_path(&self, base: &Sha, head: &Sha) -> String {
        format!("repos/{}/compare/{base}...{head}", self.repo)
    }
}

impl CodeSource for GitHubCodeSource {
    fn fallback_reason(&self) -> Option<&str> {
        self.fallback_reason.as_deref()
    }

    fn file_at(&self, sha: &Sha, path: &FilePath) -> anyhow::Result<String> {
        Ok(String::from_utf8_lossy(&self.blob_at(sha, path)?).into_owned())
    }

    fn blob_at(&self, sha: &Sha, path: &FilePath) -> anyhow::Result<Arc<[u8]>> {
        let url = format!(
            "repos/{}/contents/{}?ref={sha}",
            self.repo,
            percent_encode_path(path.as_str())
        );
        let mut response = self
            .github
            .get(&url, "application/vnd.github.raw+json")
            .with_context(|| format!("could not read {path} at {sha} from GitHub"))?;
        Ok(response.body_mut().read_to_vec()?.into())
    }

    fn blob_info_at(&self, sha: &Sha, path: &FilePath) -> anyhow::Result<BlobInfo> {
        #[derive(Deserialize)]
        struct ContentsJson {
            sha: String,
            size: u64,
        }
        let url = format!(
            "repos/{}/contents/{}?ref={sha}",
            self.repo,
            percent_encode_path(path.as_str())
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
        parse_unified_diff(&patch)
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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ReviewThreadInput<'a> {
    pull_request_id: &'a str,
    path: &'a str,
    body: &'a str,
    subject_type: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    line: Option<LineNumber>,
    #[serde(skip_serializing_if = "Option::is_none")]
    side: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    start_line: Option<LineNumber>,
    #[serde(skip_serializing_if = "Option::is_none")]
    start_side: Option<&'static str>,
}

impl<'a> ReviewThreadInput<'a> {
    fn new(pull_request_id: &'a str, path: &'a str, body: &'a str) -> Self {
        Self {
            pull_request_id,
            path,
            body,
            subject_type: "LINE",
            line: None,
            side: None,
            start_line: None,
            start_side: None,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ReplyInput<'a> {
    pull_request_review_thread_id: &'a str,
    body: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pull_request_review_id: Option<String>,
}

/// GitHub counts lines from 1 and answers `null` for none; a 0 is taken as none too, so the thread still shows.
fn thread_line(line: Option<usize>) -> Option<LineNumber> {
    line.and_then(|line| LineNumber::try_from(line).ok())
}

/// GitHub gives this refusal no error type, only the message
/// "User can only have one pending review per pull request".
const ONE_PENDING_REVIEW: &str = "one pending review";

/// Only a new thread or a reply can meet the viewer's other pending review; elsewhere the
/// same words stay a plain failure.
fn one_pending_review(error: GitHubError) -> GitHubError {
    match error {
        GitHubError::Other(error) if format!("{error:#}").contains(ONE_PENDING_REVIEW) => {
            GitHubError::PendingReviewExists
        }
        error => error,
    }
}

fn graphql_side(side: Side) -> &'static str {
    match side {
        Side::Base => "LEFT",
        Side::Head => "RIGHT",
    }
}

fn non_empty_id(id: &serde_json::Value) -> Option<String> {
    id.as_str().filter(|id| !id.is_empty()).map(str::to_owned)
}

fn token_from_gh_cli() -> Option<String> {
    let output = Command::new("gh").args(["auth", "token"]).output().ok()?;
    let token = String::from_utf8(output.stdout).ok()?.trim().to_owned();
    (output.status.success() && !token.is_empty()).then_some(token)
}

const REVIEW_THREADS_QUERY: &str = "query($owner: String!, $name: String!, $number: Int!, $after: String) {
  repository(owner: $owner, name: $name) { pullRequest(number: $number) {
    reviewThreads(first: 100, after: $after) {
      pageInfo { hasNextPage endCursor }
      nodes { id isResolved isOutdated path line startLine originalLine originalStartLine diffSide subjectType
        comments(first: 50) { nodes { id url body viewerDidAuthor reactionGroups { content viewerHasReacted reactors { totalCount } } originalCommit { oid } author { login __typename avatarUrl(size: 64) } } } }
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
        path: FilePath,
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
        #[serde(default)]
        viewer_did_author: bool,
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
        #[serde(default, rename = "avatarUrl")]
        avatar_url: String,
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
                id: ThreadId::new(&thread.id),
                is_resolved: thread.is_resolved,
                is_outdated: thread.is_outdated,
                path: thread.path,
                line: thread_line(thread.line),
                start_line: thread_line(thread.start_line),
                side: if thread.diff_side == "LEFT" {
                    Side::Base
                } else {
                    Side::Head
                },
                is_file_comment: thread.subject_type == "FILE",
                original_line: thread_line(thread.original_line),
                original_start_line: thread_line(thread.original_start_line),
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
                        avatar_url: comment
                            .author
                            .as_ref()
                            .map(|author| author.avatar_url.clone())
                            .filter(|url| !url.is_empty()),
                        is_bot: comment
                            .author
                            .as_ref()
                            .is_some_and(|author| author.kind == "Bot"),
                        author: comment
                            .author
                            .map_or_else(|| "ghost".to_owned(), |author| author.login),
                        id: CommentId::new(&comment.id),
                        by_viewer: comment.viewer_did_author,
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

/// Avatars are public on GitHub's picture host, so no token goes along.
pub struct GitHubAvatars;

/// Far above a 64 px picture; a bigger answer is not an avatar.
const AVATAR_LIMIT: u64 = 1024 * 1024;

impl AvatarPort for GitHubAvatars {
    fn avatar(&self, url: &str) -> anyhow::Result<Vec<u8>> {
        let mut response = ureq::get(url)
            .header("User-Agent", "guided-review")
            .call()
            .with_context(|| format!("could not download {url}"))?;
        response
            .body_mut()
            .with_config()
            .limit(AVATAR_LIMIT)
            .read_to_vec()
            .with_context(|| format!("could not download {url}"))
    }
}
