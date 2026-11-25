//! The cache folder managed servers download into: one download at a time, each checked before
//! anything of it is unpacked, and a folder placed whole or not at all.

use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use anyhow::Context as _;

use crate::update::{ReleasesPort, Target};

use super::{FallbackReason, ServerName, Waiting, locked};

/// Held while a server downloads, so a second hover waits for it instead of downloading again.
#[derive(Default)]
struct DownloadLock(Mutex<()>);

/// The one download running; the next waits until this is dropped.
#[must_use = "the next download waits only while this is held"]
struct DownloadTurn<'a> {
    _lock: MutexGuard<'a, ()>,
}

impl DownloadLock {
    fn take_turn(&self) -> DownloadTurn<'_> {
        DownloadTurn {
            _lock: locked(&self.0),
        }
    }
}

/// The cache folder the recipes download into, and how they fetch and check what they download.
pub(super) struct Downloads {
    /// One folder per server and pinned version.
    folder: PathBuf,
    releases: Arc<dyn ReleasesPort>,
    target: Target,
    one_at_a_time: DownloadLock,
}

impl Downloads {
    pub(super) fn new(folder: PathBuf, releases: Arc<dyn ReleasesPort>, target: Target) -> Self {
        Self {
            folder,
            releases,
            target,
            one_at_a_time: DownloadLock::default(),
        }
    }

    pub(super) fn folder(&self, name: &str) -> PathBuf {
        self.folder.join(name)
    }

    pub(super) fn target(&self) -> Target {
        self.target
    }

    /// Downloads what puts `file` in place unless it is there, and says so while it does.
    pub(super) fn downloaded(
        &self,
        name: &ServerName,
        file: &Path,
        waiting: &dyn Fn(Waiting),
        download: impl FnOnce() -> anyhow::Result<()>,
    ) -> Result<(), FallbackReason> {
        if file.is_file() {
            return Ok(());
        }
        waiting(Waiting::Downloading(name.clone()));
        let _turn: DownloadTurn<'_> = self.one_at_a_time.take_turn();
        if file.is_file() {
            return Ok(());
        }
        download().map_err(|error| {
            FallbackReason::new(&format!("{name} could not be downloaded: {error:#}"))
        })
    }

    pub(super) fn remove_all_but(&self, kept: &[String]) -> anyhow::Result<()> {
        let entries = match std::fs::read_dir(&self.folder) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("could not read {}", self.folder.display()));
            }
        };
        for entry in entries {
            let entry =
                entry.with_context(|| format!("could not read {}", self.folder.display()))?;
            if !kept
                .iter()
                .any(|kept| *kept == entry.file_name().to_string_lossy())
            {
                let path = entry.path();
                let removed = if path.is_dir() {
                    std::fs::remove_dir_all(&path)
                } else {
                    std::fs::remove_file(&path)
                };
                removed.with_context(|| format!("could not remove {}", path.display()))?;
            }
        }
        Ok(())
    }

    /// Nothing is unpacked from a download that fails its check.
    pub(super) fn checked_download(
        &self,
        url: &str,
        check: impl FnOnce(&[u8]) -> anyhow::Result<()>,
    ) -> anyhow::Result<Vec<u8>> {
        let archive = self
            .releases
            .download(url)
            .with_context(|| format!("could not download {url}"))?;
        check(&archive).with_context(|| format!("{url} was refused"))?;
        Ok(archive)
    }
}

/// Fills `folder` beside its place and renames it there, so a folder that exists is whole.
pub(super) fn place(
    folder: &Path,
    fill: impl FnOnce(&Path) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    let name = folder
        .file_name()
        .context("the download has no folder name")?
        .to_string_lossy();
    let unpacking = folder.with_file_name(format!("{name}.unpacking"));
    if unpacking.exists() {
        std::fs::remove_dir_all(&unpacking)
            .with_context(|| format!("could not remove {}", unpacking.display()))?;
    }
    let placed = std::fs::create_dir_all(&unpacking)
        .context("could not make the folder")
        .and_then(|()| fill(&unpacking))
        .and_then(|()| {
            std::fs::rename(&unpacking, folder)
                .with_context(|| format!("could not place {}", folder.display()))
        });
    if placed.is_err() {
        std::fs::remove_dir_all(&unpacking).ok();
    }
    placed
}

pub(super) fn write_program(file: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    std::fs::write(file, bytes)
        .and_then(|()| std::fs::set_permissions(file, std::fs::Permissions::from_mode(0o755)))
        .with_context(|| format!("could not write {}", file.display()))
}
