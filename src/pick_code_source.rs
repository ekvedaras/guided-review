use std::path::Path;

use crate::code_source::CodeSource;
use crate::github::GitHub;
use crate::local_git::LocalGit;
use crate::pull_request::{PullRequestTips, PullRequestUrl};

/// Run at the start of every command, so a clone made mid-review is used from the next command on.
pub fn pick_code_source(
    github: &GitHub,
    pr: &PullRequestUrl,
    tips: &PullRequestTips,
    current_dir: &Path,
) -> anyhow::Result<Box<dyn CodeSource>> {
    if let Some(clone) = LocalGit::find(&pr.repo, current_dir)? {
        match clone.fetch_missing_commits(pr, tips) {
            Ok(()) => return Ok(Box::new(clone)),
            Err(error) => eprintln!("warning: using GitHub instead of the local clone: {error:#}"),
        }
    }
    Ok(Box::new(github.code_source(&pr.repo)))
}
