//! The code navigator port: what a name at a place means in a review. The viewer and the Definition
//! card ask it and know no adapter; the by-name lookup (`Lookups`) is its first adapter.

use std::fmt;
use std::sync::Arc;

use crate::code_source::CodeSource;
use crate::pull_request::Sha;

use super::{Column, Lookup, Place, Word};

/// A name at a place: the line and column of its first letter in the file at `sha`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NavigationRequest {
    pub sha: Sha,
    pub at: Place,
    pub column: Column,
    pub word: Word,
}

/// What the reviewer answered to "Use language servers for this repo?"; asked once per repo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerAnswer {
    Yes,
    No,
}

impl ServerAnswer {
    pub fn of(use_servers: bool) -> Self {
        if use_servers { Self::Yes } else { Self::No }
    }

    pub fn is_yes(self) -> bool {
        self == Self::Yes
    }
}

/// A language server, named as the card says it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerName(Arc<str>);

impl ServerName {
    pub fn new(name: &str) -> Self {
        Self(name.into())
    }
}

impl fmt::Display for ServerName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// What an adapter waits on before it can answer, reported while the answer is on its way.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Waiting {
    Downloading(ServerName),
    Starting(ServerName),
    Indexing,
}

impl Waiting {
    /// "Downloading rust-analyzer…"
    pub fn text(&self) -> String {
        match self {
            Self::Downloading(server) => format!("Downloading {server}…"),
            Self::Starting(server) => format!("Starting {server}…"),
            Self::Indexing => "Indexing…".to_owned(),
        }
    }
}

/// Why an adapter that would have answered better handed the name to the by-name lookup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FallbackReason(Arc<str>);

impl FallbackReason {
    pub fn new(reason: &str) -> Self {
        Self(reason.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Which adapter gave an answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnsweredBy {
    LanguageServer(ServerName),
    /// `because` is why nothing better answered; `None` when by name was all that was asked for.
    ByName {
        because: Option<FallbackReason>,
    },
}

#[derive(Debug, Clone)]
pub struct Answer {
    pub lookup: Lookup,
    pub answered_by: AnsweredBy,
}

/// Answers what a name at a place is. `waiting` is called with what the adapter waits on while it
/// has not answered yet; an adapter that never waits never calls it.
pub trait CodeNavigator: Send + Sync {
    fn navigate(
        &self,
        code: &dyn CodeSource,
        request: &NavigationRequest,
        waiting: &dyn Fn(Waiting),
    ) -> anyhow::Result<Answer>;
}
