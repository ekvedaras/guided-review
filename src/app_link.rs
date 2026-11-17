//! How `guided-review open` and the one running app talk: a Unix socket in the state folder, one JSON
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

use crate::app_folders::AppFolders;
use crate::review_subject::ReviewSubject;
use crate::tour::StepId;
use crate::update::ReleaseInstall;
use crate::viewer::{AgentReplied, HandOffId};

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
    // Not `platform::running_binary`: once a reinstall replaces this binary, that names the new
    // one, and this build would take the new build's name.
    let built = std::env::current_exe()
        .and_then(|exe| exe.metadata())
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |since| since.as_secs());
    format!("{}+{built}", env!("CARGO_PKG_VERSION"))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "OpenRequestOnTheWire", into = "OpenRequestOnTheWire")]
pub struct OpenRequest {
    /// The subject key of what to open.
    pub subject: String,
    /// The caller's folder, where a local clone of the PR is looked for.
    pub folder: PathBuf,
    /// The caller's herdr terminal, when it runs in one.
    pub terminal: Option<String>,
    /// The step to show once open, as a restart brings back the screen it left; else the summary.
    pub at_step: Option<StepId>,
}

/// An open request as the line carries it. Any build may take an open, and a build from before local
/// review reads only `pr`, so a PR's key goes in `pr` too; a request from such a build has only `pr`.
#[derive(Serialize, Deserialize)]
struct OpenRequestOnTheWire {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    subject: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pr: Option<String>,
    folder: PathBuf,
    terminal: Option<String>,
    #[serde(default)]
    at_step: Option<StepId>,
}

impl TryFrom<OpenRequestOnTheWire> for OpenRequest {
    type Error = &'static str;

    fn try_from(wire: OpenRequestOnTheWire) -> Result<Self, Self::Error> {
        Ok(Self {
            subject: wire
                .subject
                .or(wire.pr)
                .ok_or("an open request names neither a subject nor a PR")?,
            folder: wire.folder,
            terminal: wire.terminal,
            at_step: wire.at_step,
        })
    }
}

