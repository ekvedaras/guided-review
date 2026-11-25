//! Shared cases per port, run against every host, so each one behaves the same at the seam.
//! A case is written once, over a builder that makes a host holding a given PR; `contract_for!`
//! runs every case against one host.

mod capabilities;
mod github_api;
mod hosts;
mod review_threads;
mod viewed_marks;

use guided_review::pull_request::PullRequestUrl;
use guided_review::viewer::{CodeHost, ViewedState};

/// The PR a case starts from, which each host is built to hold.
#[derive(Clone, Default)]
pub struct PullOnHost {
    pub files: Vec<(&'static str, ViewedState)>,
    pub threads: Vec<SeedThread>,
    pub reach: Reach,
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

pub type HostBuilder = fn(PullOnHost) -> Box<dyn CodeHost>;

pub fn pr() -> PullRequestUrl {
    "https://github.com/acme/shop/pull/7".parse().unwrap()
}

macro_rules! contract_for {
    ($host:ident: $($case:ident in $port:ident),* $(,)?) => {
        mod $host {
            $(
                #[test]
                fn $case() {
                    super::$port::$case(super::hosts::$host);
                }
            )*
        }
    };
}

macro_rules! every_case_for {
    ($($host:ident),*) => {
        $(
            contract_for!($host:
                viewed_files_list_every_changed_file_with_its_state in viewed_marks,
                a_file_marked_viewed_reads_back_viewed_and_unmarked_reads_back_unviewed in viewed_marks,
                viewed_marks_without_a_token_are_refused in viewed_marks,
                viewed_marks_of_a_host_not_answering_fail_as_an_outage in viewed_marks,
                review_threads_list_each_thread_with_its_comments in review_threads,
                a_reply_reads_back_as_the_last_comment_of_its_thread in review_threads,
                a_resolved_thread_reads_back_resolved_and_an_unresolved_one_open in review_threads,
                review_threads_without_a_token_are_refused in review_threads,
                review_threads_of_a_host_not_answering_fail_as_an_outage in review_threads,
            );
        )*
    };
}

every_case_for!(fake_host, github);
