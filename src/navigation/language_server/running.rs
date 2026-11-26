//! One started server: how it was started and initialized, what it has open, which head of the
//! checkout it last heard of, and when it last answered.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::language::Language;
use crate::line_number::LineNumber;
use crate::pull_request::Sha;

use super::notices::Notices;
use super::positions::{Encoding, language_id, uri_of};
use super::rpc::{Connection, Failure, locked};
use super::{
    Change, ChangedFile, Checkout, Column, MessageKind, ServerLaunch, ServerMessage, Timeouts,
    Waiting,
};

/// A request no server knows. LSP 3.17, "$ Notifications and Requests": a server that does not
/// implement a `$/` request answers it with `MethodNotFound`. Servers handle messages in order, so
/// that answer comes after everything the server began on `initialized`, which makes the request a
/// barrier. The test server in `tests/fixtures/fake_lsp.py` names it too.
const BARRIER_REQUEST: &str = "$/guidedReviewBarrier";

pub(super) struct Running {
    connection: Connection,
    timeouts: Timeouts,
    encoding: Encoding,
    opened: Mutex<HashSet<PathBuf>>,
    /// Since the start, until the first answer.
    last_answered: Mutex<Instant>,
    /// Questions sent and not yet answered or given up on: a server with one is not idle.
    in_flight: AtomicUsize,
    /// The head of the checkout the server last heard of: what the next move is told against.
    heard: Mutex<Sha>,
}

impl Running {
    pub(super) fn start(
        launch: &ServerLaunch,
        checkout: &Checkout,
        timeouts: Timeouts,
        notices: &Arc<Notices>,
    ) -> Result<Self, String> {
        let folder = checkout.folder.as_path();
        let (server, notices) = (launch.name.clone(), Arc::clone(notices));
        let on_message = Box::new(move |kind: MessageKind, text: String| {
            notices.show(ServerMessage {
                server: server.clone(),
                kind,
                text,
            });
        });
        let connection = Connection::spawn(
            &launch.program,
            &launch.args,
            folder,
            &launch.environment,
            launch.options.clone(),
            launch.status,
            on_message,
        )
        .map_err(|error| format!("{} could not start: {error}", launch.name))?;
        let uri = uri_of(folder);
        let initialized = connection.request(
            "initialize",
            &json!({
                "processId": std::process::id(),
                "rootUri": uri,
                "workspaceFolders": [{"uri": uri, "name": "repo"}],
                "initializationOptions": launch.options,
                "capabilities": {
                    "general": {"positionEncodings": ["utf-8", "utf-16"]},
                    "window": {"workDoneProgress": true, "showMessage": {}},
                    "workspace": {"configuration": true},
                    "textDocument": {"hover": {"contentFormat": ["markdown", "plaintext"]}},
                    "experimental": {"serverStatusNotification": true},
                },
            }),
            timeouts.start,
        );
        let initialized = initialized.map_err(|failure| match failure {
            Failure::Silent => format!("{} did not start in time", launch.name),
            Failure::Died => format!("{} stopped while starting", launch.name),
            Failure::Refused(why) => format!("{} refused to start: {why}", launch.name),
            Failure::Modified => format!("{} was not ready to start", launch.name),
        })?;
        let encoding = Encoding::named(
            initialized
                .pointer("/capabilities/positionEncoding")
                .and_then(Value::as_str),
        );
        let stopped = |_| format!("{} stopped while starting", launch.name);
        connection
            .notify("initialized", &json!({}))
            .map_err(stopped)?;
        // A round trip lets the progress the server began on `initialized` arrive before anyone checks
        // whether it is busy. A server that does not know the method answers with an error, or not at all.
        connection
            .request(BARRIER_REQUEST, &Value::Null, Duration::from_secs(2))
            .ok();
        Ok(Self {
            connection,
            timeouts,
            encoding,
            opened: Mutex::default(),
            last_answered: Mutex::new(Instant::now()),
            in_flight: AtomicUsize::new(0),
            heard: Mutex::new(checkout.head.clone()),
        })
    }

    pub(super) fn answered(&self) {
        *locked(&self.last_answered) = Instant::now();
    }

    /// Counted from the last answer, so questions that time out do not keep a server running.
    pub(super) fn idle_for(&self) -> Duration {
        if self.in_flight.load(Ordering::SeqCst) > 0 {
            return Duration::ZERO;
        }
        locked(&self.last_answered).elapsed()
    }

    pub(super) fn open(&self, file: &Path, text: &str, language: Language) -> Result<(), Failure> {
        if !locked(&self.opened).insert(file.to_path_buf()) {
            return Ok(());
        }
        self.connection.notify(
            "textDocument/didOpen",
            &json!({"textDocument": {
                "uri": uri_of(file),
                "languageId": language_id(language),
                "version": 1,
                "text": text,
            }}),
        )
    }

    /// The checkout is at another head than the server last heard of: files the server has open are
    /// opened again from their new text, and the server hears which files changed on disk since its own
    /// head so it re-indexes those only. Fails when that head can no longer be compared.
    pub(super) fn catch_up(&self, checkout: &Checkout) -> anyhow::Result<()> {
        let mut heard = locked(&self.heard);
        if *heard != checkout.head {
            self.tell_changed_files(&checkout.folder, &checkout.changes_since(&heard)?);
            *heard = checkout.head.clone();
        }
        Ok(())
    }

    /// A server that does not hear is stale until it is restarted, so the failure is not an answer's.
    fn tell_changed_files(&self, folder: &Path, changed: &[ChangedFile]) {
        if changed.is_empty() {
            return;
        }
        let mut opened = locked(&self.opened);
        let mut events = Vec::new();
        for file in changed {
            let path = file.path.under(folder);
            if opened.remove(&path) {
                self.connection
                    .notify(
                        "textDocument/didClose",
                        &json!({"textDocument": {"uri": uri_of(&path)}}),
                    )
                    .ok();
            }
            let kind = match file.change {
                Change::Created => 1,
                Change::Changed => 2,
                Change::Deleted => 3,
            };
            events.push(json!({"uri": uri_of(&path), "type": kind}));
        }
        self.connection
            .notify(
                "workspace/didChangeWatchedFiles",
                &json!({"changes": events}),
            )
            .ok();
    }

    pub(super) fn character(&self, text: &str, line: LineNumber, column: Column) -> Option<u32> {
        self.encoding.character(text, line, column)
    }

    pub(super) fn is_alive(&self) -> bool {
        self.connection.is_alive()
    }

    pub(super) fn stop(&self) {
        self.connection.stop();
    }

    /// Waits while the server says it is busy, at most as long as indexing may take.
    pub(super) fn wait_until_quiet(&self, waiting: &dyn Fn(Waiting)) -> Result<(), Failure> {
        self.connection
            .wait_until_quiet(self.timeouts.indexing, &|| waiting(Waiting::Indexing))
    }

    pub(super) fn ask(&self, method: &str, params: &Value) -> Result<Value, Failure> {
        let _in_flight: InFlight<'_> = InFlight::counted_in(&self.in_flight);
        self.connection
            .request(method, params, self.timeouts.answer)
    }
}

/// One question on its way to the server, counted until it is answered or given up on.
#[must_use = "the question counts as in flight only while this is held"]
struct InFlight<'a>(&'a AtomicUsize);

impl<'a> InFlight<'a> {
    fn counted_in(in_flight: &'a AtomicUsize) -> Self {
        in_flight.fetch_add(1, Ordering::SeqCst);
        Self(in_flight)
    }
}

impl Drop for InFlight<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}
