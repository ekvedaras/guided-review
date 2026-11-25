use guided_review::pull_request::Sha;
use guided_review::viewer::{
    CodeHost, CommentPlace, HostError, LineLocation, ReviewOutcome, SendMode, Side, Stance,
    Standing, ThreadId,
};

use crate::host_contract::{
    HostBuilder, PullOnHost, Reach, SeedComment, SeedThread, pr, reviewers_on_host,
};
use crate::support::{file_path, line};

fn place() -> CommentPlace {
    let at = LineLocation {
        path: file_path("cart.rs"),
        side: Side::Head,
        line: line(5),
    };
    CommentPlace::Lines {
        start: at.clone(),
        end: at,
    }
}

fn has_pending(host: &dyn CodeHost) -> bool {
    host.has_pending_review(&pr()).unwrap()
}

pub fn no_pending_review_until_a_thread_is_added_and_none_once_it_is_sent(host: HostBuilder) {
    let host = host(PullOnHost::default());

    let at_first = has_pending(host.as_ref());
    host.add_review_thread(&pr(), &place(), "why a Vec?")
        .unwrap();
    let with_a_thread = has_pending(host.as_ref());
    host.add_review_thread(&pr(), &place(), "and here?")
        .unwrap();
    host.submit_review(&pr(), "two questions").unwrap();
    let once_sent = has_pending(host.as_ref());

    assert_eq!((at_first, with_a_thread, once_sent), (false, true, false));
}

pub fn a_pending_review_started_on_the_web_counts(host: HostBuilder) {
    let host = host(PullOnHost {
        web_pending_review: true,
        ..PullOnHost::default()
    });

    assert!(has_pending(host.as_ref()));
}

pub fn whether_the_viewer_wrote_the_pr_reads_as_the_host_has_it(host: HostBuilder) {
    let theirs = host(PullOnHost::default());
    let mine = host(PullOnHost {
        author: crate::host_contract::VIEWER,
        ..PullOnHost::default()
    });

    assert!(!theirs.viewer_authored(&pr()).unwrap());
    assert!(mine.viewer_authored(&pr()).unwrap());
}

pub fn a_comment_now_leaves_no_pending_review(host: HostBuilder) {
    let host = host(PullOnHost::default());

    host.comment_now(&pr(), &place(), "typo").unwrap();

    assert!(!has_pending(host.as_ref()));
}

pub fn a_comment_now_past_a_pending_review_is_refused_and_sends_nothing(host: HostBuilder) {
    let host = host(PullOnHost {
        web_pending_review: true,
        ..PullOnHost::default()
    });

    let error = host.comment_now(&pr(), &place(), "typo").unwrap_err();

    assert!(matches!(error, HostError::PendingReviewExists), "{error:?}");
    assert!(has_pending(host.as_ref()));
    assert_eq!(host.reviews(&pr()).unwrap().viewer_last, None);
}

type Send = fn(&dyn CodeHost, &str) -> Result<(), HostError>;

pub fn each_verdict_reads_back_as_the_viewers_last_review(host: HostBuilder) {
    let verdicts: [(Send, ReviewOutcome); 3] = [
        (
            |host, body| host.submit_review(&pr(), body),
            ReviewOutcome::Commented,
        ),
        (
            |host, body| host.approve(&pr(), body),
            ReviewOutcome::Approved,
        ),
        (
            |host, body| {
                host.change_requests()
                    .expect("the host takes requests for changes")
                    .request_changes(&pr(), body)
            },
            ReviewOutcome::ChangesRequested,
        ),
    ];
    for (send, outcome) in verdicts {
        for pending in [false, true] {
            let host = host(PullOnHost {
                head: "cart-head",
                web_pending_review: pending,
                ..PullOnHost::default()
            });

            send(host.as_ref(), "looks fine").unwrap();

            let last = host.reviews(&pr()).unwrap().viewer_last.unwrap();
            assert_eq!(
                (last.outcome, last.body.as_str(), last.commit),
                (outcome, "looks fine", Sha::new("cart-head")),
                "{outcome:?}, pending review {pending}"
            );
            assert!(!has_pending(host.as_ref()), "{outcome:?}");
        }
    }
}

