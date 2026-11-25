use std::str::FromStr;

use anyhow::Context;

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
