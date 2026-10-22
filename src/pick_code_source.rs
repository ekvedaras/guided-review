use std::path::Path;
use std::sync::Arc;

use crate::cached_code_source::CachedCodeSource;
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
    let source = pick_uncached_code_source(github, pr, tips, current_dir)?;
    Ok(Box::new(CachedCodeSource::wrapping(source)))
}

fn pick_uncached_code_source(
    github: &GitHub,
    pr: &PullRequestUrl,
    tips: &PullRequestTips,
    current_dir: &Path,
) -> anyhow::Result<Arc<dyn CodeSource>> {
    if let Some(clone) = LocalGit::find(&pr.repo, current_dir)? {
        match clone.fetch_missing_commits(pr, tips) {
            Ok(()) => return Ok(Arc::new(clone)),
            Err(error) => eprintln!("warning: using GitHub instead of the local clone: {error:#}"),
        }
    }
    Ok(Arc::new(github.code_source(&pr.repo)))
}