/// The host posts a comment or reply now as a review of its own, with no body.
pub fn a_comment_or_reply_posted_now_is_the_viewers_last_review(host: HostBuilder) {
    let posts: [(&str, Send); 2] = [
        ("comment", |host, body| {
            host.comment_now(&pr(), &place(), body)
        }),
        ("reply", |host, body| {
            host.reply_to_thread(&pr(), &ThreadId::new("T1"), body, SendMode::CommentNow)
                .map(|_| ())
        }),
    ];
    for (what, post) in posts {
        let host = host(PullOnHost {
            head: "cart-head",
            threads: vec![SeedThread {
                id: "T1",
                path: "cart.rs",
                line: 5,
                resolved: false,
                comments: vec![SeedComment {
                    id: "C1",
                    author: "alice",
                    body: "why a Vec?",
                }],
            }],
            ..PullOnHost::default()
        });

        post(host.as_ref(), "typo").unwrap();

        let last = host.reviews(&pr()).unwrap().viewer_last;
        assert_eq!(
            last.map(|last| (last.outcome, last.body, last.commit)),
            Some((
                ReviewOutcome::Commented,
                String::new(),
                Sha::new("cart-head")
            )),
            "{what}"
        );
    }
}

/// A review the viewer sends stands as theirs beside the others', in place of being asked for one.
pub fn a_sent_review_stands_as_the_viewers(host: HostBuilder) {
    let pull = PullOnHost {
        head: "cart-head",
        reviews: vec![("bob", ReviewOutcome::Approved)],
        asks_viewer_to_review: true,
        ..PullOnHost::default()
    };
    let others = |standings: &[Standing]| -> Vec<Standing> {
        standings
            .iter()
            .filter(|standing| !standing.is_viewer())
            .cloned()
            .collect()
    };
    let host = host(pull.clone());
    let before = host.reviews(&pr()).unwrap().standings;

    host.approve(&pr(), "ship it").unwrap();

    let after = host.reviews(&pr()).unwrap().standings;
    let viewers: Vec<&Stance> = after
        .iter()
        .filter(|standing| standing.is_viewer())
        .map(|standing| &standing.stance)
        .collect();
    assert_eq!(
        viewers,
        [&Stance::Reviewed {
            outcome: ReviewOutcome::Approved,
            commit: Sha::new("cart-head"),
        }]
    );
    assert_eq!(others(&after), others(&before));
    assert_eq!(
        others(&before),
        others(&reviewers_on_host(&pull).standings())
    );
}

pub fn someone_elses_review_stands_as_theirs(host: HostBuilder) {
    let pull = PullOnHost {
        reviews: vec![
            ("bob", ReviewOutcome::Approved),
            ("carol", ReviewOutcome::Commented),
        ],
        asks_viewer_to_review: true,
        ..PullOnHost::default()
    };
    let standings = reviewers_on_host(&pull).standings();

    let reviews = host(pull).reviews(&pr()).unwrap();

    assert_eq!(reviews.viewer_last, None);
    assert_eq!(reviews.standings, standings);
}

pub fn review_writes_without_a_token_are_refused(host: HostBuilder) {
    let host = host(PullOnHost {
        reach: Reach::NoToken,
        ..PullOnHost::default()
    });

    let errors = [
        host.has_pending_review(&pr()).map(|_| ()),
        host.reviews(&pr()).map(|_| ()),
        host.viewer_authored(&pr()).map(|_| ()),
        host.add_review_thread(&pr(), &place(), "x"),
        host.comment_now(&pr(), &place(), "x"),
        host.submit_review(&pr(), "x"),
        host.approve(&pr(), "x"),
    ];

    for error in errors {
        assert!(matches!(error, Err(HostError::NoAccess(_))), "{error:?}");
    }
}

pub fn review_writes_of_a_host_not_answering_fail_as_an_outage(host: HostBuilder) {
    let host = host(PullOnHost {
        reach: Reach::ServerError,
        ..PullOnHost::default()
    });

    let errors = [
        host.has_pending_review(&pr()).map(|_| ()),
        host.reviews(&pr()).map(|_| ()),
        host.viewer_authored(&pr()).map(|_| ()),
        host.add_review_thread(&pr(), &place(), "x"),
        host.comment_now(&pr(), &place(), "x"),
        host.submit_review(&pr(), "x"),
        host.approve(&pr(), "x"),
    ];

    for error in errors {
        assert!(
            matches!(error, Err(HostError::NotAnswering(..))),
            "{error:?}"
        );
    }
}
