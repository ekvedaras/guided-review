use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::thread;

use guided_review::github::GitHub;
use guided_review::pull_request::PullRequestUrl;
use guided_review::viewer::{
    CommentPlace, LineLocation, PendingReviewExists, Side, ThreadNotCreated,
};

/// Answers each POST to /graphql with the next scripted body and remembers what was asked.
fn serve_graphql(answers: Vec<String>) -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let asked = Arc::new(Mutex::new(Vec::new()));
    let log = asked.clone();
    thread::spawn(move || {
        for answer in answers {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut request_line = String::new();
            reader.read_line(&mut request_line).unwrap();
            assert!(request_line.starts_with("POST /graphql "), "{request_line}");
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
            log.lock().unwrap().push(String::from_utf8(body).unwrap());
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{answer}",
                answer.len()
            )
            .unwrap();
        }
    });
    (url, asked)
}

fn page(has_next: bool, cursor: &str, nodes: &str) -> String {
    format!(
        r#"{{"data":{{"repository":{{"pullRequest":{{"reviewThreads":{{"pageInfo":{{"hasNextPage":{has_next},"endCursor":"{cursor}"}},"nodes":[{nodes}]}}}}}}}}}}"#
    )
}

const LINE_THREAD: &str = r#"{"id":"T1","isResolved":true,"isOutdated":false,"path":"a.php","line":227,"startLine":167,"originalLine":179,"originalStartLine":167,"diffSide":"LEFT","subjectType":"LINE","comments":{"nodes":[
  {"url":"https://github.com/acme/shop/pull/7#r1","body":"named arguments","originalCommit":{"oid":"9d6f"},"author":{"login":"alice","__typename":"User"}},
  {"url":"https://github.com/acme/shop/pull/7#r2","body":"done","originalCommit":{"oid":"9d6f"},"author":{"login":"review-bot","__typename":"Bot"}}]}}"#;

const OUTDATED_FILE_THREAD: &str = r#"{"id":"T2","isResolved":false,"isOutdated":true,"path":"b.php","line":null,"startLine":null,"originalLine":435,"originalStartLine":null,"diffSide":"RIGHT","subjectType":"FILE","comments":{"nodes":[
  {"url":"https://github.com/acme/shop/pull/7#r3","body":"gone","originalCommit":null,"author":null}]}}"#;

#[test]
fn review_threads_are_read_over_graphql_page_by_page() {
    let (url, asked) = serve_graphql(vec![
        page(true, "CURSOR1", LINE_THREAD),
        page(false, "CURSOR2", OUTDATED_FILE_THREAD),
    ]);
    let pr: PullRequestUrl = "https://github.com/acme/shop/pull/7".parse().unwrap();

    let threads = GitHub::at(&url).review_threads(&pr).unwrap();

    let [line, file] = threads.as_slice() else {
        panic!("expected two threads, got {threads:?}");
    };
    assert_eq!(line.id, "T1");
    assert!(line.is_resolved && !line.is_outdated && !line.is_file_comment);
    assert_eq!(
        (line.path.as_str(), line.line, line.start_line, line.side),
        ("a.php", Some(227), Some(167), Side::Base)
    );
    assert_eq!(line.original_commit.as_deref(), Some("9d6f"));
    assert_eq!(
        line.comments
            .iter()
            .map(|comment| (comment.author.as_str(), comment.is_bot))
            .collect::<Vec<_>>(),
        [("alice", false), ("review-bot", true)]
    );
    assert_eq!(
        line.comments[0].url,
        "https://github.com/acme/shop/pull/7#r1"
    );
    assert!(file.is_outdated && file.is_file_comment);
    assert_eq!((file.line, file.original_line), (None, Some(435)));
    assert_eq!(file.comments[0].author, "ghost");

    let asked: Vec<serde_json::Value> = asked
        .lock()
        .unwrap()
        .iter()
        .map(|body| serde_json::from_str(body).unwrap())
        .collect();
    assert_eq!(asked[0]["variables"]["after"], serde_json::Value::Null);
    assert_eq!(asked[1]["variables"]["after"], "CURSOR1");
    assert_eq!(asked[0]["variables"]["owner"], "acme");
    assert_eq!(asked[0]["variables"]["number"], 7);
}

fn location(side: Side, line: usize) -> LineLocation {
    LineLocation {
        path: "a.php".into(),
        side,
        line,
    }
}

const PULL_ID: &str = r#"{"data":{"repository":{"pullRequest":{"id":"PR_1"}}}}"#;
const THREAD_ADDED: &str = r#"{"data":{"addPullRequestReviewThread":{"thread":{"id":"T9","comments":{"nodes":[{"pullRequestReview":{"id":"REVIEW_1"}}]}}}}}"#;

