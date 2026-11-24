//! The language servers the app downloads itself, and the Node that runs the npm ones: pinned
//! versions, checked by sha256 (sha512 for npm packages), kept in the cache folder.

use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use anyhow::Context as _;
use flate2::read::GzDecoder;
use sha2::{Digest as _, Sha512};

use crate::codebase::Codebase;
use crate::language::Language;
use crate::update::{ReleasesPort, Target};

use super::recipes::{
    Intelephense, Node, NpmPackage, PackagePin, RustAnalyzer, ServerPins, ServerRecipe,
    TypeScriptLanguageServer,
};
use super::{FallbackReason, ServerCatalog, ServerLaunch, ServerName, Waiting, locked};

/// Where managed servers keep what they need: downloads can be fetched again, indexes are costly to
/// rebuild.
#[derive(Debug, Clone)]
pub struct ServerFolders {
    /// One folder per server and pinned version.
    pub downloads: PathBuf,
    /// One folder per owner and repo, kept across idle stops and app restarts.
    pub indexes: PathBuf,
}

/// Serves the pinned servers from the cache folder, downloading one the first time it is asked for.
pub struct ManagedServers {
    downloads: Downloads,
    rust_analyzer: RustAnalyzer,
    intelephense: Intelephense,
    typescript_language_server: TypeScriptLanguageServer,
}

impl ManagedServers {
    pub fn new(
        folders: ServerFolders,
        releases: Arc<dyn ReleasesPort>,
        pins: ServerPins,
        target: Target,
    ) -> Self {
        let node = Node { pin: pins.node };
        Self {
            downloads: Downloads {
                folder: folders.downloads,
                releases,
                target,
                one_at_a_time: DownloadLock::default(),
            },
            rust_analyzer: RustAnalyzer {
                pin: pins.rust_analyzer,
            },
            intelephense: Intelephense {
                node: node.clone(),
                package: NpmPackage {
                    name: "intelephense",
                    pin: pins.intelephense,
                    script: "package/lib/intelephense.js",
                },
                indexes: folders.indexes,
            },
            typescript_language_server: TypeScriptLanguageServer {
                node,
                server: NpmPackage {
                    name: "typescript-language-server",
                    pin: pins.typescript_language_server,
                    script: "package/lib/cli.mjs",
                },
                typescript: NpmPackage {
                    name: "typescript",
                    pin: pins.typescript,
                    script: "package/lib/tsserver.js",
                },
            },
        }
    }

    fn recipe(&self, language: Language) -> &dyn ServerRecipe {
        match language {
            Language::Rust => &self.rust_analyzer,
            Language::Php => &self.intelephense,
            Language::TypeScript | Language::Tsx => &self.typescript_language_server,
        }
    }

    fn recipes(&self) -> [&dyn ServerRecipe; 3] {
        [
            &self.rust_analyzer,
            &self.intelephense,
            &self.typescript_language_server,
        ]
    }

    /// Whether the server of `language` is in the cache folder, which asking for it never checks.
    pub fn is_downloaded(&self, language: Language) -> bool {
        self.recipe(language).is_downloaded(&self.downloads)
    }

    /// Removes every download of a version this build does not pin, and what a stopped unpacking left.
    pub fn remove_unpinned(&self) -> anyhow::Result<()> {
        let kept: Vec<String> = self
            .recipes()
            .iter()
            .flat_map(|recipe| recipe.folders())
            .collect();
        self.downloads.remove_all_but(&kept)
    }
}

impl ServerCatalog for ManagedServers {
    fn launch_for(
        &self,
        language: Language,
        codebase: &Codebase,
        waiting: &dyn Fn(Waiting),
    ) -> Result<ServerLaunch, FallbackReason> {
        self.recipe(language)
            .launch(&self.downloads, codebase, waiting)
    }
}

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

    fn remove_all_but(&self, kept: &[String]) -> anyhow::Result<()> {
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

    /// The registry tarball holds the package under `package/`, and `script` is in it. Intelephense,
    /// typescript-language-server and typescript are each bundled with no dependencies, so no npm
    /// installs anything.
    pub(super) fn download_package(
        &self,
        package: &str,
        pin: &PackagePin,
        script: &Path,
    ) -> anyhow::Result<()> {
        let url = format!(
            "https://registry.npmjs.org/{package}/-/{package}-{}.tgz",
            pin.version
        );
        let archive = self.checked_download(&url, |archive| check_sha512(archive, &pin.sha512))?;
        let folder = script
            .ancestors()
            .nth(3)
            .context("the script has no package folder")?;
        place(folder, |unpacking| {
            tar::Archive::new(GzDecoder::new(archive.as_slice()))
                .unpack(unpacking)
                .with_context(|| format!("could not unpack {url}"))?;
            let inside = script.strip_prefix(folder)?;
            anyhow::ensure!(
                unpacking.join(inside).is_file(),
                "{url} has no {}",
                inside.display()
            );
            Ok(())
        })
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

/// Fails unless `bytes` has the sha512 `expected`, in hex.
fn check_sha512(bytes: &[u8], expected: &str) -> anyhow::Result<()> {
    let actual = crate::hex::lower(&Sha512::digest(bytes));
    anyhow::ensure!(
        actual.eq_ignore_ascii_case(expected),
        "the archive does not match its checksum: {actual} is not {expected}"
    );
    Ok(())
}
