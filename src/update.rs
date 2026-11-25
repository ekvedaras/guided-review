//! Updates: finding a newer published release of the app and installing it in place of this one.

mod release_install;

use anyhow::Context;

use crate::platform::Os;
use crate::version::Version;

pub use release_install::{
    InTheWay, Installed, ReleaseInstall, SkillLink, check_digest, running_build,
};

/// The build of this binary as `--version` prints it and the app shows it: the release version and
/// the commit it was built from, marked `-dirty` when built with uncommitted changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppVersion {
    shown: &'static str,
    /// The tag of the release this build is, when it was built clean from the commit of that tag.
    release_tag: Option<&'static str>,
}

impl AppVersion {
    pub const RUNNING: Self = Self::new(
        concat!(
            env!("CARGO_PKG_VERSION"),
            " (",
            env!("GUIDED_REVIEW_GIT"),
            ")"
        ),
        if env!("GUIDED_REVIEW_RELEASE_TAG").is_empty() {
            None
        } else {
            Some(env!("GUIDED_REVIEW_RELEASE_TAG"))
        },
    );

    pub const fn new(shown: &'static str, release_tag: Option<&'static str>) -> Self {
        Self { shown, release_tag }
    }

    pub fn as_str(self) -> &'static str {
        self.shown
    }

    /// Where a click on the version leads: the release's own page, or for a source build that is no
    /// release, the list of releases, as its commit may never have been pushed.
    pub fn page(self) -> String {
        let releases = format!("{}/releases", env!("CARGO_PKG_REPOSITORY"));
        match self.release_tag {
            Some(tag) => format!("{releases}/tag/{tag}"),
            None => releases,
        }
    }
}

impl std::fmt::Display for AppVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.shown)
    }
}

/// The OS and CPU a release archive is built for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Target {
    pub os: Os,
    pub cpu: Cpu,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cpu {
    Arm64,
    X86_64,
}

impl Target {
    pub fn running() -> Self {
        Self {
            os: Os::RUNNING,
            cpu: if cfg!(target_arch = "x86_64") {
                Cpu::X86_64
            } else {
                Cpu::Arm64
            },
        }
    }

    pub fn triple(self) -> String {
        let arch = match self.cpu {
            Cpu::Arm64 => "aarch64",
            Cpu::X86_64 => "x86_64",
        };
        format!("{arch}-{}", self.os.triple_system())
    }
}

/// The newest release as GitHub publishes it. Drafts and pre-releases are never it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishedRelease {
    pub tag: String,
    pub assets: Vec<ReleaseAsset>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseAsset {
    pub name: String,
    pub url: String,
}

/// A newer release this OS and CPU can install: its archive and the archive's checksum file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Update {
    pub version: Version,
    pub target: Target,
    pub archive: ReleaseAsset,
    pub checksum: ReleaseAsset,
}

impl Update {
    pub fn offered(running: Version, latest: &PublishedRelease, target: Target) -> Option<Self> {
        let version = Version::from_tag(&latest.tag).filter(|version| *version > running)?;
        let archive_name = archive_name(version, target);
        let checksum_name = format!("{archive_name}.sha256");
        let asset_named = |name: &str| latest.assets.iter().find(|asset| asset.name == name);
        Some(Self {
            version,
            target,
            archive: asset_named(&archive_name)?.clone(),
            checksum: asset_named(&checksum_name)?.clone(),
        })
    }

    /// The folder the archive unpacks to, named like the archive.
    pub fn unpacked_folder(&self) -> String {
        format!("guided-review-{}-{}", self.version, self.target.triple())
    }
}

/// Where releases come from: GitHub's releases of the app's repository.
pub trait ReleasesPort: Send + Sync {
    /// The newest published release. Drafts and pre-releases are not published.
    fn latest_release(&self) -> anyhow::Result<PublishedRelease>;

    fn download(&self, url: &str) -> anyhow::Result<Vec<u8>>;
}

/// The latest release, when it is newer than `running` and built for `target`.
pub fn find_update(
    releases: &dyn ReleasesPort,
    running: Version,
    target: Target,
) -> anyhow::Result<Option<Update>> {
    let latest = releases
        .latest_release()
        .context("could not read the latest release")?;
    Ok(Update::offered(running, &latest, target))
}

/// What the app offers the reviewer: an update to install, the install under way, or one installed
/// that a restart starts.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum UpdateOffer {
    #[default]
    Nothing,
    Available(Update),
    Installing(Update),
    Installed(Installed),
}

impl UpdateOffer {
    /// A check's answer replaces what is offered, unless an install is under way or done.
    pub fn found(&mut self, update: Option<Update>) {
        if let Self::Nothing | Self::Available(_) = self {
            *self = update.map_or(Self::Nothing, Self::Available);
        }
    }

    pub fn start_install(&mut self) -> Option<Update> {
        let Self::Available(update) = self else {
            return None;
        };
        let update = update.clone();
        *self = Self::Installing(update.clone());
        Some(update)
    }

    /// A failed install offers the same update again and passes the failure on.
    pub fn install_ended(&mut self, result: anyhow::Result<Installed>) -> anyhow::Result<()> {
        let Self::Installing(update) = self else {
            anyhow::bail!("no update was being installed");
        };
        match result {
            Ok(installed) => {
                *self = Self::Installed(installed);
                Ok(())
            }
            Err(error) => {
                *self = Self::Available(update.clone());
                Err(error)
            }
        }
    }
}

fn archive_name(version: Version, target: Target) -> String {
    format!("guided-review-{version}-{}.tar.gz", target.triple())
}
