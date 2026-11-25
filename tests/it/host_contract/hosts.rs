//! A builder per host, each making the host hold the PR a case starts from.

use std::sync::Arc;

use guided_review::code_source::{BlobInfo, CodeSource, FileDiff, parse_unified_diff};
use guided_review::file_path::FilePath;
use guided_review::github::GitHub;
use guided_review::issue::{IssueLink, IssueUrl};
use guided_review::pull_request::Sha;
use guided_review::review_subject::BranchName;
use guided_review::viewer::{
    CodeHost, HeadBranch, IssueDetails, MergeRoute, PrStatus, PullRequestIssues, PullRequestOrigin,
    RelatedIssue, RelevantPullRequest, RelevantPullRequests, ReviewThread, Side, ThreadComment,
    ThreadId, WhyListed,
};

use super::{
    PullOnHost, Reach, SeedCode, SeedIssue, SeedThread, UPDATED_AT, VIEWER, author_of, blob_id,
    github_api, pr, reviewers_on_host,
};
use crate::support::fake_ports::{FakeHost, HostFault, ReadFailure, SentLog};
use crate::support::{file_path, line};

pub fn fake_host(pull: PullOnHost) -> Box<dyn CodeHost> {
    let pr_text = pr().to_string();
    let is_open = matches!(pull.status, PrStatus::Open(_));
    let mut host = pull
        .files
        .iter()
        .fold(
            FakeHost::with_pull(pull.base_branch_tip, pull.head, &SentLog::default()),
            |host, (path, state)| host.viewing(path, *state),
        )
        .with_title(pull.title)
        .with_description(pull.description)
        .with_status(pull.status.clone())
        .with_merging(pull.merging)
        .with_origin(origin_of(&pull))
        .with_issues(PullRequestIssues {
            linked: pull.closing_issues.iter().map(related_issue).collect(),
            head_branch: Some(branch(pull.head_branch)),
        })
        .with_standings(reviewers_on_host(&pull).standings())
        .with_relevant_pull_requests(relevant_of(&pull))
        .serving_code(Arc::new(SeededCode::of(&pull)));
    for issue in &pull.closing_issues {
        host = host.with_issue_details(issue_url(issue), issue_details(issue));
    }
    if pull.fork_owner.is_none() {
        host = host.with_head_branch(&pr_text, pull.head_branch);
        if is_open {
            host = host.with_open_pull_from(pull.head_branch, &pr_text, pull.title);
        }
    }
    if pull.author == VIEWER {
        host = host.with_viewer_as_pull_author();
    }
    if pull.web_pending_review {
        host = host.with_web_pending_review();
    }
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

fn branch(name: &str) -> BranchName {
    BranchName::new(name).unwrap()
}

fn origin_of(pull: &PullOnHost) -> PullRequestOrigin {
    PullRequestOrigin {
        author: Some(author_of(pull)),
        route: MergeRoute {
            base: branch(pull.base_branch),
            head: pull.fork_owner.map_or_else(
                || HeadBranch::of_repo(branch(pull.head_branch)),
                |owner| HeadBranch::of_fork(owner, branch(pull.head_branch)),
            ),
        },
    }
}

fn relevant_of(pull: &PullOnHost) -> RelevantPullRequests {
    let PrStatus::Open(open) = &pull.status else {
        return RelevantPullRequests::default();
    };
    let why_listed = if pull.author == VIEWER {
        WhyListed::Mine
    } else if pull.asks_viewer_to_review {
        WhyListed::ReviewRequested
    } else {
        WhyListed::Involved
    };
    RelevantPullRequests {
        pull_requests: vec![RelevantPullRequest {
            url: pr(),
            title: pull.title.to_owned(),
            author: author_of(pull),
            is_draft: open.readiness == guided_review::viewer::Readiness::Draft,
            updated_at: UPDATED_AT.to_owned(),
            why_listed,
        }],
        teams_left_out: Vec::new(),
    }
}

fn issue_url(issue: &SeedIssue) -> IssueUrl {
    IssueUrl::new("acme/shop".parse().unwrap(), issue.number)
}

fn related_issue(issue: &SeedIssue) -> RelatedIssue {
    RelatedIssue {
        link: IssueLink::GitHub(issue_url(issue)),
        title: Some(issue.title.into()),
    }
}

fn issue_details(issue: &SeedIssue) -> IssueDetails {
    IssueDetails {
        title: issue.title.into(),
        state: issue.state,
        author: Some(issue.author.into()),
        labels: issue.labels.iter().map(|label| (*label).into()).collect(),
        assignees: issue
            .assignees
            .iter()
            .map(|login| (*login).into())
            .collect(),
        body: IssueDetails::markdown_of(issue.body),
    }
}

/// The repo's code as the seed holds it, for `FakeHost` to serve.
struct SeededCode {
    code: SeedCode,
    tips: (Sha, Sha),
}

impl SeededCode {
    fn of(pull: &PullOnHost) -> Self {
        Self {
            code: pull.code.clone(),
            tips: (Sha::new(pull.base_branch_tip), Sha::new(pull.head)),
        }
    }

    fn text_at(&self, sha: &Sha, path: &FilePath) -> anyhow::Result<&'static str> {
        self.code
            .files
            .iter()
            .find(|(at, known, _)| *at == sha.as_str() && *known == path.as_str())
            .map(|(_, _, text)| *text)
            .ok_or_else(|| anyhow::anyhow!("no {path} at {sha}"))
    }

    fn are_the_tips(&self, a: &Sha, b: &Sha) -> anyhow::Result<()> {
        anyhow::ensure!(
            (a, b) == (&self.tips.0, &self.tips.1),
            "the seed knows only the diff of its tips"
        );
        Ok(())
    }
}

impl CodeSource for SeededCode {
    fn file_at(&self, sha: &Sha, path: &FilePath) -> anyhow::Result<String> {
        Ok(self.text_at(sha, path)?.to_owned())
    }

    fn blob_at(&self, sha: &Sha, path: &FilePath) -> anyhow::Result<Arc<[u8]>> {
        Ok(self.text_at(sha, path)?.as_bytes().into())
    }

    fn blob_info_at(&self, sha: &Sha, path: &FilePath) -> anyhow::Result<BlobInfo> {
        let text = self.text_at(sha, path)?;
        Ok(BlobInfo {
            id: blob_id(text.as_bytes()),
            size: u64::try_from(text.len())?,
        })
    }

    fn diff(&self, base: &Sha, head: &Sha) -> anyhow::Result<Vec<FileDiff>> {
        self.are_the_tips(base, head)?;
        parse_unified_diff(self.code.diff)
    }

    fn merge_base(&self, a: &Sha, b: &Sha) -> anyhow::Result<Sha> {
        self.are_the_tips(a, b)?;
        Ok(Sha::new(self.code.merge_base))
    }

    fn working_tree_is_at(&self, _head: &Sha) -> bool {
        false
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
