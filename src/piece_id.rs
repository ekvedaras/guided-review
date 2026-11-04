use std::cmp::Ordering;
use std::fmt;
use std::str::FromStr;
use std::sync::Arc;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::file_path::FilePath;

/// `app/Foo.php#Foo::handle`, or just `app/Foo.php` for a whole-file piece.
/// Symbol paths never hold `#`, `@` or `/`, so a sub-part can later be appended
/// after one of them without changing the ids old tour files point at.
/// `hunk-N` symbols stand for files that could not be cut into symbols.
///
/// A state file holds the id as text. A whole file whose name holds a `#` is written with a
/// trailing `#` (`docs/C#.md#`), so it cannot be read as the symbol `.md` of `docs/C`.
/// Ids order by that text, so a saved state lists them as files written before did.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PieceId {
    path: FilePath,
    symbol: Option<Arc<str>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidPieceId(String);

impl fmt::Display for InvalidPieceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?} is not a piece id", self.0)
    }
}

impl std::error::Error for InvalidPieceId {}

const HUNK_PREFIX: &str = "hunk-";

impl PieceId {
    pub fn whole_file(path: FilePath) -> Self {
        Self { path, symbol: None }
    }

    pub fn symbol(path: FilePath, symbol: impl Into<String>) -> Result<Self, InvalidPieceId> {
        let symbol = symbol.into();
        if symbol.is_empty() || symbol.contains(['#', '@', '/']) {
            return Err(InvalidPieceId(format!("{path}#{symbol}")));
        }
        Ok(Self {
            path,
            symbol: Some(symbol.into()),
        })
    }

    /// The `number`th change of a file that could not be cut into symbols, counting from 1.
    pub fn hunk(path: FilePath, number: usize) -> Self {
        Self {
            path,
            symbol: Some(format!("{HUNK_PREFIX}{number}").into()),
        }
    }

    pub fn path(&self) -> &FilePath {
        &self.path
    }

    pub fn symbol_path(&self) -> Option<&str> {
        self.symbol.as_deref()
    }

    /// The folder with its trailing slash, or empty at the repo root.
    pub fn folder(&self) -> &str {
        let path = self.path.as_str();
        path.rfind('/')
            .and_then(|slash| path.get(..=slash))
            .unwrap_or_default()
    }

    pub fn file_name(&self) -> &str {
        let path = self.path.as_str();
        path.rsplit_once('/').map_or(path, |(_, name)| name)
    }

    /// The last part of the symbol path: `total` of `Order::total`.
    pub fn symbol_name(&self) -> Option<&str> {
        self.symbol
            .as_deref()
            .map(|symbol| symbol.rsplit("::").next().unwrap_or(symbol))
    }

    /// `#Foo::handle` under its file header in the outline; a whole-file piece keeps its path.
    pub fn within_file(&self) -> String {
        match &self.symbol {
            Some(symbol) => format!("#{symbol}"),
            None => self.path.to_string(),
        }
    }

    /// Whether `other` may name this piece: it is this id, or one of the two was read from a
    /// state file written before whole-file ids were marked, where the whole file `docs/C#.md`
    /// had the same text as the symbol `.md` of `docs/C`.
    pub fn may_be(&self, other: &Self) -> bool {
        self == other
            || self.is_read_from_plain_text_of(other)
            || other.is_read_from_plain_text_of(self)
    }

    fn is_read_from_plain_text_of(&self, whole: &Self) -> bool {
        whole.symbol.is_none()
            && whole.path.as_str().contains('#')
            && whole.path.as_str().parse::<Self>().ok().as_ref() == Some(self)
    }

    /// The id's text in a state file, in parts: path, separator, symbol.
    fn written_parts(&self) -> [&str; 3] {
        match &self.symbol {
            Some(symbol) => [self.path.as_str(), "#", symbol],
            None if self.path.as_str().contains('#') => [self.path.as_str(), "#", ""],
            None => [self.path.as_str(), "", ""],
        }
    }

    pub fn is_hunk(&self) -> bool {
        self.symbol.as_deref().is_some_and(|symbol| {
            symbol.strip_prefix(HUNK_PREFIX).is_some_and(|number| {
                !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit())
            })
        })
    }
}

impl fmt::Display for PieceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.symbol {
            Some(symbol) => write!(f, "{}#{symbol}", self.path),
            None => fmt::Display::fmt(&self.path, f),
        }
    }
}

impl FromStr for PieceId {
    type Err = InvalidPieceId;

    /// Reads the text a state file holds. A trailing `#` marks a whole file; otherwise the text
    /// splits at its last `#` when both sides are valid, so a folder may hold a `#`. What is
    /// left is a whole file, as text written before the mark was: `icons/a#1@2x.png`, `#notes.md`.
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let marked_whole_file = text
            .strip_suffix('#')
            .and_then(|path| FilePath::new(path).ok())
            .map(Self::whole_file);
        let symbol = || {
            let (path, symbol) = text.rsplit_once('#')?;
            Self::symbol(FilePath::new(path).ok()?, symbol).ok()
        };
        let whole_file = || FilePath::new(text).ok().map(Self::whole_file);
        marked_whole_file
            .or_else(symbol)
            .or_else(whole_file)
            .ok_or_else(|| InvalidPieceId(text.to_owned()))
    }
}

impl Ord for PieceId {
    fn cmp(&self, other: &Self) -> Ordering {
        let mine = self.written_parts().into_iter().flat_map(str::bytes);
        mine.cmp(other.written_parts().into_iter().flat_map(str::bytes))
    }
}

impl PartialOrd for PieceId {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Serialize for PieceId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.written_parts().concat())
    }
}

impl<'de> Deserialize<'de> for PieceId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}
