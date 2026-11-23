//! Parts of the state file this build cannot read, most often written by a newer build: they are left
//! out of what the review shows, and written back as they were, so one value of an unknown shape costs
//! that value only, never the rest of the review.

use std::ops::{Deref, DerefMut};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

/// A list whose entries are read one by one: an entry this build cannot read is kept aside, as written,
/// and goes back to the file after the ones it reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeptEntries<T> {
    read: Vec<T>,
    unread: Vec<Value>,
}

impl<T> KeptEntries<T> {
    /// Counts the unread entries too, so a list holding only those is still written.
    pub const fn is_empty(&self) -> bool {
        self.read.is_empty() && self.unread.is_empty()
    }

    pub fn unread_count(&self) -> usize {
        self.unread.len()
    }
}

impl<T> Default for KeptEntries<T> {
    fn default() -> Self {
        Self {
            read: Vec::new(),
            unread: Vec::new(),
        }
    }
}

impl<T> From<Vec<T>> for KeptEntries<T> {
    fn from(read: Vec<T>) -> Self {
        Self {
            read,
            unread: Vec::new(),
        }
    }
}

impl<T> FromIterator<T> for KeptEntries<T> {
    fn from_iter<I: IntoIterator<Item = T>>(entries: I) -> Self {
        Self::from(entries.into_iter().collect::<Vec<T>>())
    }
}

impl<T> Deref for KeptEntries<T> {
    type Target = Vec<T>;

    fn deref(&self) -> &Vec<T> {
        &self.read
    }
}

impl<T> DerefMut for KeptEntries<T> {
    fn deref_mut(&mut self) -> &mut Vec<T> {
        &mut self.read
    }
}

impl<T: Serialize> Serialize for KeptEntries<T> {
    fn serialize<S: Serializer>(&self, to: S) -> Result<S::Ok, S::Error> {
        let mut entries = Vec::with_capacity(self.read.len() + self.unread.len());
        for entry in &self.read {
            entries.push(serde_json::to_value(entry).map_err(serde::ser::Error::custom)?);
        }
        entries.extend(self.unread.iter().cloned());
        entries.serialize(to)
    }
}

impl<'de, T: DeserializeOwned> Deserialize<'de> for KeptEntries<T> {
    fn deserialize<D: Deserializer<'de>>(from: D) -> Result<Self, D::Error> {
        let mut kept = Self::default();
        for entry in Vec::<Value>::deserialize(from)? {
            match T::deserialize(&entry) {
                Ok(read) => kept.read.push(read),
                Err(_unknown_shape) => kept.unread.push(entry),
            }
        }
        Ok(kept)
    }
}

/// A section of the state file read as a whole: one this build cannot read starts empty here and is
/// written back as it was, while this build's changes to it are not kept.
#[derive(Debug, Clone, Default)]
pub struct KeptSection<T> {
    value: T,
    unread: Option<Value>,
}

impl<T> KeptSection<T> {
    pub const fn is_unread(&self) -> bool {
        self.unread.is_some()
    }

    /// Skips the section in the file when it holds nothing this build reads or keeps.
    pub fn skips_when(&self, is_empty: impl FnOnce(&T) -> bool) -> bool {
        self.unread.is_none() && is_empty(&self.value)
    }
}

impl<T> From<T> for KeptSection<T> {
    fn from(value: T) -> Self {
        Self {
            value,
            unread: None,
        }
    }
}

impl<T> Deref for KeptSection<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.value
    }
}

impl<T> DerefMut for KeptSection<T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.value
    }
}

impl<T: Serialize> Serialize for KeptSection<T> {
    fn serialize<S: Serializer>(&self, to: S) -> Result<S::Ok, S::Error> {
        match &self.unread {
            Some(as_written) => as_written.serialize(to),
            None => self.value.serialize(to),
        }
    }
}

impl<'de, T: DeserializeOwned + Default> Deserialize<'de> for KeptSection<T> {
    fn deserialize<D: Deserializer<'de>>(from: D) -> Result<Self, D::Error> {
        let as_written = Value::deserialize(from)?;
        Ok(match T::deserialize(&as_written) {
            Ok(value) => Self::from(value),
            Err(_unknown_shape) => Self {
                value: T::default(),
                unread: Some(as_written),
            },
        })
    }
}
