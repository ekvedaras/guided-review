use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::thread;

use crate::support::file_path;
use guided_review::github::GitHub;
use guided_review::pull_request::PullRequestUrl;
use guided_review::viewer::{
    CommentPlace, LineLocation, PendingReviewExists, Reaction, ReactionCount, ReviewThreadsPort,
    ReviewVerdict, ReviewWritesPort, SendMode, Side, ThreadNotCreated,
};

/// Answers each POST to /graphql with the next scripted body and remembers what was asked.
fn serve_graphql(answers: Vec<String>) -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let asked = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&asked);
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
  {"id":"C1","url":"https://github.com/acme/shop/pull/7#r1","body":"named arguments","reactionGroups":[{"content":"HEART","viewerHasReacted":true,"reactors":{"totalCount":2}},{"content":"EYES","viewerHasReacted":false,"reactors":{"totalCount":0}}],"originalCommit":{"oid":"9d6f"},"author":{"login":"alice","__typename":"User"}},
  {"id":"C2","url":"https://github.com/acme/shop/pull/7#r2","body":"done","originalCommit":{"oid":"9d6f"},"author":{"login":"review-bot","__typename":"Bot"}}]}}"#;

const OUTDATED_FILE_THREAD: &str = r#"{"id":"T2","isResolved":false,"isOutdated":true,"path":"b.php","line":null,"startLine":null,"originalLine":435,"originalStartLine":null,"diffSide":"RIGHT","subjectType":"FILE","comments":{"nodes":[
  {"id":"C3","url":"https://github.com/acme/shop/pull/7#r3","body":"gone","originalCommit":null,"author":null}]}}"#;

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
    assert_eq!(line.comments[0].id, "C1");
    assert_eq!(
        line.comments[0].reactions,
        [ReactionCount {
            reaction: Reaction::Heart,
            count: 2,
            viewer_has_reacted: true
        }]
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
        path: file_path("a.php"),
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
        .add_thread_to_pending_review(&pr(), &place, "why?")
        .unwrap();

    assert_eq!(review.as_deref(), Some("REVIEW_1"));
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

const THREAD_ADDED_WITHOUT_REVIEW: &str = r#"{"data":{"addPullRequestReviewThread":{"thread":{"id":"T9","comments":{"nodes":[{"pullRequestReview":null}]}}}}}"#;

#[test]
fn comment_now_without_a_review_id_from_github_sends_the_viewers_pending_review() {
    let (url, asked) = serve_graphql(vec![
        PULL_ID.into(),
        THREAD_ADDED_WITHOUT_REVIEW.into(),
        PENDING.into(),
        OK.into(),
    ]);
    let place = CommentPlace {
        start: location(Side::Head, 5),
        end: location(Side::Head, 5),
    };

    GitHub::at(&url).comment_now(&pr(), &place, "x").unwrap();

    assert_eq!(
        asked_as_json(&asked)[3]["variables"]["input"],
        serde_json::json!({"pullRequestReviewId": "REVIEW_7", "event": "COMMENT"})
    );
}

#[test]
fn comment_now_whose_review_cannot_be_found_says_where_the_comment_is_and_sends_nothing_more() {
    let (url, asked) = serve_graphql(vec![
        PULL_ID.into(),
        THREAD_ADDED_WITHOUT_REVIEW.into(),
        NO_PENDING.into(),
    ]);
    let place = CommentPlace {
        start: location(Side::Head, 5),
        end: location(Side::Head, 5),
    };

    let error = GitHub::at(&url)
        .comment_now(&pr(), &place, "x")
        .unwrap_err();

    assert!(
        format!("{error:#}").contains("look for it on GitHub"),
        "{error:#}"
    );
    assert_eq!(asked_as_json(&asked).len(), 3);
}

#[test]
fn a_comment_now_that_could_not_be_posted_or_taken_back_says_it_waits_in_the_pending_review() {
    let failed = r#"{"errors":[{"message":"something broke"}],"data":null}"#;
    let (url, _) = serve_graphql(vec![
        PULL_ID.into(),
        THREAD_ADDED.into(),
        failed.into(),
        failed.into(),
    ]);
    let place = CommentPlace {
        start: location(Side::Head, 5),
        end: location(Side::Head, 5),
    };

    let error = GitHub::at(&url)
        .comment_now(&pr(), &place, "x")
        .unwrap_err();

    assert!(
        format!("{error:#}").contains("pending review on GitHub"),
        "{error:#}"
    );
}

#[test]
fn a_reply_now_without_a_review_id_from_github_sends_the_viewers_pending_review() {
    let replied = r#"{"data":{"addPullRequestReviewThreadReply":{"comment":{"pullRequestReview":{"id":"","state":"PENDING"}}}}}"#;
    let (url, asked) = serve_graphql(vec![
        NO_PENDING.into(),
        replied.into(),
        PENDING.into(),
        OK.into(),
    ]);

    GitHub::at(&url)
        .reply_to_thread(&pr(), "T1", "because", SendMode::CommentNow)
        .unwrap();

    assert_eq!(
        asked_as_json(&asked)[3]["variables"]["input"]["pullRequestReviewId"],
        "REVIEW_7"
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

const OK: &str = r#"{"data":{"x":{"clientMutationId":null}}}"#;
const NO_PENDING: &str = r#"{"data":{"repository":{"pullRequest":{"reviews":{"nodes":[]}}}}}"#;
const PENDING: &str = r#"{"data":{"repository":{"pullRequest":{"reviews":{"nodes":[{"id":"REVIEW_7","viewerDidAuthor":true}]}}}}}"#;

#[test]
fn a_thread_is_resolved_and_unresolved_by_its_id() {
    let (url, asked) = serve_graphql(vec![OK.into(), OK.into()]);
    let github = GitHub::at(&url);

    github.set_thread_resolved("T1", true).unwrap();
    github.set_thread_resolved("T1", false).unwrap();

    let asked = asked_as_json(&asked);
    assert!(
        asked[0]["query"]
            .as_str()
            .unwrap()
            .contains("resolveReviewThread(input")
    );
    assert!(
        asked[1]["query"]
            .as_str()
            .unwrap()
            .contains("unresolveReviewThread(input")
    );
    assert_eq!(asked[1]["variables"]["input"]["threadId"], "T1");
}

#[test]
fn a_reaction_is_added_and_removed_with_githubs_content_name() {
    let (url, asked) = serve_graphql(vec![OK.into(), OK.into()]);
    let github = GitHub::at(&url);

    github.set_reaction("C1", Reaction::ThumbsUp, true).unwrap();
    github
        .set_reaction("C1", Reaction::ThumbsUp, false)
        .unwrap();

    let asked = asked_as_json(&asked);
    assert!(
        asked[0]["query"]
            .as_str()
            .unwrap()
            .contains("addReaction(input")
    );
    assert!(
        asked[1]["query"]
            .as_str()
            .unwrap()
            .contains("removeReaction(input")
    );
    assert_eq!(
        asked[0]["variables"]["input"],
        serde_json::json!({"subjectId": "C1", "content": "THUMBS_UP"})
    );
}

#[test]
fn the_pending_review_is_sent_with_the_verdict_and_body() {
    let (url, asked) = serve_graphql(vec![PENDING.into(), OK.into()]);

    GitHub::at(&url)
        .submit_review(&pr(), ReviewVerdict::RequestChanges, "fix it")
        .unwrap();

    let asked = asked_as_json(&asked);
    assert!(
        asked[1]["query"]
            .as_str()
            .unwrap()
            .contains("submitPullRequestReview(input")
    );
    assert_eq!(
        asked[1]["variables"]["input"],
        serde_json::json!({"pullRequestReviewId": "REVIEW_7", "event": "REQUEST_CHANGES", "body": "fix it"})
    );
}

#[test]
fn without_a_pending_review_a_new_review_carries_the_verdict() {
    let (url, asked) = serve_graphql(vec![NO_PENDING.into(), PULL_ID.into(), OK.into()]);

    GitHub::at(&url)
        .submit_review(&pr(), ReviewVerdict::Approve, "")
        .unwrap();

    let asked = asked_as_json(&asked);
    assert!(
        asked[2]["query"]
            .as_str()
            .unwrap()
            .contains("addPullRequestReview(input")
    );
    assert_eq!(
        asked[2]["variables"]["input"],
        serde_json::json!({"pullRequestId": "PR_1", "event": "APPROVE", "body": ""})
    );
}

#[test]
fn a_reply_now_sends_the_review_it_started() {
    let replied = r#"{"data":{"addPullRequestReviewThreadReply":{"comment":{"pullRequestReview":{"id":"REVIEW_8","state":"PENDING"}}}}}"#;
    let (url, asked) = serve_graphql(vec![NO_PENDING.into(), replied.into(), OK.into()]);

    GitHub::at(&url)
        .reply_to_thread(&pr(), "T1", "because", SendMode::CommentNow)
        .unwrap();

    let asked = asked_as_json(&asked);
    assert_eq!(
        asked[1]["variables"]["input"],
        serde_json::json!({"pullRequestReviewThreadId": "T1", "body": "because"})
    );
    assert_eq!(
        asked[2]["variables"]["input"]["pullRequestReviewId"],
        "REVIEW_8"
    );
    assert_eq!(asked[2]["variables"]["input"]["event"], "COMMENT");
}

#[test]
fn a_reply_into_the_pending_review_is_not_sent() {
    let replied = r#"{"data":{"addPullRequestReviewThreadReply":{"comment":{"pullRequestReview":{"id":"REVIEW_7","state":"PENDING"}}}}}"#;
    let (url, asked) = serve_graphql(vec![PENDING.into(), replied.into()]);

    GitHub::at(&url)
        .reply_to_thread(&pr(), "T1", "because", SendMode::AddToReview)
        .unwrap();

    let asked = asked_as_json(&asked);
    assert_eq!(asked.len(), 2);
    assert_eq!(
        asked[1]["variables"]["input"]["pullRequestReviewId"],
        "REVIEW_7"
    );
}

#[test]
fn the_pr_author_is_read_from_viewer_did_author() {
    let answer = r#"{"data":{"repository":{"pullRequest":{"viewerDidAuthor":true}}}}"#;
    let (url, _) = serve_graphql(vec![answer.into()]);

    assert!(GitHub::at(&url).viewer_authored(&pr()).unwrap());
}

#[test]
fn a_file_comment_is_a_thread_of_subject_type_file_without_lines() {
    let (url, asked) = serve_graphql(vec![PULL_ID.into(), THREAD_ADDED.into()]);

    GitHub::at(&url)
        .add_review_thread(
            &pr(),
            &CommentPlace::on_file(&file_path("logo.png")),
            "why red?",
        )
        .unwrap();

    let input = &asked_as_json(&asked)[1]["variables"]["input"];
    assert_eq!(
        *input,
        serde_json::json!({
            "pullRequestId": "PR_1", "path": "logo.png", "body": "why red?", "subjectType": "FILE",
        })
    );
}
