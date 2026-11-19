use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use guided_review::pull_request::Sha;
use guided_review::review_subject::ReviewSubject;
use guided_review::review_worktree::{ReviewWorktree, ReviewWorktreePort};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorktreeCall {
    Add {
        subject: String,
        repo: PathBuf,
        head: String,
    },
    Remove {
        subject: String,
    },
}

/// Checkouts that are only a record, for tests of what starts and ends a review agent.
#[derive(Clone, Default)]
pub struct FakeReviewWorktrees {
    calls: Arc<Mutex<Vec<WorktreeCall>>>,
    refusing: bool,
}

impl FakeReviewWorktrees {
    pub fn refusing() -> Self {
        Self {
            refusing: true,
            ..Self::default()
        }
    }

    pub fn calls(&self) -> Vec<WorktreeCall> {
        self.calls.lock().unwrap().clone()
    }

    /// The subjects that have a checkout now.
    pub fn standing(&self) -> Vec<String> {
        let mut standing: Vec<String> = Vec::new();
        for call in self.calls() {
            match call {
                WorktreeCall::Add { subject, .. } => standing.push(subject),
                WorktreeCall::Remove { subject } => standing.retain(|kept| *kept != subject),
            }
        }
        standing
    }
}

impl ReviewWorktreePort for FakeReviewWorktrees {
    fn add(
        &self,
        subject: &ReviewSubject,
        repo: &Path,
        head: &Sha,
    ) -> anyhow::Result<ReviewWorktree> {
        anyhow::ensure!(!self.refusing, "git refused the worktree");
        self.calls.lock().unwrap().push(WorktreeCall::Add {
            subject: subject.to_string(),
            repo: repo.to_path_buf(),
            head: head.to_string(),
        });
        Ok(ReviewWorktree::at(
            PathBuf::from("/fake/review-worktrees").join(subject.file_stem()),
        ))
    }

    fn remove(&self, subject: &ReviewSubject) -> anyhow::Result<()> {
        self.calls.lock().unwrap().push(WorktreeCall::Remove {
            subject: subject.to_string(),
        });
        Ok(())
    }
}
