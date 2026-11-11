//! Updates: finding a newer published release of the app and installing it in place of this one.

mod release_install;

use std::str::FromStr;

use anyhow::Context;

pub use release_install::{Installed, ReleaseInstall, SkillLink};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    major: u32,
    minor: u32,
    patch: u32,
}

impl Version {
    pub fn running() -> Self {
        Self::parse(env!("CARGO_PKG_VERSION")).unwrap_or(Self {
            major: 0,
            minor: 0,
            patch: 0,
        })
    }

    /// A release tag is `v` and a version. Pre-release tags name no version, so they are never offered.
    pub fn from_tag(tag: &str) -> Option<Self> {
        tag.strip_prefix('v').and_then(Self::parse)
    }

    fn parse(text: &str) -> Option<Self> {
        let mut numbers = text.split('.').map(|part| {
            part.bytes()
                .all(|byte| byte.is_ascii_digit())
                .then(|| part.parse::<u32>().ok())
                .flatten()
        });
        let version = Self {
            major: numbers.next()??,
            minor: numbers.next()??,
            patch: numbers.next()??,
        };
        numbers.next().is_none().then_some(version)
    }
}

impl FromStr for Version {
    type Err = anyhow::Error;

    fn from_str(text: &str) -> anyhow::Result<Self> {
        Self::parse(text).with_context(|| format!("{text:?} is not a version like 1.2.3"))
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// Which Mac a release archive is built for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    AppleSilicon,
    Intel,
}

impl Target {
    pub fn running() -> Self {
        if cfg!(target_arch = "x86_64") {
            Self::Intel
        } else {
            Self::AppleSilicon
        }
    }

    pub fn triple(self) -> &'static str {
        match self {
            Self::AppleSilicon => "aarch64-apple-darwin",
            Self::Intel => "x86_64-apple-darwin",
        }
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

/// A newer release this Mac can install: its archive and the archive's checksum file.
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
