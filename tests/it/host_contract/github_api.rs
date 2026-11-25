//! GitHub's GraphQL API for one PR, held in memory, so the GitHub adapter meets the contract against
//! state it changes and reads back, never the real network. It answers in the shapes GitHub answers
//! (`github_threads.rs` has recorded ones), two nodes a page so the adapter's paging runs too.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;

use guided_review::viewer::ViewedState;
use serde_json::{Value, json};

use super::{PullOnHost, Reach};

const PAGE: usize = 2;
const VIEWER: &str = "reviewer";
const PULL_REQUEST_ID: &str = "PR_7";

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

fn answer(mut stream: TcpStream, api: &Mutex<Api>) {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut request_line = String::new();
    reader.read_line(&mut request_line).unwrap();
    let mut length = 0;
    loop {
        let mut header = String::new();
        reader.read_line(&mut header).unwrap();
        if header.trim().is_empty() {
            break;
        }
        if let Some(value) = header.to_lowercase().strip_prefix("content-length:") {
            length = value.trim().parse().unwrap();
        }
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body).unwrap();
    let mut api = api.lock().unwrap();
    let (status, answer) = if matches!(api.reach, Reach::ServerError) {
        ("503 Service Unavailable", json!({"message": "unicorn"}))
    } else if request_line.starts_with("POST /graphql ") {
        let request: Value = serde_json::from_slice(&body).unwrap();
        let query = request["query"].as_str().unwrap();
        ("200 OK", api.graphql(query, &request["variables"]))
    } else {
        ("404 Not Found", json!({"message": "Not Found"}))
    };
    let answer = answer.to_string();
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{answer}",
        answer.len()
    )
    .unwrap();
}

struct Api {
    reach: Reach,
    files: Vec<(String, &'static str)>,
    threads: Vec<Thread>,
    pending_review: Option<String>,
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
}

impl Api {
    fn holding(pull: &PullOnHost) -> Self {
        Self {
            reach: pull.reach,
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
                        })
                        .collect(),
                })
                .collect(),
            pending_review: None,
            made: 0,
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
        } else if query.contains("submitPullRequestReview(") {
            self.submit(input)
        } else if query.contains("reviews(states: PENDING") {
            Ok(self.pending_reviews())
        } else if query.contains("files(first:") {
            Ok(self.files_page(&variables["after"]))
        } else if query.contains("reviewThreads(first:") {
            Ok(self.threads_page(&variables["after"]))
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

    fn set_viewed(&mut self, input: &Value, state: &'static str) -> Result<Value, String> {
        assert_eq!(input["pullRequestId"], PULL_REQUEST_ID);
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

    fn resolve(&mut self, input: &Value, resolved: bool) -> Result<Value, String> {
        self.thread(&input["threadId"])?.resolved = resolved;
        Ok(json!({"x": {"clientMutationId": null}}))
    }

    /// A reply joins the viewer's pending review, which GitHub starts when there is none.
    fn reply(&mut self, input: &Value) -> Result<Value, String> {
        self.made += 1;
        let comment_id = format!("PRRC_{}", self.made);
        let review_id = self
            .pending_review
            .clone()
            .unwrap_or_else(|| format!("PRR_{}", self.made));
        self.thread(&input["pullRequestReviewThreadId"])?
            .comments
            .push(Comment {
                id: comment_id.clone(),
                author: VIEWER.to_owned(),
                body: input["body"].as_str().unwrap().to_owned(),
            });
        self.pending_review = Some(review_id.clone());
        Ok(json!({"addPullRequestReviewThreadReply": {"comment": {
            "id": comment_id,
            "pullRequestReview": {"id": review_id, "state": "PENDING"},
        }}}))
    }

    fn submit(&mut self, input: &Value) -> Result<Value, String> {
        let review_id = input["pullRequestReviewId"].as_str().unwrap();
        if self.pending_review.as_deref() != Some(review_id) {
            return Err(format!(
                "Could not resolve to a PullRequestReview with the id of '{review_id}'."
            ));
        }
        self.pending_review = None;
        Ok(json!({"submitPullRequestReview": {"clientMutationId": null}}))
    }

    fn pending_reviews(&self) -> Value {
        let nodes: Vec<Value> = self
            .pending_review
            .iter()
            .map(|id| json!({"id": id, "viewerDidAuthor": true}))
            .collect();
        json!({"repository": {"pullRequest": {"reviews": {"nodes": nodes}}}})
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
}

impl Thread {
    fn as_github_node(&self) -> Value {
        let comments: Vec<Value> = self
            .comments
            .iter()
            .map(|comment| {
                json!({
                    "id": comment.id,
                    "url": format!("https://github.com/acme/shop/pull/7#discussion_r{}", comment.id),
                    "body": comment.body,
                    "viewerDidAuthor": comment.author == VIEWER,
                    "reactionGroups": [],
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
