//! The language server adapter of the code navigator port: one long-lived server per (repo, server),
//! asked for the definition, the usages and the hover of a name, with the by-name lookup answering
//! whenever the server cannot.

mod hover;
mod managed;
mod rpc;

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::code_source::CodeSource;
use crate::file_path::FilePath;
use crate::language::Language;
use crate::line_number::LineNumber;
use crate::pull_request::Sha;

use super::{
    Answer, AnsweredBy, CodeNavigator, Column, Definition, FallbackReason, Lookup,
    NavigationRequest, Place, Reach, ServerName, Usage, Waiting, Word,
};
use hover::{Hovered, kind_of};
pub use managed::{ManagedServers, ReleasePin, ServerPins};
use rpc::{Connection, Failure, locked};

/// Whether a server says by `experimental/serverStatus` that it is busy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StatusReports {
    Yes,
    #[default]
    No,
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
}

impl ServerLaunch {
    /// Build scripts, proc macros and check on save are off, so it builds nothing in the repo.
    pub fn rust_analyzer(program: PathBuf) -> Self {
        Self {
            name: ServerName::new("rust-analyzer"),
            program,
            args: Vec::new(),
            status: StatusReports::No,
            options: json!({
                "cargo": {"buildScripts": {"enable": false}},
                "procMacro": {"enable": false},
                "checkOnSave": false,
            }),
        }
    }

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

/// Counts a position's column in the unit the server speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Encoding {
    Utf8,
    Utf16,
}

struct Running {
    connection: Connection,
    encoding: Encoding,
    opened: Mutex<HashSet<PathBuf>>,
    last_used: Mutex<Instant>,
}

impl Running {
    fn start(launch: &ServerLaunch, folder: &Path, timeouts: &Timeouts) -> Result<Self, String> {
        let connection = Connection::spawn(
            &launch.program,
            &launch.args,
            folder,
            launch.options.clone(),
            launch.status,
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
                    "window": {"workDoneProgress": true},
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
        })?;
        let encoding = match initialized
            .pointer("/capabilities/positionEncoding")
            .and_then(Value::as_str)
        {
            Some("utf-8") => Encoding::Utf8,
            _ => Encoding::Utf16,
        };
        let stopped = |_| format!("{} stopped while starting", launch.name);
        connection
            .notify("initialized", &json!({}))
            .map_err(stopped)?;
        // A round trip lets the progress the server began on `initialized` arrive before anyone checks
        // whether it is busy. A server that does not know the method answers with an error, or not at all.
        connection
            .request(
                "$/guidedReviewBarrier",
                &Value::Null,
                Duration::from_secs(2),
            )
            .ok();
        Ok(Self {
            connection,
            encoding,
            opened: Mutex::default(),
            last_used: Mutex::new(Instant::now()),
        })
    }

    fn touch(&self) {
        *locked(&self.last_used) = Instant::now();
    }

    fn idle_for(&self) -> Duration {
        locked(&self.last_used).elapsed()
    }

    fn open(&self, file: &Path, text: &str, language: Language) -> Result<(), Failure> {
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

    fn character(&self, text: &str, line: LineNumber, column: Column) -> Option<u32> {
        let line_text = text.lines().nth(usize::from(line).checked_sub(1)?)?;
        let before = line_text.get(..usize::try_from(column.get()).ok()?)?;
        match self.encoding {
            Encoding::Utf8 => u32::try_from(before.len()).ok(),
            Encoding::Utf16 => u32::try_from(before.encode_utf16().count()).ok(),
        }
    }

    fn ask(&self, method: &str, params: &Value, within: Duration) -> Result<Value, Failure> {
        self.connection.request(method, params, within)
    }
}

fn language_id(language: Language) -> &'static str {
    match language {
        Language::Rust => "rust",
        Language::Php => "php",
        Language::TypeScript => "typescript",
        Language::Tsx => "typescriptreact",
    }
}

fn uri_of(path: &Path) -> String {
    let mut uri = String::from("file://");
    for byte in path.to_string_lossy().bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b'-' | b'_' | b'.' | b'~' => {
                uri.push(char::from(byte));
            }
            other => {
                // Writing to a String cannot fail.
                write!(uri, "%{other:02X}").ok();
            }
        }
    }
    uri
}

