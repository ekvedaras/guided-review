//! The language servers the app downloads itself, and the Node that runs the npm ones: pinned
//! versions, checked by sha256 (sha512 for npm packages), kept in the cache folder.

use std::io::Read as _;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::Context as _;
use flate2::read::GzDecoder;
use sha2::{Digest as _, Sha512};

use crate::language::Language;
use crate::platform::Os;
use crate::pull_request::RepoName;
use crate::update::{Cpu, ReleasesPort, Target, check_digest};

/// Where the package's server is, inside its download folder.
const INTELEPHENSE_SCRIPT: &str = "package/lib/intelephense.js";

use super::{FallbackReason, ServerCatalog, ServerLaunch, ServerName, Waiting, locked};

/// One release of a server: its tag and the sha256 of its download for each target it is built for.
#[derive(Debug, Clone)]
pub struct ReleasePin {
    pub tag: String,
    pub sha256: Vec<(Target, String)>,
}

/// One version of an npm package and the sha512 of its registry tarball, in hex: the hash npm's
/// `integrity` gives in base64.
#[derive(Debug, Clone)]
pub struct PackagePin {
    pub version: String,
    pub sha512: String,
}

/// The server versions this build of the app knows.
#[derive(Debug, Clone)]
pub struct ServerPins {
    pub rust_analyzer: ReleasePin,
    /// Runs the servers that are npm packages.
    pub node: ReleasePin,
    pub intelephense: PackagePin,
}

/// Where managed servers keep what they need: downloads can be fetched again, indexes are costly to
/// rebuild.
#[derive(Debug, Clone)]
pub struct ServerFolders {
    /// One folder per server and pinned version.
    pub downloads: PathBuf,
    /// One folder per owner and repo, kept across idle stops and app restarts.
    pub indexes: PathBuf,
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
            node: ReleasePin {
                tag: "v26.13.1".to_owned(),
                sha256: vec![
                    target(
                        Os::MacOs,
                        Cpu::Arm64,
                        "aec2514ea1ceb2ba52455e9ec65986f6749a06dd759e5db2a72e6889668c9735",
                    ),
                    target(
                        Os::MacOs,
                        Cpu::X86_64,
                        "82bb96078a24ca50b9c9a38f2379e6be6d87171fcd400fc14c8934b3bce96e2f",
                    ),
                    target(
                        Os::Linux,
                        Cpu::Arm64,
                        "80a330b35415382878df18d41afbd106ea89bfb3be0087d6608e289a14c5cc7d",
                    ),
                    target(
                        Os::Linux,
                        Cpu::X86_64,
                        "518674de2b621728a5e4486f5fb3b7d2900bb3cc4aa97969edaa463db99f9778",
                    ),
                ],
            },
            intelephense: PackagePin {
                version: "1.18.5".to_owned(),
                sha512: "76a087d583424651c604b343fa25058c12651b0338a4f8a65f68e6f0068fffa2b659934cd8adbe13c74e5820f765830fdb30b6202043f52a1eda8c4ab54f5df4".to_owned(),
            },
        }
    }
}

/// Serves the pinned servers from the cache folder, downloading one the first time it is asked for.
pub struct ManagedServers {
    folder: PathBuf,
    indexes: PathBuf,
    releases: Arc<dyn ReleasesPort>,
    pins: ServerPins,
    target: Target,
    /// Held while a server downloads, so a second hover waits for it instead of downloading again.
    downloading: Mutex<()>,
}

impl ManagedServers {
    pub fn new(
        folders: ServerFolders,
        releases: Arc<dyn ReleasesPort>,
        pins: ServerPins,
        target: Target,
    ) -> Self {
        Self {
            folder: folders.downloads,
            indexes: folders.indexes,
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
            Language::Php => self.node_binary().is_file() && self.intelephense_script().is_file(),
            Language::TypeScript | Language::Tsx => false,
        }
    }

    fn rust_analyzer_folder(&self) -> String {
        format!("rust-analyzer-{}", self.pins.rust_analyzer.tag)
    }

    fn node_folder(&self) -> String {
        format!("node-{}", self.pins.node.tag)
    }

    fn intelephense_folder(&self) -> String {
        format!("intelephense-{}", self.pins.intelephense.version)
    }

    fn rust_analyzer_binary(&self) -> PathBuf {
        self.folder
            .join(self.rust_analyzer_folder())
            .join("rust-analyzer")
    }

    fn node_binary(&self) -> PathBuf {
        self.folder.join(self.node_folder()).join("node")
    }

    fn intelephense_script(&self) -> PathBuf {
        self.folder
            .join(self.intelephense_folder())
            .join(INTELEPHENSE_SCRIPT)
    }