fn asked_as_json(asked: &Arc<Mutex<Vec<String>>>) -> Vec<serde_json::Value> {
    asked
        .lock()
        .unwrap()
        .iter()
        .map(|body| serde_json::from_str(body).unwrap())
        .collect()
}

fn pr() -> PullRequestUrl {
    "https://github.com/acme/shop/pull/7".parse().unwrap()
}

#[test]
fn a_range_comment_is_added_as_a_thread_with_both_sides() {
    let (url, asked) = serve_graphql(vec![PULL_ID.into(), THREAD_ADDED.into()]);
    let place = CommentPlace {
        start: location(Side::Base, 10),
        end: location(Side::Head, 12),
    };

    let review = GitHub::at(&url)
        .add_review_thread(&pr(), &place, "why?")
        .unwrap();

    assert_eq!(review, "REVIEW_1");
    let input = &asked_as_json(&asked)[1]["variables"]["input"];
    assert_eq!(
        *input,
        serde_json::json!({
            "pullRequestId": "PR_1", "path": "a.php", "body": "why?", "line": 12,
            "side": "RIGHT", "startLine": 10, "startSide": "LEFT", "subjectType": "LINE",
        })
    );
}

#[test]
fn a_single_line_comment_sends_no_start() {
    let (url, asked) = serve_graphql(vec![PULL_ID.into(), THREAD_ADDED.into()]);
    let place = CommentPlace {
        start: location(Side::Head, 5),
        end: location(Side::Head, 5),
    };

    GitHub::at(&url)
        .add_review_thread(&pr(), &place, "x")
        .unwrap();

    let input = &asked_as_json(&asked)[1]["variables"]["input"];
    assert!(input.get("startLine").is_none() && input.get("startSide").is_none());
}

#[test]
fn a_null_thread_without_errors_is_a_failure() {
    let (url, _) = serve_graphql(vec![
        PULL_ID.into(),
        r#"{"data":{"addPullRequestReviewThread":{"thread":null}}}"#.into(),
    ]);
    let place = CommentPlace {
        start: location(Side::Head, 5),
        end: location(Side::Head, 5),
    };

    let error = GitHub::at(&url)
        .add_review_thread(&pr(), &place, "x")
        .unwrap_err();

    assert!(error.downcast_ref::<ThreadNotCreated>().is_some());
}

#[test]
fn github_saying_one_pending_review_per_user_is_told_apart() {
    let (url, _) = serve_graphql(vec![
        PULL_ID.into(),
        r#"{"errors":[{"message":"User can only have one pending review per pull request"}],"data":null}"#.into(),
    ]);
    let place = CommentPlace {
        start: location(Side::Head, 5),
        end: location(Side::Head, 5),
    };

    let error = GitHub::at(&url)
        .add_review_thread(&pr(), &place, "x")
        .unwrap_err();

    assert!(error.downcast_ref::<PendingReviewExists>().is_some());
}

#[test]
fn comment_now_submits_the_review_the_thread_went_into() {
    let (url, asked) = serve_graphql(vec![
        PULL_ID.into(),
        THREAD_ADDED.into(),
        r#"{"data":{"submitPullRequestReview":{"clientMutationId":null}}}"#.into(),
    ]);
    let place = CommentPlace {
        start: location(Side::Head, 5),
        end: location(Side::Head, 5),
    };

    GitHub::at(&url).comment_now(&pr(), &place, "x").unwrap();

    let input = &asked_as_json(&asked)[2]["variables"]["input"];
    assert_eq!(
        *input,
        serde_json::json!({"pullRequestReviewId": "REVIEW_1", "event": "COMMENT"})
    );
}

#[test]
fn only_a_pending_review_the_viewer_wrote_counts() {
    let answer = |viewer: bool| {
        format!(
            r#"{{"data":{{"repository":{{"pullRequest":{{"reviews":{{"nodes":[{{"viewerDidAuthor":{viewer}}}]}}}}}}}}}}"#
        )
    };
    let (url, _) = serve_graphql(vec![
        answer(true),
        answer(false),
        r#"{"data":{"repository":{"pullRequest":{"reviews":{"nodes":[]}}}}}"#.into(),
    ]);
    let github = GitHub::at(&url);

    assert!(github.has_pending_review(&pr()).unwrap());
    assert!(!github.has_pending_review(&pr()).unwrap());
    assert!(!github.has_pending_review(&pr()).unwrap());
}
