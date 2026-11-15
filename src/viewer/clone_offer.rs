use std::path::{Path, PathBuf};

use crate::app_folders::AppFolders;
use crate::code_source::CodeSource;
use crate::home::expand_home;
use crate::local_git::LocalGit;
use crate::pull_request::RepoName;

use super::settings::{SaveOutcome, SettingProblem, Settings};

/// Folders in home where people keep their code, the usual first.
const CODE_FOLDERS: &[&str] = &[
    "Work",
    "Code",
    "code",
    "Projects",
    "projects",
    "Developer",
    "dev",
    "src",
    "repos",
    "git",
    "PhpStormProjects",
    "IdeaProjects",
    "WebstormProjects",
    "PycharmProjects",
    "GolandProjects",
    "RustroverProjects",
];

/// Makes a clone of a GitHub repo in a folder that does not exist yet.
pub trait RepoCloner: Send + Sync {
    fn clone_repo(&self, repo: &RepoName, folder: &Path) -> anyhow::Result<()>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloneOfferState {
    Offered,
    Cloning,
    Failed(String),
}

/// The sidebar's offer to clone a repo whose code is read from GitHub, so the next reads come from disk.
pub struct CloneOffer {
    repo: RepoName,
    folders: AppFolders,
    folder: PathBuf,
    state: CloneOfferState,
}

impl CloneOffer {
    /// Only when no clone was found: a clone that could not give the commits has a problem of its own.
    pub fn for_source(
        repo: RepoName,
        source: &dyn CodeSource,
        folders: &AppFolders,
    ) -> Option<Self> {
        let has_no_clone = source.local_clone().is_none() && source.fallback_reason().is_none();
        has_no_clone.then(|| Self::new(repo, folders))
    }

    pub fn new(repo: RepoName, folders: &AppFolders) -> Self {
        let home = folders.home();
        let beside_clones = Settings::load(folders).ok().and_then(|settings| {
            settings.clones().iter().find_map(|clone| {
                expand_home(&clone.folder, home)
                    .parent()
                    .map(Path::to_path_buf)
            })
        });
        let parent = beside_clones
            .or_else(|| {
                CODE_FOLDERS
                    .iter()
                    .map(|name| home.join(name))
                    .find(|folder| folder.is_dir())
            })
            .unwrap_or_else(|| home.to_path_buf());
        Self {
            folder: parent.join(repo.name()),
            repo,
            folders: folders.clone(),
            state: CloneOfferState::Offered,
        }
    }

    pub fn folder(&self) -> &Path {
        &self.folder
    }

    pub fn state(&self) -> &CloneOfferState {
        &self.state
    }

    pub fn line(&self) -> String {
        let shown = self.folder.strip_prefix(self.folders.home()).map_or_else(
            |_| self.folder.display().to_string(),
            |in_home| format!("~/{}", in_home.display()),
        );
        format!(
            "The code comes from GitHub, so it loads slowly. Clone {} to {shown}?",
            self.repo
        )
    }

    /// A folder that already is a clone of the repo is used as it is; any other gets the clone inside it.
    pub fn pick_folder(&mut self, picked: PathBuf) {
        self.folder = if LocalGit::configured_clone(&self.repo, &picked).is_ok() {
            picked
        } else {
            picked.join(self.repo.name())
        };
    }

    pub fn start(&mut self) -> Option<CloneJob> {
        if self.state == CloneOfferState::Cloning {
            return None;
        }
        self.state = CloneOfferState::Cloning;
        Some(CloneJob {
            repo: self.repo.clone(),
            folders: self.folders.clone(),
            folder: self.folder.clone(),
        })
    }

    pub fn failed(&mut self, reason: String) {
        self.state = CloneOfferState::Failed(reason);
    }
}

/// Blocks on the network; the app runs it off the main thread.
pub struct CloneJob {
    repo: RepoName,
    folders: AppFolders,
    folder: PathBuf,
}

impl CloneJob {
    pub fn run(&self, cloner: &dyn RepoCloner) -> anyhow::Result<()> {
        if LocalGit::configured_clone(&self.repo, &self.folder).is_err() {
            cloner.clone_repo(&self.repo, &self.folder)?;
        }
        let mut settings = Settings::load(&self.folders)?;
        let row = settings.add_clone();
        settings.set_clone_repo(row, self.repo.to_string());
        settings.set_clone_folder(row, self.folder.clone());
        match settings.save()? {
            SaveOutcome::Written | SaveOutcome::NothingChanged => Ok(()),
            SaveOutcome::Refused(problems) => anyhow::bail!(
                "cloned, but Settings did not save it: {}",
                problems
                    .iter()
                    .map(SettingProblem::message)
                    .collect::<Vec<_>>()
                    .join("; ")
            ),
        }
    }
}
