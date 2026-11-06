//! How `guided-review open` and the one running app talk: a Unix socket in the guided-review home, one JSON
//! line per request and per answer, each carrying the version of the binary that wrote it.

use std::fs::{File, OpenOptions, TryLockError};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::viewer::HandOffId;

const APP_COMMAND_VARIABLE: &str = "GUIDED_REVIEW_APP_COMMAND";
const START_WAIT: Duration = Duration::from_secs(20);
const QUIT_WAIT: Duration = Duration::from_secs(10);
const POLL: Duration = Duration::from_millis(25);

/// Names the binary: its version and the time it was built, so a reinstall of the same version is another
/// build too.
pub fn build() -> String {
    if let Ok(named) = std::env::var("GUIDED_REVIEW_BUILD") {
        return named;
    }
    let built = std::env::current_exe()
        .and_then(|exe| exe.metadata())
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |since| since.as_secs());
    format!("{}+{built}", env!("CARGO_PKG_VERSION"))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenRequest {
    pub pr: String,
    /// The caller's folder, where a local clone of the PR is looked for.
    pub folder: PathBuf,
    /// The caller's herdr terminal, when it runs in one.
    pub terminal: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Request {
    Open(OpenRequest),
    /// The tour agent's reply to a hand-off is on GitHub.
    AgentReplied {
        pr: String,
        hand_off: HandOffId,
    },
    Quit,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestLine {
    pub version: String,
    pub request: Request,
}

impl RequestLine {
    pub fn new(request: Request) -> Self {
        Self {
            version: build(),
            request,
        }
    }

    pub fn to_line(&self) -> anyhow::Result<String> {
        serde_json::to_string(self).context("could not write the request")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Answer {
    /// The app is building this PR's review and shows it when it is ready.
    Opening {
        pr: String,
    },
    /// The window already shows this PR and read its tour again.
    Reloaded {
        pr: String,
    },
    Refused {
        reason: String,
    },
    /// The hand-off is marked answered and the threads are read again.
    HandOffAnswered {
        pr: String,
    },
    /// The window shows another PR, so the hand-off is not the app's to mark.
    NotShown {
        pr: String,
    },
    /// Saving what is typed, then quitting.
    Quitting,
    /// The request was written by another build and was not acted on.
    OtherBuild,
}

impl std::fmt::Display for Answer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Opening { pr } => write!(f, "guided-review app is opening {pr}"),
            Self::Reloaded { pr } => write!(f, "guided-review app reloaded the tour of {pr}"),
            Self::Refused { reason } => write!(f, "guided-review app refused: {reason}"),
            Self::HandOffAnswered { pr } => {
                write!(f, "guided-review app marked the hand-off answered on {pr}")
            }
            Self::NotShown { pr } => write!(f, "guided-review app does not show {pr}"),
            Self::Quitting => write!(f, "guided-review app is quitting"),
            Self::OtherBuild => write!(f, "guided-review app is another build"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnswerLine {
    pub version: String,
    pub answer: Answer,
}

/// Where the app keeps its socket, its lock and its log.
#[derive(Debug, Clone)]
pub struct AppFolder(PathBuf);

impl AppFolder {
    pub fn in_home(home: &Path) -> Self {
        Self(home.join(".guided-review"))
    }

    pub fn socket(&self) -> PathBuf {
        self.0.join("app.sock")
    }

    pub fn lock(&self) -> PathBuf {
        self.0.join("app.lock")
    }

    pub fn log(&self) -> PathBuf {
        self.0.join("app.log")
    }
}

#[derive(Debug, thiserror::Error)]
pub enum BindError {
    #[error("another app is running")]
    AlreadyRunning,
    #[error(transparent)]
    Failed(#[from] anyhow::Error),
}

/// The running app's end of the socket. Holding it holds the lock, so there is one app.
pub struct AppListener {
    listener: UnixListener,
    _lock: File,
}

impl AppListener {
    /// Takes the exclusive lock first; a socket file left by an app that is gone is replaced.
    pub fn bind(folder: &AppFolder) -> Result<Self, BindError> {
        let dir = folder.socket();
        let dir = dir.parent().context("the socket has no folder")?;
        std::fs::create_dir_all(dir)
            .with_context(|| format!("could not create {}", dir.display()))?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(folder.lock())
            .context("could not open the app lock")?;
        match lock.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => return Err(BindError::AlreadyRunning),
            Err(TryLockError::Error(error)) => {
                return Err(anyhow::Error::new(error)
                    .context("could not take the app lock")
                    .into());
            }
        }
        match std::fs::remove_file(folder.socket()) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(anyhow::Error::new(error)
                    .context("could not remove the old socket")
                    .into());
            }
        }
        let listener = UnixListener::bind(folder.socket()).context("could not bind the socket")?;
        Ok(Self {
            listener,
            _lock: lock,
        })
    }

    /// Answers requests on a thread of its own. A request of another build is not handed to `handle`,
    /// except a quit, which every build understands.
    pub fn serve(self, handle: impl Fn(Request) -> Answer + Send + 'static) {
        std::thread::spawn(move || {
            for stream in self.listener.incoming().flatten() {
                if let Err(error) = answer_one(&stream, &handle) {
                    eprintln!("could not answer a request: {error:#}");
                }
            }
        });
    }
}

fn answer_one(stream: &UnixStream, handle: &impl Fn(Request) -> Answer) -> anyhow::Result<()> {
    let mut line = String::new();
    BufReader::new(stream)
        .read_line(&mut line)
        .context("could not read the request")?;
    let request: RequestLine =
        serde_json::from_str(&line).context("the request is not a request line")?;
    let answer = if request.version != build() && request.request != Request::Quit {
        Answer::OtherBuild
    } else {
        handle(request.request)
    };
    let reply = serde_json::to_string(&AnswerLine {
        version: build(),
        answer,
    })?;
    let mut stream = stream;
    writeln!(stream, "{reply}").context("could not write the answer")
}

/// One request and its answer. `None` when no app is behind the socket.
fn exchange(socket: &Path, request: &Request) -> anyhow::Result<Option<AnswerLine>> {
    let Ok(mut stream) = UnixStream::connect(socket) else {
        return Ok(None);
    };
    stream.set_read_timeout(Some(QUIT_WAIT))?;
    writeln!(stream, "{}", RequestLine::new(request.clone()).to_line()?)
        .context("could not send the request")?;
    let mut reply = String::new();
    BufReader::new(stream)
        .read_line(&mut reply)
        .context("the app did not answer")?;
    serde_json::from_str(&reply)
        .map(Some)
        .context("the app's answer is not an answer line")
}

/// Tells the running app a hand-off was answered. `None` when no app runs, or one of another build,
/// which would not act on it.
pub fn tell_app(folder: &AppFolder, request: &Request) -> anyhow::Result<Option<Answer>> {
    Ok(exchange(&folder.socket(), request)?
        .filter(|line| line.version == build())
        .map(|line| line.answer))
}

/// Hands `open` to the running app, starting one when none answers and replacing one of another build.
pub fn hand_over(folder: &AppFolder, open: OpenRequest) -> anyhow::Result<Answer> {
    let request = Request::Open(open);
    let socket = folder.socket();
    match exchange(&socket, &request)? {
        Some(answer) if answer.version == build() => return Ok(answer.answer),
        Some(_) => {
            exchange(&socket, &Request::Quit)?;
            wait_until(QUIT_WAIT, || !is_answering(&socket))
                .context("the app of another build did not quit")?;
        }
        None => {}
    }
    start_app(folder)?;
    let mut answer = None;
    wait_until(START_WAIT, || {
        answer = exchange(&socket, &request).ok().flatten();
        answer.is_some()
    })
    .context("the app did not start; see its log in the guided-review folder")?;
    answer
        .map(|line| line.answer)
        .context("the app did not answer")
}

fn is_answering(socket: &Path) -> bool {
    UnixStream::connect(socket).is_ok()
}

fn wait_until(limit: Duration, mut done: impl FnMut() -> bool) -> Option<()> {
    let deadline = Instant::now() + limit;
    while !done() {
        if Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(POLL);
    }
    Some(())
}

/// The app runs on its own process group, so it outlives the agent or terminal that ran `open`.
fn start_app(folder: &AppFolder) -> anyhow::Result<()> {
    let program = match std::env::var_os(APP_COMMAND_VARIABLE) {
        Some(program) => PathBuf::from(program),
        None => std::env::current_exe().context("could not find the guided-review binary")?,
    };
    let log_path = folder.log();
    if let Some(dir) = log_path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .with_context(|| format!("could not open {}", log_path.display()))?;
    std::process::Command::new(program)
        .arg("serve")
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log)
        .process_group(0)
        .spawn()
        .context("could not start the app")?;
    Ok(())
}