fn path_of(uri: &str) -> Option<PathBuf> {
    let encoded = uri.strip_prefix("file://")?;
    let mut bytes = Vec::new();
    let mut rest = encoded.bytes();
    while let Some(byte) = rest.next() {
        if byte == b'%' {
            let high = rest.next()?;
            let low = rest.next()?;
            let pair = [high, low];
            let hex = std::str::from_utf8(&pair).ok()?;
            bytes.push(u8::from_str_radix(hex, 16).ok()?);
        } else {
            bytes.push(byte);
        }
    }
    String::from_utf8(bytes).ok().map(PathBuf::from)
}

type Slot = Arc<Mutex<Option<Arc<Running>>>>;

struct Pool {
    catalog: Arc<dyn ServerCatalog>,
    timeouts: Timeouts,
    /// One per (folder, server): the server is reused by every review of the repo.
    slots: Mutex<HashMap<(PathBuf, String), Slot>>,
}

impl Pool {
    fn slot(&self, folder: &Path, name: &ServerName) -> Slot {
        let mut slots = locked(&self.slots);
        Arc::clone(
            slots
                .entry((folder.to_path_buf(), name.to_string()))
                .or_default(),
        )
    }

    fn stop_where(&self, stop_it: impl Fn(&Running) -> bool) {
        let slots: Vec<Slot> = locked(&self.slots).values().cloned().collect();
        for slot in slots {
            let mut running = locked(&slot);
            if running.as_ref().is_some_and(|server| stop_it(server))
                && let Some(server) = running.take()
            {
                server.connection.stop();
            }
        }
    }
}

/// The running servers of the app. One instance serves every review.
pub struct LanguageServers {
    pool: Arc<Pool>,
}

impl LanguageServers {
    pub fn new(catalog: Arc<dyn ServerCatalog>, timeouts: Timeouts) -> Self {
        let pool = Arc::new(Pool {
            catalog,
            timeouts,
            slots: Mutex::default(),
        });
        let watched: Weak<Pool> = Arc::downgrade(&pool);
        let idle = timeouts.idle;
        let tick = (idle / 4).clamp(Duration::from_millis(50), Duration::from_secs(5));
        std::thread::spawn(move || {
            loop {
                std::thread::sleep(tick);
                let Some(pool) = watched.upgrade() else {
                    return;
                };
                pool.stop_where(|server| server.idle_for() >= idle);
            }
        });
        Self { pool }
    }

    /// The adapter for one review: `by_name` answers what no server can.
    pub fn navigator(&self, by_name: Arc<dyn CodeNavigator>) -> Arc<dyn CodeNavigator> {
        Arc::new(ServerNavigator {
            pool: Arc::clone(&self.pool),
            by_name,
        })
    }

    /// Every server shuts down; their indexes stay on disk.
    pub fn stop_all(&self) {
        self.pool.stop_where(|_| true);
    }
}

struct ServerNavigator {
    pool: Arc<Pool>,
    by_name: Arc<dyn CodeNavigator>,
}

impl CodeNavigator for ServerNavigator {
    fn navigate(
        &self,
        code: &dyn CodeSource,
        request: &NavigationRequest,
        waiting: &dyn Fn(Waiting),
    ) -> anyhow::Result<Answer> {
        let by_name = |because: Option<FallbackReason>| -> anyhow::Result<Answer> {
            let mut answer = self.by_name.navigate(code, request, waiting)?;
            answer.answered_by = AnsweredBy::ByName { because };
            Ok(answer)
        };
        let Some(language) = Language::for_path(request.at.path.as_str()) else {
            return by_name(None);
        };
        let launch = match self.pool.catalog.launch_for(language, waiting) {
            Ok(launch) => launch,
            Err(reason) => return by_name(Some(reason)),
        };
        let Some(clone) = code.local_clone() else {
            return by_name(Some(FallbackReason::new(&format!(
                "{} needs a local clone, and the code is read from GitHub",
                launch.name
            ))));
        };
        if !code.working_tree_is_at(&request.sha) {
            return by_name(Some(FallbackReason::new(&format!(
                "{} needs the clone clean at the reviewed head, and it is not",
                launch.name
            ))));
        }
        match self.answer(code, clone, &launch, language, request, waiting) {
            Ok(answer) => Ok(answer),
            Err(reason) => by_name(Some(FallbackReason::new(&reason))),
        }
    }
}

