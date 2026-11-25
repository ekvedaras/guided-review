//! GitHub's API for one PR, held in memory, so the GitHub adapter meets the contract against state it
//! changes and reads back, never the real network. It answers GraphQL and the REST calls the adapter
//! makes in the shapes GitHub answers (`github_threads.rs` has recorded ones), two nodes a page so
//! the adapter's paging runs too.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;

use guided_review::viewer::{
    AutoMerge, CheckState, Checks, IssueState, MergeMethod, Mergeability, Merging, PrStatus,
    Readiness, ReviewOutcome, ViewedState,
};
use serde_json::{Value, json};

use super::{PullOnHost, Reach, SeedCode, SeedIssue, UPDATED_AT, VIEWER, avatar_of, blob_id};

const PAGE: usize = 2;
const PULL_REQUEST_ID: &str = "PR_7";
const REPOSITORY_ID: u64 = 1;
const SUBMITTED_AT: &str = "2026-11-06T09:00:00Z";

/// Serves `pull` until the test ends. Returns the API's address.
pub fn serve(pull: &PullOnHost) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let api = Arc::new(Mutex::new(Api::holding(pull)));
    thread::spawn(move || {
        for stream in listener.incoming() {
            answer(stream.unwrap(), &api);
        }
    });
    url
}

struct Request {
    method: String,
    path: String,
    query: Vec<(String, String)>,
    accept: String,
    body: Vec<u8>,
}

struct Answer {
    status: &'static str,
    content_type: &'static str,
    body: Vec<u8>,
}

impl Answer {
    fn json(status: &'static str, body: &Value) -> Self {
        Self {
            status,
            content_type: "application/json",
            body: body.to_string().into_bytes(),
        }
    }

    fn ok(body: &Value) -> Self {
        Self::json("200 OK", body)
    }

    fn not_found() -> Self {
        Self::json("404 Not Found", &json!({"message": "Not Found"}))
    }

    fn text(body: &str) -> Self {
        Self {
            status: "200 OK",
            content_type: "text/plain",
            body: body.as_bytes().to_vec(),
        }
    }
}

fn read_request(stream: &TcpStream) -> Request {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut request_line = String::new();
    reader.read_line(&mut request_line).unwrap();
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap().to_owned();
    let target = parts.next().unwrap();
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let mut length = 0;
    let mut accept = String::new();
    loop {
        let mut header = String::new();
        reader.read_line(&mut header).unwrap();
        if header.trim().is_empty() {
            break;
        }
        let lower = header.to_lowercase();
        if let Some(value) = lower.strip_prefix("content-length:") {
            length = value.trim().parse().unwrap();
        }
        if let Some(value) = lower.strip_prefix("accept:") {
            value.trim().clone_into(&mut accept);
        }
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body).unwrap();
    Request {
        method,
        path: decoded(path),
        query: query
            .split('&')
            .filter(|pair| !pair.is_empty())
            .map(|pair| {
                let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
                (decoded(name), decoded(value))
            })
            .collect(),
        accept,
        body,
    }
}

fn decoded(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'%' && at + 2 < bytes.len() {
            out.push(
                u8::from_str_radix(std::str::from_utf8(&bytes[at + 1..at + 3]).unwrap(), 16)
                    .unwrap(),
            );
            at += 3;
        } else {
            out.push(if bytes[at] == b'+' { b' ' } else { bytes[at] });
            at += 1;
        }
    }
    String::from_utf8(out).unwrap()
}

fn answer(mut stream: TcpStream, api: &Mutex<Api>) {
    let request = read_request(&stream);
    let mut api = api.lock().unwrap();
    let answer = if matches!(api.reach, Reach::ServerError) {
        Answer::json("503 Service Unavailable", &json!({"message": "unicorn"}))
    } else {
        api.route(&request)
    };
    write!(
        stream,
        "HTTP/1.1 {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        answer.status,
        answer.content_type,
        answer.body.len()
    )
    .unwrap();
    stream.write_all(&answer.body).unwrap();
}

