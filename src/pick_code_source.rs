use std::path::Path;
use std::sync::Arc;

use crate::cached_code_source::CachedCodeSource;
use crate::code_source::{BlobInfo, CodeSource, FileAt, FileDiff};
use crate::file_path::FilePath;
use crate::local_git::LocalGit;
use crate::pull_request::{ChangeTips, PullRequestUrl, Sha};
use crate::viewer::HostedCodePort;

/// Run at the start of every command, so a clone made mid-review is used from the next command on.
pub fn pick_code_source(
    host: &dyn HostedCodePort,
    pr: &PullRequestUrl,
    tips: &ChangeTips,
    current_dir: &Path,
) -> anyhow::Result<Box<dyn CodeSource>> {
    let source = pick_uncached_code_source(host, pr, tips, current_dir)?;
    Ok(Box::new(CachedCodeSource::wrapping(source)))
}

fn pick_uncached_code_source(
    host: &dyn HostedCodePort,
    pr: &PullRequestUrl,
    tips: &ChangeTips,
    current_dir: &Path,
) -> anyhow::Result<Arc<dyn CodeSource>> {
    let Some(clone) = LocalGit::find(&pr.hosted_repo(), current_dir)? else {
        return host.code_of(&pr.repo);
    };
    match clone.fetch_missing_commits(pr, tips) {
        Ok(()) => Ok(Arc::new(clone)),
        Err(error) => {
            eprintln!(
                "warning: using {} instead of the local clone: {error:#}",
                pr.host.name()
            );
            Ok(Arc::new(InsteadOfClone {
                source: host.code_of(&pr.repo)?,
                reason: format!("{error:#}"),
            }))
        }
    }
}

/// The host's code, read because the PR's clone could not give the commits.
struct InsteadOfClone {
    source: Arc<dyn CodeSource>,
    reason: String,
}

impl CodeSource for InsteadOfClone {
    fn file_at(&self, sha: &Sha, path: &FilePath) -> anyhow::Result<String> {
        self.source.file_at(sha, path)
    }

    fn blob_at(&self, sha: &Sha, path: &FilePath) -> anyhow::Result<Arc<[u8]>> {
        self.source.blob_at(sha, path)
    }

    fn blob_info_at(&self, sha: &Sha, path: &FilePath) -> anyhow::Result<BlobInfo> {
        self.source.blob_info_at(sha, path)
    }

    fn diff(&self, base: &Sha, head: &Sha) -> anyhow::Result<Vec<FileDiff>> {
        self.source.diff(base, head)
    }

    fn merge_base(&self, a: &Sha, b: &Sha) -> anyhow::Result<Sha> {
        self.source.merge_base(a, b)
    }

    fn working_tree_is_at(&self, head: &Sha) -> bool {
        self.source.working_tree_is_at(head)
    }

    fn fallback_reason(&self) -> Option<&str> {
        Some(&self.reason)
    }

    fn read_ahead(&self, files: &[FileAt]) {
        self.source.read_ahead(files);
    }
}