impl ServerNavigator {
    fn running(
        &self,
        clone: &Path,
        launch: &ServerLaunch,
        waiting: &dyn Fn(Waiting),
    ) -> Result<(Slot, Arc<Running>), String> {
        let slot = self.pool.slot(clone, &launch.name);
        let mut held = locked(&slot);
        if let Some(server) = held.as_ref().filter(|server| server.connection.is_alive()) {
            return Ok((Arc::clone(&slot), Arc::clone(server)));
        }
        waiting(Waiting::Starting(launch.name.clone()));
        let server = Arc::new(Running::start(launch, clone, &self.pool.timeouts)?);
        *held = Some(Arc::clone(&server));
        drop(held);
        Ok((slot, server))
    }

    fn answer(
        &self,
        code: &dyn CodeSource,
        clone: &Path,
        launch: &ServerLaunch,
        language: Language,
        request: &NavigationRequest,
        waiting: &dyn Fn(Waiting),
    ) -> Result<Answer, String> {
        let name = &launch.name;
        let timeouts = self.pool.timeouts;
        let (slot, server) = self.running(clone, launch, waiting)?;
        server.touch();
        let failed = |failure: Failure| {
            if failure == Failure::Died {
                let mut held = locked(&slot);
                if held.as_ref().is_some_and(|held| Arc::ptr_eq(held, &server)) {
                    *held = None;
                }
            }
            match failure {
                Failure::Silent => format!("{name} did not answer in time"),
                Failure::Died => format!("{name} stopped answering"),
                Failure::Refused(why) => format!("{name} refused the question: {why}"),
            }
        };
        server
            .connection
            .wait_until_quiet(timeouts.indexing, &|| waiting(Waiting::Indexing))
            .map_err(failed)?;
        let file = request.at.path.under(clone);
        let text = std::fs::read_to_string(&file)
            .map_err(|error| format!("{} could not read {}: {error}", name, request.at.path))?;
        server.open(&file, &text, language).map_err(failed)?;
        let character = server
            .character(&text, request.at.line, request.column)
            .ok_or_else(|| format!("{name} was asked about a place the line does not have"))?;
        let line = u32::try_from(usize::from(request.at.line).saturating_sub(1))
            .map_err(|error| format!("the line number is too large: {error}"))?;
        let place = json!({
            "textDocument": {"uri": uri_of(&file)},
            "position": {"line": line, "character": character},
        });
        let references = json!({
            "textDocument": {"uri": uri_of(&file)},
            "position": {"line": line, "character": character},
            "context": {"includeDeclaration": false},
        });
        let within = timeouts.answer;
        let definitions = server
            .ask("textDocument/definition", &place, within)
            .map_err(failed)?;
        let usages = server
            .ask("textDocument/references", &references, within)
            .map_err(failed)?;
        let hover = server
            .ask("textDocument/hover", &place, within)
            .map_err(failed)?;
        server.touch();
        let places = Places {
            code,
            clone,
            sha: &request.sha,
        };
        Ok(Answer {
            lookup: places.lookup(&request.word, &definitions, &usages, &hover),
            answered_by: AnsweredBy::LanguageServer(name.clone()),
        })
    }
}

/// Turns the server's locations into places of the repo.
struct Places<'a> {
    code: &'a dyn CodeSource,
    clone: &'a Path,
    sha: &'a Sha,
}

struct Found {
    place: Place,
    /// Set when the file is not in the repo's git, so a click opens it from disk.
    on_disk: Option<PathBuf>,
}

