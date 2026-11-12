use std::ffi::OsStr;
use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};

use anyhow::Context;
use flate2::read::GzDecoder;
use sha2::{Digest, Sha256};

use super::app_bundle::{AppBundle, BUNDLE_FOLDER, BundlePlaced};
use super::{ReleasesPort, Update, Version};

/// The way the README installs a release: each version unpacked in its own folder under
/// `~/.local/share/guided-review/<version>/`, with `~/.local/bin/guided-review` and
/// `~/.claude/skills/tour` linked into the newest. An update unpacks beside the old version and moves
/// the links, so the running binary is never written over.
#[derive(Debug, Clone)]
pub struct ReleaseInstall {
    versions: PathBuf,
    bin_link: PathBuf,
    skill_link: PathBuf,
    bundle: AppBundle,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installed {
    pub version: Version,
    /// The new version's own binary, which the restart runs: never the bin link or a name looked up on
    /// `PATH`, either of which may lead to another binary by then.
    pub program: PathBuf,
    pub skill: SkillLink,
}

/// A `guided-review` that is not the release install but runs in its place, so the release install's
/// updates never reach the reviewer. Removing `binary` puts it right.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InTheWay {
    /// The app runs `binary` itself.
    Runs {
        binary: PathBuf,
        release_install: PathBuf,
    },
    /// The app is the release install, but `binary` comes before it on `PATH`, so `guided-review` in a
    /// terminal or run by the tour skill is that one.
    FirstOnPath {
        binary: PathBuf,
        release_install: PathBuf,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillLink {
    Moved,
    /// The tour skill is not a link into the release folders, so the reviewer keeps it up to date.
    NotOurs,
}

impl ReleaseInstall {
    pub fn in_home(home: &Path) -> Self {
        Self {
            versions: home.join(".local/share/guided-review"),
            bin_link: home.join(".local/bin/guided-review"),
            skill_link: home.join(".claude/skills/tour"),
            bundle: AppBundle::in_home(home),
        }
    }

    /// Whether `exe` is the binary the bin link points at in the release folders. A binary built from
    /// source, or moved in by hand, has no link to move, so it is never updated.
    pub fn runs(&self, exe: &Path) -> bool {
        let Some(linked) = self.linked_into_versions(&self.bin_link) else {
            return false;
        };
        fs::canonicalize(exe).is_ok_and(|exe| exe == linked)
    }

    /// The `guided-review` that runs in place of the release install, when one is installed: `exe`
    /// itself, or the first `guided-review` on `path` that does not lead to it.
    pub fn in_the_way(&self, exe: &Path, path: &OsStr) -> Option<InTheWay> {
        let release = self.linked_into_versions(&self.bin_link)?;
        let release_install = self.bin_link.clone();
        if !self.runs(exe) {
            return Some(InTheWay::Runs {
                binary: exe.to_path_buf(),
                release_install,
            });
        }
        let first_on_path = std::env::split_paths(path)
            .map(|folder| folder.join("guided-review"))
            .find(|binary| binary.is_file())?;
        let leads_to_release = fs::canonicalize(&first_on_path).is_ok_and(|it| it == release);
        (!leads_to_release).then_some(InTheWay::FirstOnPath {
            binary: first_on_path,
            release_install,
        })
    }

    /// Puts the bundle of the running release in `~/Applications` when it is missing or another
    /// version's. An update by 0.6.0, which knew nothing of the bundle, leaves it so.
    pub fn bring_bundle_up_to_date(&self, exe: &Path) -> anyhow::Result<BundlePlaced> {
        let release = self
            .linked_into_versions(&self.bin_link)
            .filter(|_| self.runs(exe))
            .and_then(|binary| binary.parent().map(Path::to_path_buf));
        match release {
            Some(release) => self.bundle.place_from(&release.join(BUNDLE_FOLDER)),
            None => Ok(BundlePlaced::NotInRelease),
        }
    }

    /// Checks the archive against its checksum file, unpacks it beside the versions there are, and
    /// moves the links to it. Nothing changes when the archive is not the one the checksum names.
    pub fn install(
        &self,
        update: &Update,
        archive: &[u8],
        checksum_file: &str,
    ) -> anyhow::Result<Installed> {
        check_sum(archive, checksum_file)
            .with_context(|| format!("{} was not installed", update.archive.name))?;
        let folder = self.versions.join(update.version.to_string());
        let unpacking = self.versions.join(format!(".unpacking-{}", update.version));
        let unpacked = unpack(archive, &unpacking, update);
        if unpacked.is_err() && unpacking.exists() {
            fs::remove_dir_all(&unpacking)
                .with_context(|| format!("could not remove {}", unpacking.display()))?;
        }
        unpacked?;
        if folder.exists() {
            fs::remove_dir_all(&folder)
                .with_context(|| format!("could not remove {}", folder.display()))?;
        }
        fs::rename(&unpacking, &folder)
            .with_context(|| format!("could not move the release to {}", folder.display()))?;
        let release = folder.join(update.unpacked_folder());
        let skill = match self.linked_into_versions(&self.skill_link) {
            Some(_) => {
                relink(&self.skill_link, &release.join("skills/tour"))?;
                SkillLink::Moved
            }
            None => SkillLink::NotOurs,
        };
        self.bundle.place_from(&release.join(BUNDLE_FOLDER))?;
        let program = release.join("guided-review");
        relink(&self.bin_link, &program)?;
        Ok(Installed {
            version: update.version,
            program,
            skill,
        })
    }

    pub fn download_and_install(
        &self,
        releases: &dyn ReleasesPort,
        update: &Update,
    ) -> anyhow::Result<Installed> {
        let checksum = releases
            .download(&update.checksum.url)
            .with_context(|| format!("could not download {}", update.checksum.name))?;
        let checksum = String::from_utf8(checksum)
            .with_context(|| format!("{} is not text", update.checksum.name))?;
        let archive = releases
            .download(&update.archive.url)
            .with_context(|| format!("could not download {}", update.archive.name))?;
        self.install(update, &archive, &checksum)
    }

    /// Where `link` leads, when it is a link into the release folders.
    fn linked_into_versions(&self, link: &Path) -> Option<PathBuf> {
        let is_link = fs::symlink_metadata(link).is_ok_and(|meta| meta.file_type().is_symlink());
        let versions = fs::canonicalize(&self.versions).ok()?;
        let target = fs::canonicalize(link).ok()?;
        (is_link && target.starts_with(versions)).then_some(target)
    }
}

/// A checksum file is `shasum -a 256`'s line: the hex digest, then the file's name.
fn check_sum(archive: &[u8], checksum_file: &str) -> anyhow::Result<()> {
    let expected = checksum_file
        .split_whitespace()
        .next()
        .context("the checksum file is empty")?;
    let actual = crate::hex::lower(&Sha256::digest(archive));
    anyhow::ensure!(
        actual.eq_ignore_ascii_case(expected),
        "the archive does not match its checksum: {actual} is not {expected}"
    );
    Ok(())
}

fn unpack(archive: &[u8], into: &Path, update: &Update) -> anyhow::Result<()> {
    if into.exists() {
        fs::remove_dir_all(into).with_context(|| format!("could not remove {}", into.display()))?;
    }
    fs::create_dir_all(into).with_context(|| format!("could not make {}", into.display()))?;
    tar::Archive::new(GzDecoder::new(archive))
        .unpack(into)
        .with_context(|| format!("could not unpack {}", update.archive.name))?;
    let binary = format!("{}/guided-review", update.unpacked_folder());
    anyhow::ensure!(
        into.join(&binary).is_file(),
        "{} has no {binary}",
        update.archive.name
    );
    Ok(())
}

/// Points `link` at `target` in one step: a new link beside it is renamed over it, so there is no
/// moment with no link.
fn relink(link: &Path, target: &Path) -> anyhow::Result<()> {
    let name = link
        .file_name()
        .with_context(|| format!("{} names no file", link.display()))?;
    let mut fresh_name = std::ffi::OsString::from(".");
    fresh_name.push(name);
    fresh_name.push(".updating");
    let fresh = link.with_file_name(fresh_name);
    if fs::symlink_metadata(&fresh).is_ok() {
        fs::remove_file(&fresh).with_context(|| format!("could not remove {}", fresh.display()))?;
    }
    symlink(target, &fresh).with_context(|| format!("could not link {}", fresh.display()))?;
    fs::rename(&fresh, link).with_context(|| format!("could not move {}", link.display()))
}
