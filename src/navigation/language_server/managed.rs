//! The language servers the app downloads itself: pinned versions, checked by sha256, kept in the
//! cache folder.

use std::io::Read as _;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::Context as _;
use flate2::read::GzDecoder;

use crate::language::Language;
use crate::platform::Os;
use crate::update::{Cpu, ReleasesPort, Target, check_digest};

use super::{FallbackReason, ServerCatalog, ServerLaunch, ServerName, Waiting, locked};

/// One release of a server: its tag and the sha256 of its download for each target it is built for.
#[derive(Debug, Clone)]
pub struct ReleasePin {
    pub tag: String,
    pub sha256: Vec<(Target, String)>,
}

/// The server versions this build of the app knows.
#[derive(Debug, Clone)]
pub struct ServerPins {
    pub rust_analyzer: ReleasePin,
}

impl ServerPins {
    /// The versions of this build.
    pub fn pinned() -> Self {
        let target = |os, cpu, sha: &str| (Target { os, cpu }, sha.to_owned());
        Self {
            rust_analyzer: ReleasePin {
                tag: "2026-11-23".to_owned(),
                sha256: vec![
                    target(
                        Os::MacOs,
                        Cpu::Arm64,
                        "9d996784c9bc811642edd9d993c8c85e12c7111463a8e426d5e74c27da1bc0a5",
                    ),
                    target(
                        Os::MacOs,
                        Cpu::X86_64,
                        "4673e51749f937567b77813ef12621b6e11e6ba9c1b591c58f8a98fbbc9f5bac",
                    ),
                    target(
                        Os::Linux,
                        Cpu::Arm64,
                        "cf7e9c2cd34540da92473d47f0866c9515425da01116e7aaca988c9b19408285",
                    ),
                    target(
                        Os::Linux,
                        Cpu::X86_64,
                        "99748126069cc1360e2729e45ae7b563a2b885def41c0d600de8e5977a7adf13",
                    ),
                ],
            },
        }
    }
}

/// Serves the pinned servers from the cache folder, downloading one the first time it is asked for.
pub struct ManagedServers {
    folder: PathBuf,
    releases: Arc<dyn ReleasesPort>,
    pins: ServerPins,
    target: Target,
    /// Held while a server downloads, so a second hover waits for it instead of downloading again.
    downloading: Mutex<()>,
}

impl ManagedServers {
    pub fn new(
        folder: PathBuf,
        releases: Arc<dyn ReleasesPort>,
        pins: ServerPins,
        target: Target,
    ) -> Self {
        Self {
            folder,
            releases,
            pins,
            target,
            downloading: Mutex::new(()),
        }
    }

    /// Whether the server of `language` is in the cache folder, which asking for it never checks.
    pub fn is_downloaded(&self, language: Language) -> bool {
        match language {
            Language::Rust => self.rust_analyzer_binary().is_file(),
            Language::Php | Language::TypeScript | Language::Tsx => false,
        }
    }

    fn rust_analyzer_binary(&self) -> PathBuf {
        self.folder
            .join(format!("rust-analyzer-{}", self.pins.rust_analyzer.tag))
            .join("rust-analyzer")
    }

    /// Removes every download of a version this build does not pin, and what a stopped unpacking left.
    pub fn remove_unpinned(&self) -> anyhow::Result<()> {
        let kept = format!("rust-analyzer-{}", self.pins.rust_analyzer.tag);
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
            if entry.file_name().to_string_lossy() != kept {
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

    fn rust_analyzer(&self, waiting: &dyn Fn(Waiting)) -> Result<ServerLaunch, FallbackReason> {
        let name = ServerName::new("rust-analyzer");
        let pin = &self.pins.rust_analyzer;
        let binary = self.rust_analyzer_binary();
        if !binary.is_file() {
            waiting(Waiting::Downloading(name.clone()));
            let _alone = locked(&self.downloading);
            if !binary.is_file() {
                self.download_rust_analyzer(pin, &binary).map_err(|error| {
                    FallbackReason::new(&format!("{name} could not be downloaded: {error:#}"))
                })?;
            }
        }
        Ok(ServerLaunch::rust_analyzer(binary).reporting_status())
    }

    /// The release is one gzipped binary. It is checked before anything is unpacked, and unpacked
    /// beside its place, so a folder that exists holds a whole binary.
    fn download_rust_analyzer(&self, pin: &ReleasePin, binary: &Path) -> anyhow::Result<()> {
        let sha256 = pin
            .sha256
            .iter()
            .find(|(target, _)| *target == self.target)
            .map(|(_, sha256)| sha256)
            .with_context(|| format!("none is pinned for {}", self.target.triple()))?;
        let url = format!(
            "https://github.com/rust-lang/rust-analyzer/releases/download/{}/rust-analyzer-{}.gz",
            pin.tag,
            self.target.triple()
        );
        let archive = self
            .releases
            .download(&url)
            .with_context(|| format!("could not download {url}"))?;
        check_digest(&archive, sha256).with_context(|| format!("{url} was refused"))?;
        let mut unpacked = Vec::new();
        GzDecoder::new(archive.as_slice())
            .read_to_end(&mut unpacked)
            .with_context(|| format!("could not unpack {url}"))?;
        let folder = binary.parent().context("the binary has no folder")?;
        let unpacking = folder.with_extension("unpacking");
        if unpacking.exists() {
            std::fs::remove_dir_all(&unpacking)
                .with_context(|| format!("could not remove {}", unpacking.display()))?;
        }
        let placed = std::fs::create_dir_all(&unpacking)
            .and_then(|()| {
                let file = unpacking.join("rust-analyzer");
                std::fs::write(&file, &unpacked)?;
                std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755))?;
                std::fs::rename(&unpacking, folder)
            })
            .with_context(|| format!("could not place {}", folder.display()));
        if placed.is_err() {
            std::fs::remove_dir_all(&unpacking).ok();
        }
        placed
    }
}

impl ServerCatalog for ManagedServers {
    fn launch_for(
        &self,
        language: Language,
        waiting: &dyn Fn(Waiting),
    ) -> Result<ServerLaunch, FallbackReason> {
        match language {
            Language::Rust => self.rust_analyzer(waiting),
            Language::Php | Language::TypeScript | Language::Tsx => {
                Err(FallbackReason::new(&format!(
                    "{} is not downloaded by the app yet",
                    ServerName::of(language)
                )))
            }
        }
    }
}