impl Places<'_> {
    fn lookup(&self, word: &Word, definitions: &Value, usages: &Value, hover: &Value) -> Lookup {
        let hovered = Hovered::of(hover);
        let mut on_disk: Vec<(FilePath, PathBuf)> = Vec::new();
        let mut keep = |found: &Found| {
            if let Some(path) = &found.on_disk
                && !on_disk.iter().any(|(file, _)| *file == found.place.path)
            {
                on_disk.push((found.place.path.clone(), path.clone()));
            }
        };
        let mut sources: HashMap<FilePath, Option<String>> = HashMap::new();
        let mut definition_list = Vec::new();
        for (index, found) in locations_of(definitions)
            .filter_map(|at| self.place_of(at))
            .take(10)
            .enumerate()
        {
            keep(&found);
            let line_text = self.line_at(&mut sources, &found).unwrap_or_default();
            let (signature, doc) = match (index, &hovered.signature) {
                (0, Some(signature)) => (signature.clone(), hovered.doc.clone()),
                _ => (line_text, None),
            };
            definition_list.push(Definition {
                name: word.clone(),
                kind: kind_of(&signature),
                place: found.place,
                signature: signature.into(),
                signature_origin: Arc::new([]),
                doc: doc.map(Into::into),
            });
        }
        let mut usage_list = Vec::new();
        for found in locations_of(usages).filter_map(|at| self.place_of(at)) {
            keep(&found);
            if let Some(text) = self.line_at(&mut sources, &found) {
                usage_list.push(Usage {
                    place: found.place,
                    text: text.into(),
                });
            }
        }
        usage_list
            .sort_by(|a, b| (&a.place.path, a.place.line).cmp(&(&b.place.path, b.place.line)));
        Lookup {
            word: word.clone(),
            definitions: definition_list,
            usages: usage_list.into(),
            reach: Reach::WholeRepo,
            cut_short: false,
            on_disk_files: on_disk.into(),
        }
    }

    fn place_of(&self, location: &Value) -> Option<Found> {
        let uri = location["uri"]
            .as_str()
            .or_else(|| location["targetUri"].as_str())?;
        let range = if location["range"].is_null() {
            &location["targetSelectionRange"]
        } else {
            &location["range"]
        };
        let line = LineNumber::try_from(
            u32::try_from(range.pointer("/start/line")?.as_u64()?)
                .ok()?
                .checked_add(1)?,
        )
        .ok()?;
        let absolute = path_of(uri)?;
        let inside = absolute
            .strip_prefix(self.clone)
            .ok()
            .map(Path::to_path_buf)
            .or_else(|| {
                let root = self.clone.canonicalize().ok()?;
                absolute
                    .canonicalize()
                    .ok()?
                    .strip_prefix(root)
                    .ok()
                    .map(Path::to_path_buf)
            });
        let tracked = inside
            .as_deref()
            .and_then(|relative| FilePath::new(&relative.to_string_lossy()).ok())
            .filter(|path| self.code.blob_info_at(self.sha, path).is_ok());
        let (path, on_disk) = if let Some(path) = tracked {
            (path, None)
        } else {
            let label = inside.unwrap_or_else(|| absolute.clone());
            let label = label.to_string_lossy();
            let path = FilePath::new(label.trim_start_matches('/')).ok()?;
            (path, Some(absolute))
        };
        Some(Found {
            place: Place { path, line },
            on_disk,
        })
    }

    fn line_at(
        &self,
        sources: &mut HashMap<FilePath, Option<String>>,
        found: &Found,
    ) -> Option<String> {
        let source =
            sources
                .entry(found.place.path.clone())
                .or_insert_with(|| match &found.on_disk {
                    Some(disk) => std::fs::read_to_string(disk).ok(),
                    None => self.code.file_at(self.sha, &found.place.path).ok(),
                });
        let index = usize::from(found.place.line).checked_sub(1)?;
        Some(source.as_deref()?.lines().nth(index)?.trim().to_owned())
    }
}

/// A server answers with one location, a list of them, or nothing.
fn locations_of(answer: &Value) -> impl Iterator<Item = &Value> {
    let list: &[Value] = match answer {
        Value::Array(list) => list,
        Value::Object(_) => std::slice::from_ref(answer),
        _ => &[],
    };
    list.iter()
}
