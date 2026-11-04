use std::borrow::Borrow;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A file's path in the repo, relative to its root, with `/` separators.
/// A clone is a refcount, so a page of rows names its file without copying the text.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FilePath(Arc<str>);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidFilePath(String);

impl fmt::Display for InvalidFilePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?} is not a path relative to the repo root", self.0)
    }
}

impl std::error::Error for InvalidFilePath {}

impl FilePath {
    /// Refuses what cannot be a repo-relative path. `..` is left alone: git never writes it.
    pub fn new(path: &str) -> Result<Self, InvalidFilePath> {
        if path.is_empty() || path.starts_with('/') || path.contains('\\') {
            return Err(InvalidFilePath(path.to_owned()));
        }
        Ok(Self(path.into()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Where the file is in a clone rooted at `root`.
    pub fn under(&self, root: &Path) -> PathBuf {
        root.join(&*self.0)
    }
}

impl fmt::Display for FilePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Borrow<str> for FilePath {
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl PartialEq<str> for FilePath {
    fn eq(&self, other: &str) -> bool {
        &*self.0 == other
    }
}

impl PartialEq<String> for FilePath {
    fn eq(&self, other: &String) -> bool {
        &*self.0 == other.as_str()
    }
}

impl PartialEq<&str> for FilePath {
    fn eq(&self, other: &&str) -> bool {
        &*self.0 == *other
    }
}

impl Serialize for FilePath {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for FilePath {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::new(&text).map_err(serde::de::Error::custom)
    }
}

impl From<FilePath> for Arc<str> {
    fn from(path: FilePath) -> Self {
        path.0
    }
}