struct Api {
    reach: Reach,
    pull: PullOnHost,
    description: String,
    files: Vec<(String, &'static str)>,
    threads: Vec<Thread>,
    pending_review: Option<String>,
    /// The viewer's latest sent review: GitHub's state, its body and the commit it saw.
    viewer_last: Option<(&'static str, String, String)>,
    pull_request_reactions: Vec<String>,
    uploads: usize,
    made: usize,
}

struct Thread {
    id: String,
    path: String,
    line: usize,
    resolved: bool,
    comments: Vec<Comment>,
}

struct Comment {
    id: String,
    author: String,
    body: String,
    /// The reactions the viewer put on it, by GitHub's content name.
    reactions: Vec<String>,
}

impl Api {
    fn holding(pull: &PullOnHost) -> Self {
        Self {
            reach: pull.reach,
            pull: pull.clone(),
            description: pull.description.to_owned(),
            files: pull
                .files
                .iter()
                .map(|(path, state)| ((*path).to_owned(), github_viewed_state(*state)))
                .collect(),
            threads: pull
                .threads
                .iter()
                .map(|seed| Thread {
                    id: seed.id.to_owned(),
                    path: seed.path.to_owned(),
                    line: seed.line,
                    resolved: seed.resolved,
                    comments: seed
                        .comments
                        .iter()
                        .map(|comment| Comment {
                            id: comment.id.to_owned(),
                            author: comment.author.to_owned(),
                            body: comment.body.to_owned(),
                            reactions: Vec::new(),
                        })
                        .collect(),
                })
                .collect(),
            pending_review: pull.web_pending_review.then(|| "PRR_web".to_owned()),
            viewer_last: None,
            pull_request_reactions: Vec::new(),
            uploads: 0,
            made: 0,
        }
    }

    fn route(&mut self, request: &Request) -> Answer {
        let query = |name: &str| {
            request
                .query
                .iter()
                .find(|(known, _)| known == name)
                .map(|(_, value)| value.as_str())
        };
        let path = request.path.as_str();
        match (request.method.as_str(), path) {
            ("POST", "/graphql") => {
                let body: Value = serde_json::from_slice(&request.body).unwrap();
                let text = body["query"].as_str().unwrap();
                Answer::ok(&self.graphql(text, &body["variables"]))
            }
            ("POST", "/user-attachments/assets") => {
                let repository = query("repository_id").and_then(|id| id.parse::<u64>().ok());
                if repository != Some(REPOSITORY_ID) || query("name").is_none() {
                    return Answer::json("422 Unprocessable Entity", &json!({"message": "bad"}));
                }
                self.uploads += 1;
                Answer::json(
                    "201 Created",
                    &json!({"url": format!("https://github.com/user-attachments/assets/upload-{}", self.uploads)}),
                )
            }
            ("GET", "/repos/acme/shop") => Answer::ok(&json!({"id": REPOSITORY_ID})),
            ("GET", "/repos/acme/shop/pulls/7") => Answer::ok(&self.pull_json()),
            ("GET", "/repos/acme/shop/pulls") => {
                let wanted = query("head").unwrap_or_default();
                let head_owner = self.pull.fork_owner.unwrap_or("acme");
                let listed = query("state") == Some("open")
                    && self.is_open()
                    && wanted == format!("{head_owner}:{}", self.pull.head_branch);
                let pulls: Vec<Value> = listed
                    .then(|| json!({"number": 7, "title": self.pull.title}))
                    .into_iter()
                    .collect();
                Answer::ok(&Value::Array(pulls))
            }
            _ => {
                if let Some(file) = path.strip_prefix("/repos/acme/shop/contents/") {
                    return self.contents(file, query("ref").unwrap_or_default(), &request.accept);
                }
                if let Some(range) = path.strip_prefix("/repos/acme/shop/compare/") {
                    return self.compare(range, &request.accept);
                }
                Answer::not_found()
            }
        }
    }

    fn is_open(&self) -> bool {
        matches!(self.pull.status, PrStatus::Open(_))
    }

    fn pull_json(&self) -> Value {
        let head_repo = format!("{}/shop", self.pull.fork_owner.unwrap_or("acme"));
        json!({
            "number": 7,
            "title": self.pull.title,
            "body": self.description,
            "base": {"sha": self.pull.base_branch_tip, "ref": self.pull.base_branch},
            "head": {
                "sha": self.pull.head,
                "ref": self.pull.head_branch,
                "repo": {"full_name": head_repo},
            },
        })
    }

    fn contents(&self, path: &str, sha: &str, accept: &str) -> Answer {
        let code: &SeedCode = &self.pull.code;
        let Some((_, _, text)) = code
            .files
            .iter()
            .find(|(at, known, _)| *at == sha && *known == path)
        else {
            return Answer::not_found();
        };
        if accept.contains("raw") {
            Answer::text(text)
        } else {
            Answer::ok(&json!({"sha": blob_id(text.as_bytes()), "size": text.len()}))
        }
    }

    fn compare(&self, range: &str, accept: &str) -> Answer {
        let tips = format!("{}...{}", self.pull.base_branch_tip, self.pull.head);
        if range != tips {
            return Answer::not_found();
        }
        if accept.contains("diff") {
            Answer::text(self.pull.code.diff)
        } else {
            Answer::ok(&json!({"merge_base_commit": {"sha": self.pull.code.merge_base}}))
        }
    }

    /// The whole answer: `data`, or `errors` as GitHub gives them.
    fn graphql(&mut self, query: &str, variables: &Value) -> Value {
        let input = &variables["input"];
        let data = if query.contains("unmarkFileAsViewed(") {
            self.set_viewed(input, "UNVIEWED")
        } else if query.contains("markFileAsViewed(") {
            self.set_viewed(input, "VIEWED")
        } else if query.contains("unresolveReviewThread(") {
            self.resolve(input, false)
        } else if query.contains("resolveReviewThread(") {
            self.resolve(input, true)
        } else if query.contains("addPullRequestReviewThreadReply(") {
            self.reply(input)
        } else if query.contains("addPullRequestReviewThread(") {
            self.add_thread(input)
        } else if query.contains("submitPullRequestReview(") {
            self.submit(input)
        } else if query.contains("addPullRequestReview(") {
            self.add_review(input)
        } else if query.contains("deletePullRequestReview(") {
            self.delete_review(input)
        } else if query.contains("updatePullRequestReviewComment(") {
            self.edit_comment(input)
        } else if query.contains("deletePullRequestReviewComment(") {
            self.delete_comment(input)
        } else if query.contains("removeReaction(") {
            self.react(input, false)
        } else if query.contains("addReaction(") {
            self.react(input, true)
        } else if query.contains("updatePullRequest(") {
            self.describe(input)
        } else if query.contains("convertPullRequestToDraft(") {
            self.set_readiness(input, Readiness::Draft)
        } else if query.contains("markPullRequestReadyForReview(") {
            self.set_readiness(input, Readiness::Ready)
        } else if query.contains("enablePullRequestAutoMerge(") {
            self.set_auto_merge(input, true)
        } else if query.contains("disablePullRequestAutoMerge(") {
            self.set_auto_merge(input, false)
        } else if query.contains("mergePullRequest(") {
            self.merge(input)
        } else if query.contains("reviews(states: PENDING") {
            Ok(self.pending_reviews())
        } else if query.contains("viewerLatestReview") {
            Ok(self.reviews())
        } else if query.contains("pullRequest(number: $number) { viewerDidAuthor }") {
            Ok(
                json!({"repository": {"pullRequest": {"viewerDidAuthor": self.pull.author == VIEWER}}}),
            )
        } else if query.contains("files(first:") {
            Ok(self.files_page(&variables["after"]))
        } else if query.contains("reviewThreads(first:") {
            Ok(self.threads_page(&variables["after"]))
        } else if query.contains("statusCheckRollup") {
            Ok(self.status())
        } else if query.contains("mergeStateStatus") {
            Ok(self.merging())
        } else if query.contains("closingIssuesReferences") {
            Ok(self.closing_issues())
        } else if query.contains("headRepositoryOwner") {
            Ok(self.origin())
        } else if query.contains("issueOrPullRequest(") {
            Ok(self.issue_details(variables))
        } else if query.contains("organizations(first:") {
            Ok(json!({"viewer": {"login": VIEWER, "organizations": {"nodes": []}}}))
        } else if query.contains("search(query:") {
            Ok(self.search())
        } else if query.contains("pullRequest(number: $number) { id }") {
            Ok(json!({"repository": {"pullRequest": {"id": PULL_REQUEST_ID}}}))
        } else {
            Err(format!("this double does not know the query {query}"))
        };
        match data {
            Ok(data) => json!({"data": data}),
            Err(message) => {
                json!({"errors": [{"type": "NOT_FOUND", "message": message}], "data": null})
            }
        }
    }

    fn is_this_pull_request(input: &Value) -> Result<(), String> {
        let id = input["pullRequestId"].as_str().unwrap_or_default();
        if id == PULL_REQUEST_ID {
            Ok(())
        } else {
            Err(format!(
                "Could not resolve to a PullRequest with the id of '{id}'."
            ))
        }
    }

    fn set_viewed(&mut self, input: &Value, state: &'static str) -> Result<Value, String> {
        Self::is_this_pull_request(input)?;
        let path = input["path"].as_str().unwrap();
        let file = self
            .files
            .iter_mut()
            .find(|(known, _)| known == path)
            .ok_or_else(|| format!("no file {path} in the pull request"))?;
        file.1 = state;
        Ok(json!({"x": {"clientMutationId": null}}))
    }

    fn thread(&mut self, id: &Value) -> Result<&mut Thread, String> {
        let id = id.as_str().unwrap();
        self.threads
            .iter_mut()
            .find(|thread| thread.id == id)
            .ok_or_else(|| {
                format!("Could not resolve to a PullRequestReviewThread with the id of '{id}'.")
            })
    }

    fn comment(&mut self, id: &Value) -> Result<&mut Comment, String> {
        let id = id.as_str().unwrap();
        self.threads
            .iter_mut()
            .flat_map(|thread| thread.comments.iter_mut())
            .find(|comment| comment.id == id)
            .ok_or_else(|| format!("Could not resolve to a node with the global id of '{id}'."))
    }

    fn resolve(&mut self, input: &Value, resolved: bool) -> Result<Value, String> {
        self.thread(&input["threadId"])?.resolved = resolved;
        Ok(json!({"x": {"clientMutationId": null}}))
    }

    /// The viewer's pending review, which GitHub starts when there is none.
    fn pending_review_id(&mut self) -> String {
        self.made += 1;
        let made = self.made;
        self.pending_review
            .get_or_insert_with(|| format!("PRR_{made}"))
            .clone()
    }

    fn reply(&mut self, input: &Value) -> Result<Value, String> {
        self.thread(&input["pullRequestReviewThreadId"])?;
        let review_id = self.pending_review_id();
        let comment_id = format!("PRRC_{}", self.made);
        self.thread(&input["pullRequestReviewThreadId"])?
            .comments
            .push(Comment {
                id: comment_id.clone(),
                author: VIEWER.to_owned(),
                body: input["body"].as_str().unwrap().to_owned(),
                reactions: Vec::new(),
            });
        Ok(json!({"addPullRequestReviewThreadReply": {"comment": {
            "id": comment_id,
            "pullRequestReview": {"id": review_id, "state": "PENDING"},
        }}}))
    }

    /// A new thread joins the viewer's pending review, as a reply does.
    fn add_thread(&mut self, input: &Value) -> Result<Value, String> {
        Self::is_this_pull_request(input)?;
        let review_id = self.pending_review_id();
        Ok(json!({"addPullRequestReviewThread": {"thread": {
            "id": format!("PRRT_{}", self.made),
            "comments": {"nodes": [{"pullRequestReview": {"id": review_id}}]},
        }}}))
    }

    fn sent(&mut self, event: &Value, body: &Value) {
        let state = match event.as_str().unwrap() {
            "APPROVE" => "APPROVED",
            "REQUEST_CHANGES" => "CHANGES_REQUESTED",
            _ => "COMMENTED",
        };
        self.viewer_last = Some((
            state,
            body.as_str().unwrap_or_default().to_owned(),
            self.pull.head.to_owned(),
        ));
    }

    fn submit(&mut self, input: &Value) -> Result<Value, String> {
        let review_id = input["pullRequestReviewId"].as_str().unwrap();
        if self.pending_review.as_deref() != Some(review_id) {
            return Err(format!(
                "Could not resolve to a PullRequestReview with the id of '{review_id}'."
            ));
        }
        self.pending_review = None;
        self.sent(&input["event"], &input["body"]);
        Ok(json!({"submitPullRequestReview": {"clientMutationId": null}}))
    }

    fn add_review(&mut self, input: &Value) -> Result<Value, String> {
        Self::is_this_pull_request(input)?;
        if self.pending_review.is_some() {
            return Err("User can only have one pending review per pull request".to_owned());
        }
        self.sent(&input["event"], &input["body"]);
        Ok(json!({"addPullRequestReview": {"clientMutationId": null}}))
    }

    fn delete_review(&mut self, input: &Value) -> Result<Value, String> {
        let review_id = input["pullRequestReviewId"].as_str().unwrap();
        if self.pending_review.as_deref() != Some(review_id) {
            return Err(format!(
                "Could not resolve to a PullRequestReview with the id of '{review_id}'."
            ));
        }
        self.pending_review = None;
        Ok(json!({"deletePullRequestReview": {"clientMutationId": null}}))
    }

    fn edit_comment(&mut self, input: &Value) -> Result<Value, String> {
        let comment = self.comment(&input["pullRequestReviewCommentId"])?;
        comment.body = input["body"].as_str().unwrap().to_owned();
        Ok(
            json!({"updatePullRequestReviewComment": {"pullRequestReviewComment": {"id": comment.id}}}),
        )
    }

    /// A thread whose last comment is deleted is gone too.
    fn delete_comment(&mut self, input: &Value) -> Result<Value, String> {
        self.comment(&input["id"])?;
        let id = input["id"].as_str().unwrap();
        for thread in &mut self.threads {
            thread.comments.retain(|comment| comment.id != id);
        }
        self.threads.retain(|thread| !thread.comments.is_empty());
        Ok(json!({"deletePullRequestReviewComment": {"clientMutationId": null}}))
    }

    fn react(&mut self, input: &Value, add: bool) -> Result<Value, String> {
        let content = input["content"].as_str().unwrap().to_owned();
        let reactions = if input["subjectId"] == PULL_REQUEST_ID {
            &mut self.pull_request_reactions
        } else {
            &mut self.comment(&input["subjectId"])?.reactions
        };
        reactions.retain(|known| *known != content);
        if add {
            reactions.push(content);
        }
        Ok(json!({"x": {"clientMutationId": null}}))
    }

    fn describe(&mut self, input: &Value) -> Result<Value, String> {
        Self::is_this_pull_request(input)?;
        input["body"]
            .as_str()
            .unwrap()
            .clone_into(&mut self.description);
        Ok(json!({"updatePullRequest": {"pullRequest": {"id": PULL_REQUEST_ID}}}))
    }

    fn set_readiness(&mut self, input: &Value, readiness: Readiness) -> Result<Value, String> {
        Self::is_this_pull_request(input)?;
        let PrStatus::Open(open) = &mut self.pull.status else {
            return Err("Pull request is closed".to_owned());
        };
        open.readiness = readiness;
        Ok(json!({"x": {"clientMutationId": null}}))
    }

    fn set_auto_merge(&mut self, input: &Value, on: bool) -> Result<Value, String> {
        Self::is_this_pull_request(input)?;
        self.pull.merging.auto_merge = if on {
            AutoMerge::On(merge_method_named(input["mergeMethod"].as_str().unwrap()))
        } else {
            AutoMerge::Off
        };
        Ok(json!({"x": {"clientMutationId": null}}))
    }

    fn merge(&mut self, input: &Value) -> Result<Value, String> {
        Self::is_this_pull_request(input)?;
        self.pull.status = PrStatus::Merged;
        Ok(json!({"x": {"clientMutationId": null}}))
    }

    fn pending_reviews(&self) -> Value {
        let nodes: Vec<Value> = self
            .pending_review
            .iter()
            .map(|id| json!({"id": id, "viewerDidAuthor": true}))
            .collect();
        json!({"repository": {"pullRequest": {"reviews": {"nodes": nodes}}}})
    }

    fn reviews(&self) -> Value {
        let listed = |login: &str, state: &str| {
            json!({
                "state": state,
                "commit": {"oid": self.pull.head},
                "author": {"login": login, "avatarUrl": avatar_of(login)},
            })
        };
        let mut latest: Vec<Value> = self
            .pull
            .reviews
            .iter()
            .map(|(login, outcome)| listed(login, review_state(*outcome)))
            .collect();
        latest.extend(
            self.viewer_last
                .iter()
                .map(|(state, _, _)| listed(VIEWER, state)),
        );
        let opinionated: Vec<Value> = latest
            .iter()
            .filter(|review| {
                matches!(
                    review["state"].as_str(),
                    Some("APPROVED" | "CHANGES_REQUESTED")
                )
            })
            .cloned()
            .collect();
        let requested: Vec<Value> = self
            .pull
            .asks_viewer_to_review
            .then(|| {
                json!({"requestedReviewer": {
                    "__typename": "User", "login": VIEWER, "avatarUrl": avatar_of(VIEWER),
                }})
            })
            .into_iter()
            .collect();
        let viewer_latest = self.viewer_last.as_ref().map_or(Value::Null, |(state, body, commit)| {
            json!({"state": state, "body": body, "submittedAt": SUBMITTED_AT, "commit": {"oid": commit}})
        });
        json!({
            "viewer": {"login": VIEWER},
            "repository": {"pullRequest": {
                "author": {"login": self.pull.author},
                "viewerLatestReview": viewer_latest,
                "latestOpinionatedReviews": {"nodes": opinionated},
                "latestReviews": {"nodes": latest},
                "reviewRequests": {"nodes": requested},
            }},
        })
    }

    fn files_page(&self, after: &Value) -> Value {
        let nodes: Vec<Value> = self
            .files
            .iter()
            .map(|(path, state)| json!({"path": path, "viewerViewedState": state}))
            .collect();
        let (page_info, nodes) = page_of(nodes, after);
        json!({"repository": {"pullRequest": {
            "id": PULL_REQUEST_ID,
            "files": {"pageInfo": page_info, "nodes": nodes},
        }}})
    }

    fn threads_page(&self, after: &Value) -> Value {
        let nodes: Vec<Value> = self.threads.iter().map(Thread::as_github_node).collect();
        let (page_info, nodes) = page_of(nodes, after);
        json!({"repository": {"pullRequest": {
            "reviewThreads": {"pageInfo": page_info, "nodes": nodes},
        }}})
    }

    fn status(&self) -> Value {
        let pull = match &self.pull.status {
            PrStatus::Merged => {
                json!({"state": "MERGED", "isDraft": false, "viewerCanUpdate": false})
            }
            PrStatus::Closed => {
                json!({"state": "CLOSED", "isDraft": false, "viewerCanUpdate": false})
            }
            PrStatus::Open(open) => {
                let rollup = match open.checks {
                    Checks::NoneRun => Value::Null,
                    checks => {
                        let contexts: Vec<Value> = open
                            .check_runs
                            .iter()
                            .map(|run| {
                                let (status, conclusion) = check_run_state(run.state);
                                json!({
                                    "__typename": "CheckRun",
                                    "name": &*run.name,
                                    "status": status,
                                    "conclusion": conclusion,
                                    "detailsUrl": run.page.as_deref(),
                                })
                            })
                            .collect();
                        json!({"state": rollup_state(checks), "contexts": {"nodes": contexts}})
                    }
                };
                json!({
                    "state": "OPEN",
                    "isDraft": open.readiness == Readiness::Draft,
                    "viewerCanUpdate": open.viewer_may_change_readiness,
                    "commits": {"nodes": [{"commit": {"statusCheckRollup": rollup}}]},
                })
            }
        };
        json!({"repository": {"pullRequest": pull}})
    }

    fn merging(&self) -> Value {
        let merging: &Merging = &self.pull.merging;
        let allows = |method| merging.methods.allows(method);
        json!({"repository": {
            "autoMergeAllowed": merging.repo_allows_auto_merge,
            "mergeCommitAllowed": allows(MergeMethod::Merge),
            "squashMergeAllowed": allows(MergeMethod::Squash),
            "rebaseMergeAllowed": allows(MergeMethod::Rebase),
            "viewerPermission": if merging.viewer_may_merge { "WRITE" } else { "READ" },
            "pullRequest": {
                "mergeStateStatus": match merging.mergeability {
                    Mergeability::Now => "CLEAN",
                    Mergeability::Blocked => "BLOCKED",
                },
                "viewerCanEnableAutoMerge": merging.viewer_may_enable_auto_merge,
                "viewerCanDisableAutoMerge": merging.viewer_may_disable_auto_merge,
                "autoMergeRequest": match merging.auto_merge {
                    AutoMerge::Off => Value::Null,
                    AutoMerge::On(method) => json!({"mergeMethod": merge_method_name(method)}),
                },
            },
        }})
    }

    fn closing_issues(&self) -> Value {
        let nodes: Vec<Value> = self
            .pull
            .closing_issues
            .iter()
            .map(|issue| {
                json!({"number": issue.number, "title": issue.title, "repository": {"nameWithOwner": "acme/shop"}})
            })
            .collect();
        json!({"repository": {"pullRequest": {
            "headRefName": self.pull.head_branch,
            "closingIssuesReferences": {"nodes": nodes},
        }}})
    }

    fn origin(&self) -> Value {
        json!({"repository": {"pullRequest": {
            "author": {"login": self.pull.author, "avatarUrl": avatar_of(self.pull.author)},
            "baseRefName": self.pull.base_branch,
            "headRefName": self.pull.head_branch,
            "headRepositoryOwner": {"login": self.pull.fork_owner.unwrap_or("acme")},
            "isCrossRepository": self.pull.fork_owner.is_some(),
        }}})
    }

    /// Each aliased issue, `null` for one the repo does not have.
    fn issue_details(&self, variables: &Value) -> Value {
        let mut data = serde_json::Map::new();
        for place in 0.. {
            let Some(number) = variables[format!("number{place}")].as_u64() else {
                break;
            };
            let issue = self
                .pull
                .closing_issues
                .iter()
                .find(|issue| issue.number == number)
                .map_or(Value::Null, issue_node);
            data.insert(format!("issue{place}"), json!({"issue": issue}));
        }
        Value::Object(data)
    }

    fn search(&self) -> Value {
        let PrStatus::Open(open) = &self.pull.status else {
            return json!({"viewer": {"login": VIEWER}, "s0": {"nodes": []}});
        };
        let requested: Vec<Value> = self
            .pull
            .asks_viewer_to_review
            .then(|| json!({"requestedReviewer": {"login": VIEWER}}))
            .into_iter()
            .collect();
        json!({"viewer": {"login": VIEWER}, "s0": {"nodes": [{
            "number": 7,
            "url": super::pr().to_string(),
            "title": self.pull.title,
            "isDraft": open.readiness == Readiness::Draft,
            "updatedAt": UPDATED_AT,
            "author": {"login": self.pull.author, "avatarUrl": avatar_of(self.pull.author)},
            "reviewRequests": {"nodes": requested},
        }]}})
    }
}

impl Thread {
    fn as_github_node(&self) -> Value {
        let comments: Vec<Value> = self
            .comments
            .iter()
            .map(|comment| {
                let reactions: Vec<Value> = comment
                    .reactions
                    .iter()
                    .map(|content| {
                        json!({"content": content, "viewerHasReacted": true, "reactors": {"totalCount": 1}})
                    })
                    .collect();
                json!({
                    "id": comment.id,
                    "url": format!("https://github.com/acme/shop/pull/7#discussion_r{}", comment.id),
                    "body": comment.body,
                    "viewerDidAuthor": comment.author == VIEWER,
                    "reactionGroups": reactions,
                    "originalCommit": {"oid": "bbb"},
                    "author": {"login": comment.author, "__typename": "User", "avatarUrl": ""},
                })
            })
            .collect();
        json!({
            "id": self.id,
            "isResolved": self.resolved,
            "isOutdated": false,
            "path": self.path,
            "line": self.line,
            "startLine": null,
            "originalLine": self.line,
            "originalStartLine": null,
            "diffSide": "RIGHT",
            "subjectType": "LINE",
            "comments": {"nodes": comments},
        })
    }
}

fn issue_node(issue: &SeedIssue) -> Value {
    let (state, reason) = match issue.state {
        IssueState::Open => ("OPEN", None),
        IssueState::ClosedAsCompleted => ("CLOSED", Some("COMPLETED")),
        IssueState::ClosedAsNotPlanned => ("CLOSED", Some("NOT_PLANNED")),
        IssueState::Closed => ("CLOSED", Some("DUPLICATE")),
        IssueState::Merged => ("MERGED", None),
    };
    let labels: Vec<Value> = issue
        .labels
        .iter()
        .map(|name| json!({"name": name}))
        .collect();
    let assignees: Vec<Value> = issue
        .assignees
        .iter()
        .map(|login| json!({"login": login}))
        .collect();
    json!({
        "title": issue.title,
        "body": issue.body,
        "state": state,
        "stateReason": reason,
        "author": {"login": issue.author},
        "labels": {"nodes": labels},
        "assignees": {"nodes": assignees},
    })
}

fn rollup_state(checks: Checks) -> &'static str {
    match checks {
        Checks::Passing => "SUCCESS",
        Checks::Failing => "FAILURE",
        Checks::Running | Checks::NoneRun => "PENDING",
    }
}

fn check_run_state(state: CheckState) -> (&'static str, Option<&'static str>) {
    match state {
        CheckState::Running => ("IN_PROGRESS", None),
        CheckState::Passing => ("COMPLETED", Some("SUCCESS")),
        CheckState::Failing => ("COMPLETED", Some("FAILURE")),
        CheckState::Neutral => ("COMPLETED", Some("NEUTRAL")),
        CheckState::Skipped => ("COMPLETED", Some("SKIPPED")),
    }
}

fn review_state(outcome: ReviewOutcome) -> &'static str {
    match outcome {
        ReviewOutcome::Approved => "APPROVED",
        ReviewOutcome::ChangesRequested => "CHANGES_REQUESTED",
        ReviewOutcome::Commented => "COMMENTED",
        ReviewOutcome::Dismissed => "DISMISSED",
    }
}

fn merge_method_name(method: MergeMethod) -> &'static str {
    match method {
        MergeMethod::Merge => "MERGE",
        MergeMethod::Squash => "SQUASH",
        MergeMethod::Rebase => "REBASE",
    }
}

fn merge_method_named(name: &str) -> MergeMethod {
    MergeMethod::ALL
        .into_iter()
        .find(|method| merge_method_name(*method) == name)
        .unwrap()
}

/// GitHub's cursor is opaque; here it is the index the next page starts at.
fn page_of(nodes: Vec<Value>, after: &Value) -> (Value, Vec<Value>) {
    let start: usize = after.as_str().map_or(0, |cursor| cursor.parse().unwrap());
    let end = (start + PAGE).min(nodes.len());
    let page_info = json!({"hasNextPage": end < nodes.len(), "endCursor": end.to_string()});
    (
        page_info,
        nodes.into_iter().skip(start).take(end - start).collect(),
    )
}

fn github_viewed_state(state: ViewedState) -> &'static str {
    match state {
        ViewedState::Viewed => "VIEWED",
        ViewedState::Unviewed => "UNVIEWED",
        ViewedState::Dismissed => "DISMISSED",
    }
}