    /// Removes every download of a version this build does not pin, and what a stopped unpacking left.
    pub fn remove_unpinned(&self) -> anyhow::Result<()> {
        let kept = [
            self.rust_analyzer_folder(),
            self.node_folder(),
            self.intelephense_folder(),
        ];
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

    fn rust_analyzer(&self, waiting: &dyn Fn(Waiting)) -> Result<ServerLaunch, FallbackReason> {
        let binary = self.rust_analyzer_binary();
        self.downloaded(&ServerName::new("rust-analyzer"), &binary, waiting, || {
            self.download_rust_analyzer(&binary)
        })?;
        Ok(ServerLaunch::rust_analyzer(binary).reporting_status())
    }

    /// Intelephense keeps its index of the repo in a folder of its own, so a restart reads it back.
    fn intelephense(
        &self,
        repo: &RepoName,
        waiting: &dyn Fn(Waiting),
    ) -> Result<ServerLaunch, FallbackReason> {
        let node = self.node_binary();
        self.downloaded(&ServerName::new("Node"), &node, waiting, || {
            self.download_node(&node)
        })?;
        let script = self.intelephense_script();
        self.downloaded(&ServerName::of(Language::Php), &script, waiting, || {
            self.download_intelephense(&script)
        })?;
        let index = self
            .indexes
            .join(repo.owner())
            .join(repo.name())
            .join("intelephense");
        Ok(ServerLaunch::intelephense(node, &script, &index))
    }

    /// Downloads what puts `file` in place unless it is there, and says so while it does.
    fn downloaded(
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
        let _alone = locked(&self.downloading);
        if file.is_file() {
            return Ok(());
        }
        download().map_err(|error| {
            FallbackReason::new(&format!("{name} could not be downloaded: {error:#}"))
        })
    }

    fn pinned_sha256<'a>(&self, pin: &'a ReleasePin) -> anyhow::Result<&'a str> {
        pin.sha256
            .iter()
            .find(|(target, _)| *target == self.target)
            .map(|(_, sha256)| sha256.as_str())
            .with_context(|| format!("none is pinned for {}", self.target.triple()))
    }

    /// The release is one gzipped binary.
    fn download_rust_analyzer(&self, binary: &Path) -> anyhow::Result<()> {
        let pin = &self.pins.rust_analyzer;
        let url = format!(
            "https://github.com/rust-lang/rust-analyzer/releases/download/{}/rust-analyzer-{}.gz",
            pin.tag,
            self.target.triple()
        );
        let archive = self.checked_download(&url, |archive| {
            check_digest(archive, self.pinned_sha256(pin)?)
        })?;
        let mut unpacked = Vec::new();
        GzDecoder::new(archive.as_slice())
            .read_to_end(&mut unpacked)
            .with_context(|| format!("could not unpack {url}"))?;
        let folder = binary.parent().context("the binary has no folder")?;
        place(folder, |unpacking| {
            write_program(&unpacking.join("rust-analyzer"), &unpacked)
        })
    }

    /// Of Node's release archive only the `node` program is kept.
    fn download_node(&self, binary: &Path) -> anyhow::Result<()> {
        let pin = &self.pins.node;
        let platform = match self.target.os {
            Os::MacOs => "darwin",
            Os::Linux => "linux",
        };
        let arch = match self.target.cpu {
            Cpu::Arm64 => "arm64",
            Cpu::X86_64 => "x64",
        };
        let url = format!(
            "https://nodejs.org/dist/{tag}/node-{tag}-{platform}-{arch}.tar.gz",
            tag = pin.tag
        );
        let archive = self.checked_download(&url, |archive| {
            check_digest(archive, self.pinned_sha256(pin)?)
        })?;
        let mut node = None;
        let mut entries = tar::Archive::new(GzDecoder::new(archive.as_slice()));
        for entry in entries
            .entries()
            .with_context(|| format!("could not unpack {url}"))?
        {
            let mut entry = entry.with_context(|| format!("could not unpack {url}"))?;
            let path = entry
                .path()
                .with_context(|| format!("could not unpack {url}"))?;
            if path.ends_with("bin/node") && path.components().count() == 3 {
                let mut bytes = Vec::new();
                entry
                    .read_to_end(&mut bytes)
                    .with_context(|| format!("could not unpack {url}"))?;
                node = Some(bytes);
                break;
            }
        }
        let node = node.with_context(|| format!("{url} has no bin/node"))?;
        let folder = binary.parent().context("the binary has no folder")?;
        place(folder, |unpacking| {
            write_program(&unpacking.join("node"), &node)
        })
    }

    /// The registry tarball holds the package under `package/`. Intelephense is bundled into one
    /// script, so it needs no packages of its own.
    fn download_intelephense(&self, script: &Path) -> anyhow::Result<()> {
        let pin = &self.pins.intelephense;
        let url = format!(
            "https://registry.npmjs.org/intelephense/-/intelephense-{}.tgz",
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
            anyhow::ensure!(
                unpacking.join(INTELEPHENSE_SCRIPT).is_file(),
                "{url} has no {INTELEPHENSE_SCRIPT}"
            );
            Ok(())
        })
    }

    /// Nothing is unpacked from a download that fails its check.
    fn checked_download(
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
fn place(folder: &Path, fill: impl FnOnce(&Path) -> anyhow::Result<()>) -> anyhow::Result<()> {
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

fn write_program(file: &Path, bytes: &[u8]) -> anyhow::Result<()> {
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

impl ServerCatalog for ManagedServers {
    fn launch_for(
        &self,
        language: Language,
        repo: &RepoName,
        waiting: &dyn Fn(Waiting),
    ) -> Result<ServerLaunch, FallbackReason> {
        match language {
            Language::Rust => self.rust_analyzer(waiting),
            Language::Php => self.intelephense(repo, waiting),
            Language::TypeScript | Language::Tsx => Err(FallbackReason::new(&format!(
                "{} is not downloaded by the app yet",
                ServerName::of(language)
            ))),
        }
    }
}
