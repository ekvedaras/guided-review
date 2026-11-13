use std::collections::HashMap;
use std::sync::Arc;

/// Downloads the picture at a comment author's avatar address.
pub trait AvatarPort: Send + Sync {
    fn avatar(&self, url: &str) -> anyhow::Result<Vec<u8>>;
}

/// The comment authors' pictures, by their GitHub avatar address, downloaded once each per app run.
/// One that failed stays a placeholder: avatars are not worth a retry or a problem.
#[derive(Debug, Default)]
pub struct Avatars {
    by_url: HashMap<String, AvatarState>,
}

#[derive(Debug)]
enum AvatarState {
    Downloading,
    Downloaded(Arc<[u8]>),
    Failed,
}

impl Avatars {
    /// The download to run, the first time an address is wanted.
    pub fn want(&mut self, url: &str) -> Option<AvatarRequest> {
        if self.by_url.contains_key(url) {
            return None;
        }
        self.by_url.insert(url.to_owned(), AvatarState::Downloading);
        Some(AvatarRequest {
            url: url.to_owned(),
        })
    }

    pub fn landed(&mut self, outcome: AvatarOutcome) {
        let state = match outcome.bytes {
            Some(bytes) => AvatarState::Downloaded(bytes.into()),
            None => AvatarState::Failed,
        };
        self.by_url.insert(outcome.url, state);
    }

    /// The picture's bytes, once downloaded.
    pub fn get(&self, url: &str) -> Option<&Arc<[u8]>> {
        match self.by_url.get(url)? {
            AvatarState::Downloaded(bytes) => Some(bytes),
            AvatarState::Downloading | AvatarState::Failed => None,
        }
    }
}

/// One download, owned so it can run off the UI thread.
pub struct AvatarRequest {
    url: String,
}

pub struct AvatarOutcome {
    url: String,
    bytes: Option<Vec<u8>>,
}

impl AvatarOutcome {
    pub fn url(&self) -> &str {
        &self.url
    }
}

impl AvatarRequest {
    pub fn run(self, port: &dyn AvatarPort) -> AvatarOutcome {
        AvatarOutcome {
            bytes: port.avatar(&self.url).ok(),
            url: self.url,
        }
    }
}
