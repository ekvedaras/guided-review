use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

use crate::support::FixtureRepo;
use crate::support::folders_in;
use guided_review::config::Config;
use guided_review::pull_request::RepoName;
use guided_review::viewer::{CloneOffer, CloneOfferState, ClonePort};
use tempfile::TempDir;

fn shop() -> RepoName {
    RepoName::new("acme", "shop")
}

fn write_config(home: &Path, text: &str) {
    let path = home.join(".config/guided-review/config.toml");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn make_clone_of(repo: &RepoName, folder: &Path) {
    std::fs::create_dir_all(folder).unwrap();
    let git = |args: &[&str]| {
        let status = Command::new("git")
            .args(args)
            .current_dir(folder)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap()
            .status;
        assert!(status.success());
    };
    git(&["init", "-q"]);
    git(&[
        "remote",
        "add",
        "origin",
        &format!("git@github.com:{repo}.git"),
    ]);
}

#[derive(Default)]
struct FakeCloner {
    cloned: Mutex<Vec<PathBuf>>,
    refusal: Option<String>,
}

impl ClonePort for FakeCloner {
    fn clone_repo(&self, repo: &RepoName, folder: &Path) -> anyhow::Result<()> {
        if let Some(reason) = &self.refusal {
            anyhow::bail!("{reason}");
        }
        make_clone_of(repo, folder);
        self.cloned.lock().unwrap().push(folder.to_path_buf());
        Ok(())
    }
}

#[test]
fn the_folder_is_suggested_beside_the_clones_the_config_names() {
    let home = TempDir::new().unwrap();
    write_config(
        home.path(),
        &format!(
            "[clones]\n\"acme/web\" = \"{}\"\n",
            home.path().join("Code/web").display()
        ),
    );

    let offer = CloneOffer::new(shop(), &folders_in(home.path()));

    assert_eq!(offer.folder(), home.path().join("Code/shop"));
}

#[test]
fn with_no_clones_the_folder_is_suggested_in_a_code_folder_of_home() {
    let home = TempDir::new().unwrap();
    std::fs::create_dir_all(home.path().join("PhpStormProjects")).unwrap();

    let offer = CloneOffer::new(shop(), &folders_in(home.path()));

    assert_eq!(offer.folder(), home.path().join("PhpStormProjects/shop"));
}

#[test]
fn with_no_code_folder_the_folder_is_suggested_in_home() {
    let home = TempDir::new().unwrap();

    let offer = CloneOffer::new(shop(), &folders_in(home.path()));

    assert_eq!(offer.folder(), home.path().join("shop"));
}

#[test]
fn a_picked_folder_gets_the_clone_inside_it() {
    let home = TempDir::new().unwrap();
    let mut offer = CloneOffer::new(shop(), &folders_in(home.path()));

    offer.pick_folder(home.path().join("elsewhere"));

    assert_eq!(offer.folder(), home.path().join("elsewhere/shop"));
}

#[test]
fn a_picked_folder_that_is_a_clone_of_the_repo_is_used_as_it_is() {
    let existing = FixtureRepo::cloned_from("acme/shop");
    let mut offer = CloneOffer::new(shop(), &existing.folders());

    offer.pick_folder(existing.path().to_path_buf());

    assert_eq!(offer.folder(), existing.path());
}

#[test]
fn a_clone_is_made_and_saved_as_the_repos_clone() {
    let home = TempDir::new().unwrap();
    let folders = folders_in(home.path());
    let mut offer = CloneOffer::new(shop(), &folders);
    let cloner = FakeCloner::default();

    let job = offer.start().unwrap();
    assert_eq!(offer.state(), &CloneOfferState::Cloning);
    job.run(&cloner).unwrap();

    let folder = home.path().join("shop");
    assert_eq!(
        *cloner.cloned.lock().unwrap(),
        std::slice::from_ref(&folder)
    );
    assert_eq!(
        Config::load_from(&folders).unwrap().clone_of(&shop()),
        Some(folder)
    );
}

#[test]
fn a_folder_that_is_already_a_clone_is_saved_without_cloning() {
    let existing = FixtureRepo::cloned_from("acme/shop");
    let folders = existing.folders();
    let mut offer = CloneOffer::new(shop(), &folders);
    offer.pick_folder(existing.path().to_path_buf());
    let cloner = FakeCloner::default();

    offer.start().unwrap().run(&cloner).unwrap();

    assert!(cloner.cloned.lock().unwrap().is_empty());
    assert_eq!(
        Config::load_from(&folders).unwrap().clone_of(&shop()),
        Some(existing.path().to_path_buf())
    );
}

#[test]
fn a_clone_that_fails_saves_nothing_and_can_be_tried_again() {
    let home = TempDir::new().unwrap();
    let folders = folders_in(home.path());
    let mut offer = CloneOffer::new(shop(), &folders);
    let cloner = FakeCloner {
        refusal: Some("repository not found".into()),
        ..FakeCloner::default()
    };

    let error = offer.start().unwrap().run(&cloner).unwrap_err();
    offer.failed(format!("{error:#}"));

    assert_eq!(
        offer.state(),
        &CloneOfferState::Failed("repository not found".into())
    );
    assert_eq!(Config::load_from(&folders).unwrap().clone_of(&shop()), None);
    assert!(offer.start().is_some());
}

#[test]
fn a_clone_running_is_not_started_twice() {
    let home = TempDir::new().unwrap();
    let mut offer = CloneOffer::new(shop(), &folders_in(home.path()));

    offer.start().unwrap();

    assert!(offer.start().is_none());
}

#[test]
fn the_offer_names_the_repo_and_the_folder_from_home() {
    let home = TempDir::new().unwrap();

    let offer = CloneOffer::new(shop(), &folders_in(home.path()));

    assert_eq!(
        offer.line(),
        "The code comes from GitHub, so it loads slowly. Clone acme/shop to ~/shop?"
    );
}
