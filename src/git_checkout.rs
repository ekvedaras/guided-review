//! The git facts the app needs about a folder to decide where the review agent works.

use std::path::Path;

use crate::pull_request::Sha;
use crate::review_worktree::git;
use crate::viewer::CheckoutPort;

pub struct GitCheckouts;

impl CheckoutPort for GitCheckouts {
    fn head_of(&self, folder: &Path) -> anyhow::Result<Sha> {
        Ok(Sha::new(git(folder, &["rev-parse", "HEAD"])?.trim()))
    }

    fn has_uncommitted_changes(&self, folder: &Path) -> anyhow::Result<bool> {
        Ok(!git(folder, &["status", "--porcelain"])?.trim().is_empty())
    }
}
