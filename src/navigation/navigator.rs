//! The code navigator port: what a name at a place means in a review. The viewer and the Definition
//! card ask it and know no adapter; the by-name lookup (`Lookups`) is its first adapter.

use std::fmt;
use std::path::Path;
use std::sync::Arc;

use crate::code_source::CodeSource;
use crate::codebase::Codebase;
use crate::file_path::FilePath;
use crate::pull_request::Sha;

use super::{Column, Definition, Place, Usage, Word};

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

/// The reviewer's answers to "Use language servers for this repo?" while the app runs, shared by
/// every open review, so an answer given on a card or in Settings counts in each of them at once.
pub trait ServerOptIns: Send + Sync {
    fn answer(&self, codebase: &Codebase) -> Option<ServerAnswer>;
    fn answered(&self, codebase: &Codebase, answer: ServerAnswer);
}

/// Whether a server's message reports a failure or only tells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageKind {
    Error,
    Notice,
}

/// A message a language server asked the client to show the reviewer (`window/showMessage`, or
/// `window/showMessageRequest`, which is answered with no action).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerMessage {
    pub server: ServerName,
    pub kind: MessageKind,
    pub text: String,
}

/// Where a server's messages are shown. Called from the server's reading thread.
pub trait ServerMessages: Send + Sync {
    fn show(&self, message: ServerMessage);
}

/// A language server, named as the card says it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerName(Arc<str>);

impl ServerName {
    pub fn new(name: &str) -> Self {
        Self(name.into())
    }

    /// The server that answers files of `language`.
    pub fn of(language: crate::language::Language) -> Self {
        use crate::language::Language;
        Self::new(match language {
            Language::Rust => "rust-analyzer",
            Language::Php => "Intelephense",
            Language::TypeScript | Language::Tsx => "typescript-language-server",
        })
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

/// Which files a lookup can see.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reach {
    WholeRepo,
    /// A local (a variable or parameter) is looked up in the function that holds it, so no other
    /// file is searched.
    EnclosingFunction,
    /// The code is read from the host, so only the review's changed files are searched.
    ChangedFiles,
}

/// Where a word is defined and used at a commit, definitions best match first.
#[derive(Debug, Clone)]
pub struct Lookup {
    pub word: Word,
    pub definitions: Vec<Definition>,
    /// By file, then line.
    pub usages: Arc<[Usage]>,
    pub reach: Reach,
    /// More files hold the word than were parsed, so definitions and usages may be missing.
    pub cut_short: bool,
    /// Files a language server named that are not in the repo's git: where each is on disk.
    pub on_disk_files: Arc<[(FilePath, std::path::PathBuf)]>,
}

impl Lookup {
    /// Where a place's file is on disk when git does not hold it.
    pub fn on_disk(&self, path: &FilePath) -> Option<&Path> {
        self.on_disk_files
            .iter()
            .find_map(|(file, disk)| (file == path).then_some(disk.as_path()))
    }
}
