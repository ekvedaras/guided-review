use guided_review::viewer::{CodeHost, HostError, Resolution, ReviewThread, SendMode, ThreadId};

use super::{HostBuilder, PullOnHost, Reach, SeedComment, SeedThread, pr};
use crate::support::line;

fn three_threads() -> Vec<SeedThread> {
    vec![
        SeedThread {
            id: "T1",
            path: "cart.rs",
            line: 12,
            resolved: false,
            comments: vec![
                SeedComment {
                    id: "C1",
                    author: "alice",
                    body: "why a Vec?",
                },
                SeedComment {
                    id: "C2",
                    author: "bob",
                    body: "order matters",
                },
            ],
        },
        SeedThread {
            id: "T2",
            path: "cart.rs",
            line: 40,
            resolved: true,
            comments: vec![SeedComment {
                id: "C3",
                author: "alice",
                body: "typo",
            }],
        },
        SeedThread {
            id: "T3",
            path: "price.rs",
            line: 3,
            resolved: false,
            comments: vec![SeedComment {
                id: "C4",
                author: "carol",
                body: "rounding?",
            }],
        },
    ]
}

fn threads_of(host: &dyn CodeHost) -> Vec<ReviewThread> {
    host.review_threads(&pr()).unwrap()
}

fn thread<'a>(threads: &'a [ReviewThread], id: &str) -> &'a ReviewThread {
    threads
        .iter()
        .find(|thread| thread.id == ThreadId::new(id))
        .unwrap_or_else(|| panic!("no thread {id} in {threads:?}"))
}

/// What a caller reads of a thread: where it is, whether it is resolved, and who said what.
type ThreadSummary = (
    String,
    String,
    Option<usize>,
    bool,
    Vec<(String, String, String, bool)>,
);

fn summary(thread: &ReviewThread) -> ThreadSummary {
    (
        thread.id.as_str().to_owned(),
        thread.path.to_string(),
        thread.line.map(usize::from),
        thread.is_resolved,
        thread
            .comments
            .iter()
            .map(|comment| {
                (
                    comment.id.as_str().to_owned(),
                    comment.author.clone(),
                    comment.body.clone(),
                    comment.by_viewer,
                )
            })
            .collect(),
    )
}

pub fn review_threads_list_each_thread_with_its_comments(host: HostBuilder) {
    let seeds = three_threads();
    let host = host(PullOnHost {
        threads: seeds.clone(),
        ..PullOnHost::default()
    });

    let read: Vec<ThreadSummary> = threads_of(host.as_ref()).iter().map(summary).collect();

    let seeded: Vec<ThreadSummary> = seeds
        .iter()
        .map(|seed| {
            (
                seed.id.to_owned(),
                seed.path.to_owned(),
                Some(usize::from(line(seed.line))),
                seed.resolved,
                seed.comments
                    .iter()
                    .map(|comment| {
                        (
                            comment.id.to_owned(),
                            comment.author.to_owned(),
                            comment.body.to_owned(),
                            false,
                        )
                    })
                    .collect(),
            )
        })
        .collect();
    assert_eq!(read, seeded);
}

pub fn a_reply_reads_back_as_the_last_comment_of_its_thread(host: HostBuilder) {
    for mode in [SendMode::AddToReview, SendMode::CommentNow] {
        let host = host(PullOnHost {
            threads: three_threads(),
            ..PullOnHost::default()
        });
        let before: Vec<ThreadSummary> = threads_of(host.as_ref()).iter().map(summary).collect();

        let reply = host
            .reply_to_thread(&pr(), &ThreadId::new("T1"), "a set would lose it", mode)
            .unwrap();

        let after = threads_of(host.as_ref());
        let replied = thread(&after, "T1");
        let last = replied.comments.last().unwrap();
        assert_eq!(
            (&last.id, last.body.as_str(), last.by_viewer),
            (&reply, "a set would lose it", true),
            "{mode:?}"
        );
        assert_eq!(replied.comments.len(), 3, "{mode:?}");
        let others = |threads: &[ThreadSummary]| -> Vec<ThreadSummary> {
            threads
                .iter()
                .filter(|thread| thread.0 != "T1")
                .cloned()
                .collect()
        };
        assert_eq!(
            others(&after.iter().map(summary).collect::<Vec<_>>()),
            others(&before),
            "{mode:?}"
        );
    }
}

pub fn a_resolved_thread_reads_back_resolved_and_an_unresolved_one_open(host: HostBuilder) {
    let host = host(PullOnHost {
        threads: three_threads(),
        ..PullOnHost::default()
    });

    host.set_thread_resolved(&ThreadId::new("T1"), Resolution::Resolved)
        .unwrap();
    host.set_thread_resolved(&ThreadId::new("T2"), Resolution::Unresolved)
        .unwrap();

    let threads = threads_of(host.as_ref());
    assert_eq!(
        ["T1", "T2", "T3"].map(|id| thread(&threads, id).is_resolved),
        [true, false, false]
    );
}

pub fn review_threads_without_a_token_are_refused(host: HostBuilder) {
    let host = host(PullOnHost {
        threads: three_threads(),
        reach: Reach::NoToken,
        ..PullOnHost::default()
    });

    let read = host.review_threads(&pr()).unwrap_err();
    let reply = host
        .reply_to_thread(&pr(), &ThreadId::new("T1"), "x", SendMode::AddToReview)
        .unwrap_err();
    let resolve = host
        .set_thread_resolved(&ThreadId::new("T1"), Resolution::Resolved)
        .unwrap_err();

    for error in [read, reply, resolve] {
        assert!(matches!(error, HostError::NoAccess(_)), "{error:?}");
    }
}

pub fn review_threads_of_a_host_not_answering_fail_as_an_outage(host: HostBuilder) {
    let host = host(PullOnHost {
        threads: three_threads(),
        reach: Reach::ServerError,
        ..PullOnHost::default()
    });

    let read = host.review_threads(&pr()).unwrap_err();
    let reply = host.reply_to_thread(&pr(), &ThreadId::new("T1"), "x", SendMode::AddToReview);
    let resolve = host.set_thread_resolved(&ThreadId::new("T1"), Resolution::Resolved);

    assert!(matches!(read, HostError::NotAnswering(_)), "{read:?}");
    assert!(
        reply.is_err(),
        "a reply to a host not answering went through"
    );
    assert!(
        resolve.is_err(),
        "a resolve on a host not answering went through"
    );
}