impl From<OpenRequest> for OpenRequestOnTheWire {
    fn from(request: OpenRequest) -> Self {
        let is_a_pr = request
            .subject
            .parse::<ReviewSubject>()
            .is_ok_and(|subject| subject.has_github_page());
        Self {
            pr: is_a_pr.then(|| request.subject.clone()),
            subject: Some(request.subject),
            folder: request.folder,
            terminal: request.terminal,
            at_step: request.at_step,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Request {
    Open(OpenRequest),
    /// The tour agent's reply to a hand-off is on GitHub.
    AgentReplied {
        pr: String,
        hand_off: HandOffId,
        #[serde(default)]
        replied: AgentReplied,
    },
    /// Show Settings: over the PR on screen, or in a window of its own when no PR is open.
    ShowSettings,
    /// Show the PR switcher, as a launch from the Desktop entry does: over the PR on screen, or in a
    /// window of its own when no PR is open.
    ShowSwitcher,
    Quit,
}

/// Whether an app of another build may act on a request it can read.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AcrossBuilds {
    /// Requests of builds before 2026-11-16 carry no mark, and their CLI quits an app that refuses.
    #[default]
    ThisBuildOnly,
    AnyBuild,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestLine {
    pub version: String,
    pub request: Request,
    #[serde(default)]
    pub across_builds: AcrossBuilds,
    /// For a quit: the build of the app it ends, which the sender learnt from that app's answer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replacing: Option<String>,
}

impl RequestLine {
    pub fn new(request: Request) -> Self {
        Self {
            version: build(),
            request,
            across_builds: AcrossBuilds::ThisBuildOnly,
            replacing: None,
        }
    }

    pub fn for_any_build(request: Request) -> Self {
        Self {
            across_builds: AcrossBuilds::AnyBuild,
            ..Self::new(request)
        }
    }

    /// A request no app acts on, whose answer names the running app's build: its version is no build's,
    /// and apps of every build so far answer another build's request with their own build, acting on
    /// nothing but a quit.
    fn asking_which_build() -> Self {
        Self {
            version: "which-build".to_owned(),
            ..Self::new(Request::ShowSwitcher)
        }
    }

    pub fn quit_replacing(running: &str) -> Self {
        Self {
            replacing: Some(running.to_owned()),
            ..Self::new(Request::Quit)
        }
    }

    /// Whether the app of `build` acts on this request: always for its own build; for another build,
    /// a quit only when it names `build` as the one it ends, any other request only when it says any
    /// build may take it.
    pub fn is_taken_by(&self, build: &str) -> bool {
        if self.version == build {
            return true;
        }
        match self.request {
            Request::Quit => self.replacing.as_deref() == Some(build),
            Request::Open(_)
            | Request::AgentReplied { .. }
            | Request::ShowSettings
            | Request::ShowSwitcher => self.across_builds == AcrossBuilds::AnyBuild,
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
    /// The PR has no tour, so the app shows a tour agent writing one; the agent's own `open` ends it.
    Writing {
        pr: String,
    },
    /// The reviewer reads another PR, so the app does not switch; its PR switcher marks this one ready.
    KeptInBackground {
        pr: String,
    },
    /// The basic tour on screen now has a guided tour ready; the window did not move, and the
    /// reviewer switches when they choose.
    GuidedReady {
        pr: String,
    },
    /// The hand-off is marked answered and the threads are read again.
    HandOffAnswered {
        pr: String,
    },
    /// The window shows another PR, so the hand-off is not the app's to mark.
    NotShown {
        pr: String,
    },
    /// Settings is on screen.
    ShowingSettings,
    /// The PR switcher is on screen, or Settings being edited are, which come first.
    ShowingSwitcher,
    /// Saving what is typed, then quitting.
    Quitting,
    /// The request was written by another build and was not acted on.
    OtherBuild,
}

impl std::fmt::Display for Answer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Opening { pr } => write!(f, "guided-review app is opening {pr}"),
            Self::Writing { pr } => write!(
                f,
                "No tour of {pr} yet; guided-review app is writing one and opens it when it is ready"
            ),
            Self::Reloaded { pr } => write!(f, "guided-review app reloaded the tour of {pr}"),
            Self::Refused { reason } => write!(f, "guided-review app refused: {reason}"),
            Self::KeptInBackground { pr } => write!(
                f,
                "guided-review app has {pr} ready; the reviewer reads another PR, so it shows in the PR switcher"
            ),
            Self::GuidedReady { pr } => write!(
                f,
                "guided-review app shows the guided tour of {pr} as ready; the reviewer switches to it when they choose"
            ),
            Self::HandOffAnswered { pr } => {
                write!(f, "guided-review app marked the hand-off answered on {pr}")
            }
            Self::NotShown { pr } => write!(f, "guided-review app does not show {pr}"),
            Self::ShowingSettings => write!(f, "guided-review app is showing Settings"),
            Self::ShowingSwitcher => write!(f, "guided-review app is showing the PR switcher"),
            Self::Quitting => write!(f, "guided-review app is quitting"),
            Self::OtherBuild => write!(f, "guided-review app is another build"),
        }
    }
}

/// Who runs a command that reaches the app, which decides whether it may end an app of another build.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Caller {
    /// Run in a terminal, by the reviewer or by an agent, which cannot be told apart.
    Command,
    /// The reviewer launched the app from the OS launcher.
    Launcher,
    /// The app's Restart, or its update, ran this build to take over from the app of `replacing`.
    AppRestart { replacing: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OnOtherBuild {
    /// Quit the running app and start this build's.
    Replace,
    Stop(OtherBuildRuns),
}

impl Caller {
    /// What to do about a running app of the `running` build, another than this one, that did not take
    /// the request. Only the reviewer ends their app: a command from another build once ended the
    /// reviewer's trial build and its session, run by a tour agent.
    pub fn on_other_build(&self, running: &str) -> OnOtherBuild {
        match self {
            Self::Launcher => OnOtherBuild::Replace,
            Self::AppRestart { replacing } if replacing == running => OnOtherBuild::Replace,
            Self::Command | Self::AppRestart { .. } => OnOtherBuild::Stop(OtherBuildRuns {
                running: running.to_owned(),
                this: build(),
            }),
        }
    }

    fn across_builds(&self) -> AcrossBuilds {
        match self {
            Self::Command | Self::AppRestart { .. } => AcrossBuilds::AnyBuild,
            Self::Launcher => AcrossBuilds::ThisBuildOnly,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error(
    "the running guided-review app is build {running} and this guided-review is build {this}; the app \
     does not take this request from another build, and it was left running. Run the command with \
     the guided-review the app runs, or let the reviewer quit the app (Primary+Q, or Restart app \
     here to start the build its binary is now) and run it again"
)]
pub struct OtherBuildRuns {
    pub running: String,
    pub this: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnswerLine {
    pub version: String,
    pub answer: Answer,
}

/// Where the app keeps its socket, its lock and its log, in the app folders.
#[derive(Debug, Clone)]
pub struct AppFolder {
    folders: AppFolders,
}

impl AppFolder {
    pub fn of(folders: &AppFolders) -> Self {
        Self {
            folders: folders.clone(),
        }
    }

    pub fn socket(&self) -> PathBuf {
        self.folders.socket()
    }

    pub fn lock(&self) -> PathBuf {
        self.folders.lock()
    }

    pub fn log(&self) -> PathBuf {
        self.folders.log()
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
    let this = build();
    let answer = match serde_json::from_str::<RequestLine>(&line) {
        Ok(request) if request.is_taken_by(&this) => handle(request.request),
        Ok(_) => Answer::OtherBuild,
        Err(error) => match serde_json::from_str::<VersionOnly>(&line) {
            Ok(sent) if sent.version != this => Answer::OtherBuild,
            _ => return Err(error).context("the request is not a request line"),
        },
    };
    let reply = serde_json::to_string(&AnswerLine {
        version: this,
        answer,
    })?;
    let mut stream = stream;
    writeln!(stream, "{reply}").context("could not write the answer")
}

#[derive(Deserialize)]
struct VersionOnly {
    version: String,
}

/// One request and its answer. `None` when no app is behind the socket.
fn exchange(socket: &Path, request: &RequestLine) -> anyhow::Result<Option<AnswerLine>> {
    let Ok(mut stream) = UnixStream::connect(socket) else {
        return Ok(None);
    };
    stream.set_read_timeout(Some(QUIT_WAIT))?;
    writeln!(stream, "{}", request.to_line()?).context("could not send the request")?;
    let mut reply = String::new();
    BufReader::new(stream)
        .read_line(&mut reply)
        .context("the app did not answer")?;
    serde_json::from_str(&reply)
        .map(Some)
        .context("the app's answer is not an answer line")
}

/// Tells the running app a hand-off was answered. `None` when no app runs.
pub fn tell_app(folder: &AppFolder, request: &Request) -> anyhow::Result<Option<Answer>> {
    Ok(exchange(
        &folder.socket(),
        &RequestLine::for_any_build(request.clone()),
    )?
    .map(|line| line.answer))
}

/// Hands the request to the running app, starting one when none answers. An app of another build that
/// does not take it is replaced only when `caller` may end it.
pub fn hand_over(folder: &AppFolder, request: &Request, caller: &Caller) -> anyhow::Result<Answer> {
    let socket = folder.socket();
    let line = RequestLine {
        across_builds: caller.across_builds(),
        ..RequestLine::new(request.clone())
    };
    match exchange(&socket, &line)? {
        Some(answer) if answer.answer == Answer::OtherBuild && answer.version != build() => {
            replace(&socket, &answer.version, caller)?;
        }
        Some(answer) => return Ok(answer.answer),
        None => {}
    }
    start_and_hand(folder, &line)
}

/// Quits the running app, of this build or of one `caller` may end, then starts one and hands it the
/// request: the way the app's Restart picks up a reinstalled binary.
pub fn restart(folder: &AppFolder, request: &Request, caller: &Caller) -> anyhow::Result<Answer> {
    let socket = folder.socket();
    if let Some(running) = exchange(&socket, &RequestLine::asking_which_build())? {
        if running.version == build() {
            exchange(&socket, &RequestLine::new(Request::Quit))?;
            wait_until(QUIT_WAIT, || !is_answering(&socket)).context("the app did not quit")?;
        } else {
            replace(&socket, &running.version, caller)?;
        }
    }
    start_and_hand(folder, &RequestLine::new(request.clone()))
}

fn replace(socket: &Path, running: &str, caller: &Caller) -> anyhow::Result<()> {
    match caller.on_other_build(running) {
        OnOtherBuild::Stop(stop) => Err(stop.into()),
        OnOtherBuild::Replace => {
            exchange(socket, &RequestLine::quit_replacing(running))?;
            wait_until(QUIT_WAIT, || !is_answering(socket))
                .context("the app of another build did not quit")
        }
    }
}

/// An app of an earlier version may have kept `~/.guided-review` from moving; with it gone the move is
/// tried again first, so this start and the app it starts use the same folders.
fn start_and_hand(folder: &AppFolder, request: &RequestLine) -> anyhow::Result<Answer> {
    let folder = &AppFolder::of(&folder.folders.settle_again()?);
    let socket = folder.socket();
    start_app(folder)?;
    let mut answer = None;
    wait_until(START_WAIT, || {
        answer = exchange(&socket, request).ok().flatten();
        answer.is_some()
    })
    .with_context(|| format!("the app did not start; see {}", folder.log().display()))?;
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
        None => ReleaseInstall::in_home(folder.folders.home())
            .desktop_entry()
            .start_program_for(
                &crate::platform::running_binary()
                    .context("could not find the guided-review binary")?,
            ),
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
