//! A builder per host, each making the host hold the PR a case starts from.

use guided_review::github::GitHub;
use guided_review::viewer::{CodeHost, ReviewThread, Side, ThreadComment, ThreadId};

use super::{PullOnHost, Reach, SeedThread, github_api};
use crate::support::fake_ports::{FakeHost, HostFault, ReadFailure, SentLog};
use crate::support::{file_path, line};

pub fn fake_host(pull: PullOnHost) -> Box<dyn CodeHost> {
    let mut host = pull.files.iter().fold(
        FakeHost::with_pull("aaa", "bbb", &SentLog::default()),
        |host, (path, state)| host.viewing(path, *state),
    );
    host.threads = pull.threads.iter().map(review_thread).collect();
    match pull.reach {
        Reach::Answers => {}
        Reach::NoToken => host.inject(HostFault::NoToken),
        Reach::ServerError => {
            host.inject(HostFault::ReadsFailWith(ReadFailure::Status(503)));
            host.inject(HostFault::ConnectionLost);
        }
    }
    Box::new(host)
}

pub fn github(pull: PullOnHost) -> Box<dyn CodeHost> {
    let api = github_api::serve(&pull);
    match pull.reach {
        Reach::NoToken => Box::new(GitHub::without_token_at(&api)),
        Reach::Answers | Reach::ServerError => Box::new(GitHub::at(&api)),
    }
}

fn review_thread(seed: &SeedThread) -> ReviewThread {
    ReviewThread {
        id: ThreadId::new(seed.id),
        is_resolved: seed.resolved,
        is_outdated: false,
        path: file_path(seed.path),
        line: Some(line(seed.line)),
        start_line: None,
        side: Side::Head,
        is_file_comment: false,
        original_line: Some(line(seed.line)),
        original_start_line: None,
        original_commit: Some("bbb".to_owned()),
        comments: seed
            .comments
            .iter()
            .map(|comment| ThreadComment {
                id: guided_review::viewer::CommentId::new(comment.id),
                author: comment.author.to_owned(),
                avatar_url: None,
                is_bot: false,
                by_agent: None,
                by_viewer: false,
                body: comment.body.to_owned(),
                url: format!(
                    "https://github.com/acme/shop/pull/7#discussion_r{}",
                    comment.id
                ),
                reactions: Vec::new(),
            })
            .collect(),
    }
}
