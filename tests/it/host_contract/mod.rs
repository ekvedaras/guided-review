//! Shared cases per port, run against every host, so each one behaves the same at the seam.
//! A case is written once, over a builder that makes a host holding a given PR; `contract_for!`
//! runs every case against one host.

mod capabilities;
mod github_api;
mod hosts;
mod pull_request;
mod repo;
mod review;

use std::sync::Arc;

use guided_review::pull_request::{PullRequestUrl, Sha};
use guided_review::viewer::{
    Checks, CodeHost, IssueState, Merging, OpenPrStatus, PrAuthor, PrStatus, Readiness,
    ReviewOutcome, ReviewRecord, Reviewer, ReviewersOnHost, ViewedState,
};

use crate::support::fake_ports::blocked_merging;

/// The login every host knows the viewer by.
pub const VIEWER: &str = "reviewer";

/// When the PR last changed, as each host writes it.
pub const UPDATED_AT: &str = "2026-11-20T10:00:00Z";

/// The PR a case starts from, which each host is built to hold.
#[derive(Clone)]
pub struct PullOnHost {
    pub title: &'static str,
    pub description: &'static str,
    pub author: &'static str,
    pub base_branch: &'static str,
    pub head_branch: &'static str,
    /// Who owns the fork the PR comes from; `None` for a branch of the repo itself.
    pub fork_owner: Option<&'static str>,
    pub base_branch_tip: &'static str,
    pub head: &'static str,
    pub status: PrStatus,
    pub merging: Merging,
    pub closing_issues: Vec<SeedIssue>,
    pub asks_viewer_to_review: bool,
    /// Reviews sent by others, each at the head.
    pub reviews: Vec<(&'static str, ReviewOutcome)>,
    /// A pending review the viewer started on the host's web page.
    pub web_pending_review: bool,
    pub code: SeedCode,
    pub files: Vec<(&'static str, ViewedState)>,
    pub threads: Vec<SeedThread>,
    pub reach: Reach,
}

impl Default for PullOnHost {
    fn default() -> Self {
        Self {
            title: "Add a cart",
            description: "Adds a cart.",
            author: "alice",
            base_branch: "main",
            head_branch: "cart",
            fork_owner: None,
            base_branch_tip: "aaa",
            head: "bbb",
            status: open(Readiness::Ready),
            merging: blocked_merging(),
            closing_issues: Vec::new(),
            asks_viewer_to_review: false,
            reviews: Vec::new(),
            web_pending_review: false,
            code: SeedCode::default(),
            files: Vec::new(),
            threads: Vec::new(),
            reach: Reach::Answers,
        }
    }
}

pub fn open(readiness: Readiness) -> PrStatus {
    PrStatus::Open(OpenPrStatus {
        readiness,
        checks: Checks::NoneRun,
        check_runs: Arc::default(),
        viewer_may_change_readiness: true,
    })
}

/// How the host answers the app.
#[derive(Clone, Copy, Default)]
pub enum Reach {
    #[default]
    Answers,
    /// The app has no token for the host.
    NoToken,
    /// The host answers every request with a server error.
    ServerError,
}

#[derive(Clone)]
pub struct SeedThread {
    pub id: &'static str,
    pub path: &'static str,
    pub line: usize,
    pub resolved: bool,
    pub comments: Vec<SeedComment>,
}

/// A comment someone other than the viewer wrote.
#[derive(Clone)]
pub struct SeedComment {
    pub id: &'static str,
    pub author: &'static str,
    pub body: &'static str,
}

/// An issue of the PR's repo on the host's tracker, which the PR closes.
#[derive(Clone)]
pub struct SeedIssue {
    pub number: u64,
    pub title: &'static str,
    pub state: IssueState,
    pub author: &'static str,
    pub labels: Vec<&'static str>,
    pub assignees: Vec<&'static str>,
    pub body: &'static str,
}

/// The repo's code as the host serves it: files at commits, the diff of the PR's tips as a unified
/// diff, and their merge base.
#[derive(Clone, Default)]
pub struct SeedCode {
    pub files: Vec<(&'static str, &'static str, &'static str)>,
    pub diff: &'static str,
    pub merge_base: &'static str,
}

pub type HostBuilder = fn(PullOnHost) -> Box<dyn CodeHost>;

pub fn pr() -> PullRequestUrl {
    "https://github.com/acme/shop/pull/7".parse().unwrap()
}

/// Where each host says a person's avatar is.
pub fn avatar_of(login: &str) -> String {
    format!("https://avatars.example/{login}")
}

pub fn author_of(pull: &PullOnHost) -> PrAuthor {
    PrAuthor {
        login: pull.author.into(),
        avatar_url: Some(avatar_of(pull.author).into()),
    }
}

/// What the host says of the PR's reviewers: each review sent by others, and the viewer asked
/// for one when the PR asks.
pub fn reviewers_on_host(pull: &PullOnHost) -> ReviewersOnHost {
    let person = |login: &str| Reviewer::person(login, Some(&avatar_of(login)));
    let record = |(login, outcome): &(&str, ReviewOutcome)| ReviewRecord {
        reviewer: person(login),
        outcome: *outcome,
        commit: Sha::new(pull.head),
    };
    ReviewersOnHost {
        viewer_login: VIEWER.to_owned(),
        author_login: Some(pull.author.to_owned()),
        latest: pull.reviews.iter().map(record).collect(),
        verdicts: pull
            .reviews
            .iter()
            .filter(|(_, outcome)| {
                matches!(
                    outcome,
                    ReviewOutcome::Approved | ReviewOutcome::ChangesRequested
                )
            })
            .map(record)
            .collect(),
        requested: if pull.asks_viewer_to_review {
            vec![person(VIEWER)]
        } else {
            Vec::new()
        },
    }
}

/// Names a file's bytes, so equal bytes have equal ids, as git's blob ids do.
pub fn blob_id(bytes: &[u8]) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

macro_rules! contract_for {
    ($host:ident: $($case:ident in $($port:ident)::+),* $(,)?) => {
        mod $host {
            $(
                #[test]
                fn $case() {
                    super::$($port)::+::$case(super::hosts::$host);
                }
            )*
        }
    };
}

macro_rules! every_case_for {
    ($($host:ident),*) => {
        $(
            contract_for!($host:
                viewed_files_list_every_changed_file_with_its_state in review::viewed_marks,
                a_file_marked_viewed_reads_back_viewed_and_unmarked_reads_back_unviewed in review::viewed_marks,
                viewed_marks_without_a_token_are_refused in review::viewed_marks,
                viewed_marks_of_a_host_not_answering_fail_as_an_outage in review::viewed_marks,
                review_threads_list_each_thread_with_its_comments in review::threads,
                a_reply_reads_back_as_the_last_comment_of_its_thread in review::threads,
                a_reply_to_a_thread_the_pr_does_not_have_fails in review::threads,
                a_new_thread_reads_back_with_the_viewers_comment_where_it_was_put in review::threads,
                a_resolved_thread_reads_back_resolved_and_an_unresolved_one_open in review::threads,
                an_edited_comment_reads_back_with_its_new_body_and_a_deleted_one_is_gone in review::threads,
                a_reaction_on_a_comment_reads_back_as_the_viewers_and_a_removed_one_is_gone in review::threads,
                review_threads_without_a_token_are_refused in review::threads,
                review_threads_of_a_host_not_answering_fail_as_an_outage in review::threads,
                the_tips_are_the_base_branch_tip_and_the_head in pull_request::change_tips,
                change_tips_of_a_host_not_answering_fail_as_an_outage in pull_request::change_tips,
                the_open_pr_of_its_head_branch_is_found_and_no_other_branch_has_one in pull_request::pulls_of_branch,
                the_head_branch_of_a_pr_from_the_repo_itself_is_known_and_of_a_fork_none in pull_request::pulls_of_branch,
                pulls_of_branch_of_a_host_not_answering_fail_as_an_outage in pull_request::pulls_of_branch,
                the_pr_is_listed_as_the_viewers_own_as_asked_of_them_or_as_involving_them in pull_request::relevant,
                the_title_and_description_read_as_the_host_has_them in pull_request::description,
                a_public_repos_pr_and_code_read_without_a_token in pull_request::public_reads,
                a_description_written_reads_back in pull_request::description,
                the_issues_the_pr_closes_are_linked_with_their_titles_and_the_head_branch in pull_request::description,
                the_description_of_a_host_not_answering_fails_as_an_outage in pull_request::description,
                the_origin_is_the_author_and_the_branches_of_the_repo_or_a_fork in pull_request::origin,
                the_origin_of_a_host_not_answering_fails_as_an_outage in pull_request::origin,
                the_status_reads_as_the_host_has_it in pull_request::status,
                a_readiness_set_reads_back in pull_request::status,
                the_status_without_a_token_is_refused in pull_request::status,
                the_status_of_a_host_not_answering_fails_as_an_outage in pull_request::status,
                merging_reads_as_the_host_has_it in pull_request::merging,
                auto_merge_turned_on_and_off_reads_back_and_a_merge_now_merges_the_pr in pull_request::merging,
                merging_of_a_host_not_answering_fails_as_an_outage in pull_request::merging,
                reactions_on_the_pr_are_taken_added_and_removed in pull_request::reactions,
                reactions_on_the_pr_without_a_token_are_refused in pull_request::reactions,
                reactions_on_the_pr_of_a_host_not_answering_fail_as_an_outage in pull_request::reactions,
                an_uploaded_picture_gets_an_address_on_the_host in repo::picture_uploads,
                a_file_that_is_no_picture_or_cannot_be_read_is_not_uploaded in repo::picture_uploads,
                an_upload_without_a_token_or_to_a_host_not_answering_fails in repo::picture_uploads,
                each_known_issue_has_its_details_and_an_unknown_one_none in repo::issue_tracker,
                issue_details_of_a_host_not_answering_fail_as_an_outage in repo::issue_tracker,
                no_pending_review_until_a_thread_is_added_and_none_once_it_is_sent in review::writes,
                a_pending_review_started_on_the_web_counts in review::writes,
                whether_the_viewer_wrote_the_pr_reads_as_the_host_has_it in review::writes,
                a_comment_now_leaves_no_pending_review in review::writes,
                a_comment_now_past_a_pending_review_is_refused_and_sends_nothing in review::writes,
                each_verdict_reads_back_as_the_viewers_last_review in review::writes,
                someone_elses_review_stands_as_theirs in review::writes,
                a_comment_or_reply_posted_now_is_the_viewers_last_review in review::writes,
                a_sent_review_stands_as_the_viewers in review::writes,
                review_writes_without_a_token_are_refused in review::writes,
                review_writes_of_a_host_not_answering_fail_as_an_outage in review::writes,
                a_file_reads_at_each_commit_with_its_blob_info in repo::code,
                a_clone_is_a_git_folder_whose_origin_is_the_repo_on_the_host in repo::clones,
                a_clone_from_a_host_not_answering_fails_and_makes_no_folder in repo::clones,
                the_diff_and_merge_base_of_the_tips_read_as_the_host_has_them in repo::code,
            );
        )*
    };
}

every_case_for!(fake_host, github);
