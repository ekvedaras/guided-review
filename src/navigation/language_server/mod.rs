//! The language server adapter of the code navigator port: one long-lived server per (codebase, server),
//! asked for the definition, the usages and the hover of a name, with the by-name lookup answering
//! whenever the server cannot.

mod answers;
mod checkout;
mod downloads;
mod hover;
mod managed;
mod notices;
mod pool;
mod positions;
mod recipes;
mod rpc;
mod running;
mod server_navigator;

use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;

use crate::codebase::Codebase;
use crate::language::Language;

use super::{
    Answer, AnsweredBy, CodeNavigator, Column, Definition, FallbackReason, Lookup, MessageKind,
    NavigationRequest, Place, Reach, ServerAnswer, ServerMessage, ServerMessages, ServerName,
    ServerOptIns, Usage, Waiting, Word,
};
pub use checkout::{Change, ChangedFile, Checkout, ServerCheckouts};
pub use managed::{ManagedServers, ServerFolders};
use notices::Notices;
use pool::Pool;
pub use recipes::{ModulePin, PackagePin, ReleasePin, ServerPins};
use rpc::locked;
use server_navigator::ServerNavigator;

/// Whether a server says by `experimental/serverStatus` that it is busy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StatusReports {
    Yes,
    #[default]
    No,
}

/// What a server's process is given of the environment.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum ServerEnvironment {
    /// The app's own, with relative and empty `PATH` entries removed.
    #[default]
    Inherited,
    /// These variables and no others, so none of the reviewer's settings reaches the server.
    Only(Vec<(&'static str, OsString)>),
}

/// How to start one server.
#[derive(Debug, Clone)]
pub struct ServerLaunch {
    pub name: ServerName,
    pub program: PathBuf,
    pub args: Vec<String>,
    /// Sent as `initializationOptions`, and as the answer to the server's configuration requests.
    pub options: Value,
    pub status: StatusReports,
    pub environment: ServerEnvironment,
}

impl ServerLaunch {
    /// The server tells whether it is busy with `experimental/serverStatus`, so until it has, it is.
    pub fn reporting_status(mut self) -> Self {
        self.status = StatusReports::Yes;
        self
    }

    pub fn with_args(mut self, args: Vec<String>) -> Self {
        self.args = args;
        self
    }
}

/// Which server answers which language, and how it is started.
pub trait ServerCatalog: Send + Sync {
    /// `Err` says why no server answers the language. `waiting` is told when the server has to be
    /// downloaded first.
    fn launch_for(
        &self,
        language: Language,
        codebase: &Codebase,
        waiting: &dyn Fn(Waiting),
    ) -> Result<ServerLaunch, FallbackReason>;
}

#[derive(Debug, Clone, Copy)]
pub struct Timeouts {
    pub start: Duration,
    pub answer: Duration,
    /// How long a hover waits for a server that is still indexing.
    pub indexing: Duration,
    /// How long a server may go unasked before it is stopped.
    pub idle: Duration,
}

impl Default for Timeouts {
    fn default() -> Self {
        Self {
            start: Duration::from_secs(30),
            answer: Duration::from_secs(30),
            indexing: Duration::from_secs(180),
            idle: Duration::from_secs(600),
        }
    }
}

/// The running servers of the app. One instance serves every review.
pub struct LanguageServers {
    pool: Arc<Pool>,
}

impl LanguageServers {
    pub fn new(
        catalog: Arc<dyn ServerCatalog>,
        checkouts: ServerCheckouts,
        timeouts: Timeouts,
        messages: Arc<dyn ServerMessages>,
    ) -> Self {
        Self {
            pool: Pool::new(catalog, checkouts, timeouts, Notices::new(messages)),
        }
    }

    /// The adapter for one review: `by_name` answers what no server can.
    pub fn navigator(
        &self,
        codebase: Codebase,
        by_name: Arc<dyn CodeNavigator>,
    ) -> Arc<dyn CodeNavigator> {
        Arc::new(ServerNavigator {
            pool: Arc::clone(&self.pool),
            codebase,
            by_name,
        })
    }

    /// What the reviewer answered for each codebase, as every review of it reads it.
    pub fn opt_ins(&self) -> Arc<dyn ServerOptIns> {
        Arc::clone(&self.pool) as Arc<dyn ServerOptIns>
    }

    /// Counts in every open review from their next hover; a no stops the codebase's servers.
    pub fn answered(&self, codebase: &Codebase, answer: ServerAnswer) {
        self.pool.answered(codebase, answer);
    }

    /// Every server shuts down; their indexes stay on disk.
    pub fn stop_all(&self) {
        self.pool.stop_all();
    }
}
