//! The adapter of each code host the app reads: the one place a `HostSite` becomes the adapter that
//! talks to it. A PR's link names its host, and a local change set's origin names the host its
//! branch's PR would be on.

use std::sync::Arc;

use crate::github::GitHub;
use crate::pull_request::{HostSite, HostedRepo, PullRequestUrl};
use crate::review_subject::{BranchName, LocalChangeSet, ReviewSubject};
use crate::viewer::{
    CodeHost, HostError, NoHost, PullOfBranch, PullsOfBranchPort, RelevantPullRequests,
    RelevantPullRequestsPort,
};

pub fn adapter_of(host: HostSite) -> Arc<dyn CodeHost> {
    match host {
        HostSite::GitHub => Arc::new(GitHub::from_env()),
    }
}

/// A PR's code is read without a clone through the adapter's own code source, which no port hands
/// out yet, so whoever reads a PR's code holds the adapter by its type.
pub(crate) fn code_reader_of(host: HostSite) -> GitHub {
    match host {
        HostSite::GitHub => GitHub::from_env(),
    }
}

/// The code host a review talks to: a PR's, or a local review's `NoHost`.
pub fn host_of(subject: &ReviewSubject) -> Arc<dyn CodeHost> {
    match subject {
        ReviewSubject::PullRequest(pr) => adapter_of(pr.host),
        ReviewSubject::Local(change_set) => Arc::new(local_host(change_set)),
    }
}

/// A local review asks a host only for the PR of its branch, on the host its origin is on. With no
/// origin on a host the app reads it asks none, so it has no branch link.
fn local_host(change_set: &LocalChangeSet) -> NoHost {
    change_set.origin().map_or_else(NoHost::default, |origin| {
        NoHost::looking_up_branches_with(adapter_of(origin.host))
    })
}

/// Every code host the app reads, each through its own adapter: the switcher lists the PRs of all of
/// them, and a branch's PR or a PR's branch is looked up on the host its repo or PR is on.
pub struct EveryHost {
    github: Arc<dyn CodeHost>,
}

impl EveryHost {
    pub fn from_env() -> Self {
        Self {
            github: adapter_of(HostSite::GitHub),
        }
    }

    fn adapter(&self, host: HostSite) -> &dyn CodeHost {
        match host {
            HostSite::GitHub => self.github.as_ref(),
        }
    }
}

impl PullsOfBranchPort for EveryHost {
    fn open_pull_of_branch(
        &self,
        repo: &HostedRepo,
        branch: &BranchName,
    ) -> Result<Option<PullOfBranch>, HostError> {
        self.adapter(repo.host).open_pull_of_branch(repo, branch)
    }

    fn pull_request_head_branch(
        &self,
        pr: &PullRequestUrl,
    ) -> Result<Option<BranchName>, HostError> {
        self.adapter(pr.host).pull_request_head_branch(pr)
    }
}

impl RelevantPullRequestsPort for EveryHost {
    fn relevant_pull_requests(&self) -> Result<RelevantPullRequests, HostError> {
        RelevantPullRequests::merged(
            HostSite::ALL.map(|host| self.adapter(host).relevant_pull_requests()),
        )
    }
}
