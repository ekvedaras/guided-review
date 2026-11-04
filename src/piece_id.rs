use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::file_path::FilePath;

/// `app/Foo.php#Foo::handle`, or just `app/Foo.php` for a whole-file piece.
/// Symbol paths never hold `#`, `@` or `/`, so a sub-part can later be appended
/// after one of them without changing the ids old tour files point at.
/// `hunk-N` symbols stand for files that could not be cut into symbols.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PieceId {
    path: FilePath,
    symbol: Option<String>,
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
            symbol: Some(symbol),
        })
    }

    /// The `number`th change of a file that could not be cut into symbols, counting from 1.
    pub fn hunk(path: FilePath, number: usize) -> Self {
        Self {
            path,
            symbol: Some(format!("{HUNK_PREFIX}{number}")),
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
        self.path
            .as_str()
            .rsplit_once('/')
            .map_or("", |(folder, _)| {
                let with_slash = folder.len() + 1;
                self.path.as_str().get(..with_slash).unwrap_or_default()
            })
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

    /// Splits at the last `#` whose right side holds no `/`, so a folder may hold a `#`.
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let path_of =
            |path: &str| FilePath::new(path).map_err(|_refused| InvalidPieceId(text.to_owned()));
        match text.rsplit_once('#') {
            Some((path, symbol)) if !symbol.contains('/') => Self::symbol(path_of(path)?, symbol),
            _ => path_of(text).map(Self::whole_file),
        }
    }
}

impl Serialize for PieceId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for PieceId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}
