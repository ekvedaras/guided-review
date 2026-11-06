use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Context;

use crate::code_source::CodeSource;
use crate::github::GitHub;
use crate::pick_code_source::pick_code_source;
use crate::pieces::{self, ChangedFile};
use crate::pull_request::{PullRequestTips, PullRequestUrl, Sha};
use crate::tour::Tour;
use crate::viewer::{PickCodeSource, PullRequestTipsPort, ReviewStateFile};

/// What every command needs to work on one PR: where its files live, GitHub, and the folder it runs in.
pub struct OpenedPr {
    pub pr: PullRequestUrl,
    pub home: PathBuf,
    pub current_dir: PathBuf,
    pub github: GitHub,
}

impl OpenedPr {
    pub fn new(pr: &PullRequestUrl) -> anyhow::Result<Self> {
        Ok(Self {
            pr: pr.clone(),
            home: home_folder()?,
            current_dir: std::env::current_dir().context("could not read the current folder")?,
            github: GitHub::from_env(),
        })
    }

    pub fn in_folders(pr: &PullRequestUrl, home: &Path, current_dir: &Path) -> Self {
        Self {
            pr: pr.clone(),
            home: home.to_path_buf(),
            current_dir: current_dir.to_path_buf(),
            github: GitHub::from_env(),
        }
    }

    pub fn tour_path(&self) -> PathBuf {
        Tour::path_for(&self.home, &self.pr)
    }

    pub fn state_file(&self) -> ReviewStateFile {
        ReviewStateFile::for_pr(&self.home, &self.pr)
    }

    /// The PR as GitHub has it now.
    pub fn at_live_tips(&self) -> anyhow::Result<CodeAtTips> {
        let tips = self.github.pull_request_tips(&self.pr)?;
        self.at(tips)
    }

    /// The tour is read at the commits it is pinned to, even when the PR has moved on since.
    pub fn at_pinned_tips(&self, tour: &Tour) -> anyhow::Result<CodeAtTips> {
        self.at(tour.pinned_tips())
    }

    fn at(&self, tips: PullRequestTips) -> anyhow::Result<CodeAtTips> {
        let source = pick_code_source(&self.github, &self.pr, &tips, &self.current_dir)?;
        Ok(CodeAtTips { tips, source })
    }

    /// Picks the code source again for any tips, as the app does when the PR moves.
    pub fn code_source_picker(&self) -> PickCodeSource {
        let (github, pr, current_dir) = (
            self.github.clone(),
            self.pr.clone(),
            self.current_dir.clone(),
        );
        Arc::new(move |tips| pick_code_source(&github, &pr, tips, &current_dir))
    }
}

pub fn home_folder() -> anyhow::Result<PathBuf> {
    std::env::home_dir().context("could not find the home folder")
}

pub struct CodeAtTips {
    pub tips: PullRequestTips,
    pub source: Box<dyn CodeSource>,
}

impl CodeAtTips {
    pub fn merge_base(&self) -> anyhow::Result<Sha> {
        self.source
            .merge_base(&self.tips.base_branch_tip, &self.tips.head)
    }

    /// The merge base and what changed from it to the head.
    pub fn pieces_since_merge_base(&self) -> anyhow::Result<(Sha, Vec<ChangedFile>)> {
        let base = self.merge_base()?;
        let files = pieces::cut(self.source.as_ref(), &base, &self.tips.head)?;
        Ok((base, files))
    }
}
