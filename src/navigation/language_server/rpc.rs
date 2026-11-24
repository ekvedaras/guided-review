//! JSON-RPC over a child process's stdio, as language servers speak it: `Content-Length` framed
//! messages, requests answered by id, progress and server status noted for the one who waits.

use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::StatusReports;

pub(super) fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Why a request got no answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Failure {
    /// The process is gone.
    Died,
    /// Nothing came back within the time given.
    Silent,
    /// The server answered with an error.
    Refused(String),
}

type Pending = Arc<Mutex<HashMap<u64, Sender<Result<Value, Failure>>>>>;

/// What the server said it is busy with.
#[derive(Default)]
struct Busy {
    progress: HashSet<String>,
    quiescent: Option<bool>,
    /// The server's first status may come after the reply to the first request, so a server that
    /// reports status is busy until it has.
    status: StatusReports,
}

impl Busy {
    fn is_busy(&self) -> bool {
        let status_due = self.status == StatusReports::Yes && self.quiescent.is_none();
        !self.progress.is_empty() || self.quiescent == Some(false) || status_due
    }
}

struct Shared {
    busy: Mutex<Busy>,
    changed: Condvar,
}

pub(super) struct Connection {
    child: Mutex<Child>,
    stdin: Arc<Mutex<ChildStdin>>,
    pending: Pending,
    next_id: AtomicU64,
    shared: Arc<Shared>,
}

impl Connection {
    /// `settings` answers the server's `workspace/configuration` requests.
    pub(super) fn spawn(
        program: &PathBuf,
        args: &[String],
        folder: &std::path::Path,
        settings: Value,
        status: StatusReports,
    ) -> std::io::Result<Self> {
        let mut child = Command::new(program)
            .args(args)
            .current_dir(folder)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let missing = |what| std::io::Error::other(format!("no {what} on the process"));
        let stdin = Arc::new(Mutex::new(
            child.stdin.take().ok_or_else(|| missing("stdin"))?,
        ));
        let stdout = child.stdout.take().ok_or_else(|| missing("stdout"))?;
        let pending: Pending = Arc::default();
        let shared = Arc::new(Shared {
            busy: Mutex::new(Busy {
                status,
                ..Busy::default()
            }),
            changed: Condvar::new(),
        });
        let reader = Reader {
            pending: Arc::clone(&pending),
            shared: Arc::clone(&shared),
            stdin: Arc::clone(&stdin),
            settings,
        };
        std::thread::spawn(move || reader.run(BufReader::new(stdout)));
        Ok(Self {
            child: Mutex::new(child),
            stdin,
            pending,
            next_id: AtomicU64::new(1),
            shared,
        })
    }

    pub(super) fn is_alive(&self) -> bool {
        locked(&self.child)
            .try_wait()
            .is_ok_and(|status| status.is_none())
    }

    pub(super) fn notify(&self, method: &str, params: &Value) -> Result<(), Failure> {
        write_message(
            &self.stdin,
            &json!({"jsonrpc": "2.0", "method": method, "params": params}),
        )
    }

    pub(super) fn request(
        &self,
        method: &str,
        params: &Value,
        within: Duration,
    ) -> Result<Value, Failure> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (answer, answered) = mpsc::channel();
        locked(&self.pending).insert(id, answer);
        let sent = write_message(
            &self.stdin,
            &json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}),
        );
        if let Err(failure) = sent {
            locked(&self.pending).remove(&id);
            return Err(failure);
        }
        let outcome = match answered.recv_timeout(within) {
            Ok(outcome) => outcome,
            Err(mpsc::RecvTimeoutError::Timeout) => Err(Failure::Silent),
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(Failure::Died),
        };
        locked(&self.pending).remove(&id);
        outcome
    }

    /// Waits until the server reports no work in progress. `indexing` is called once, when the server
    /// turns out to be busy.
    pub(super) fn wait_until_quiet(
        &self,
        within: Duration,
        indexing: &dyn Fn(),
    ) -> Result<(), Failure> {
        let deadline = Instant::now() + within;
        let mut told = false;
        let mut busy = locked(&self.shared.busy);
        while busy.is_busy() {
            if !told {
                told = true;
                indexing();
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(Failure::Silent);
            }
            if !self.is_alive() {
                return Err(Failure::Died);
            }
            let wait = left.min(Duration::from_millis(200));
            busy = self
                .shared
                .changed
                .wait_timeout(busy, wait)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
        Ok(())
    }

    /// Asks the server to shut down and waits a moment for it, then kills what is left.
    pub(super) fn stop(&self) {
        let grace = Duration::from_secs(2);
        if self.request("shutdown", &Value::Null, grace).is_ok() {
            self.notify("exit", &Value::Null).ok();
            let deadline = Instant::now() + grace;
            while self.is_alive() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(20));
            }
        }
        self.kill();
    }

    fn kill(&self) {
        let mut child = locked(&self.child);
        // The process may be gone already; both calls then have nothing to do.
        child.kill().ok();
        child.wait().ok();
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        self.kill();
    }
}

fn write_message(stdin: &Mutex<ChildStdin>, message: &Value) -> Result<(), Failure> {
    let body = message.to_string();
    let mut stdin = locked(stdin);
    write!(stdin, "Content-Length: {}\r\n\r\n{body}", body.len())
        .and_then(|()| stdin.flush())
        .map_err(|_broken_pipe| Failure::Died)
}

struct Reader {
    pending: Pending,
    shared: Arc<Shared>,
    stdin: Arc<Mutex<ChildStdin>>,
    settings: Value,
}

impl Reader {
    fn run(self, mut from: BufReader<impl Read>) {
        while let Some(message) = read_message(&mut from) {
            self.handle(&message);
        }
        let mut pending = locked(&self.pending);
        for (_, waiting) in pending.drain() {
            // The one waiting may have given up already.
            waiting.send(Err(Failure::Died)).ok();
        }
        self.shared.changed.notify_all();
    }

    fn handle(&self, message: &Value) {
        let method = message["method"].as_str();
        match (method, message.get("id")) {
            (Some(method), Some(id)) => self.answer_server(method, id, &message["params"]),
            (Some(method), None) => self.note(method, &message["params"]),
            (None, Some(id)) => self.settle(id, message),
            (None, None) => {}
        }
    }

    fn answer_server(&self, method: &str, id: &Value, params: &Value) {
        let result = if method == "workspace/configuration" {
            let items = params["items"].as_array().map_or(1, Vec::len);
            Value::Array(vec![self.settings.clone(); items])
        } else {
            Value::Null
        };
        // A server that is gone cannot hear the answer; the reader ends with it.
        write_message(
            &self.stdin,
            &json!({"jsonrpc": "2.0", "id": id, "result": result}),
        )
        .ok();
    }

    fn note(&self, method: &str, params: &Value) {
        let mut busy = locked(&self.shared.busy);
        match method {
            "$/progress" => {
                let token = params["token"].to_string();
                match params.pointer("/value/kind").and_then(Value::as_str) {
                    Some("begin") => {
                        busy.progress.insert(token);
                    }
                    Some("end") => {
                        busy.progress.remove(&token);
                    }
                    _ => {}
                }
            }
            "experimental/serverStatus" => busy.quiescent = params["quiescent"].as_bool(),
            _ => return,
        }
        self.shared.changed.notify_all();
    }

    fn settle(&self, id: &Value, message: &Value) {
        let Some(id) = id.as_u64() else { return };
        let Some(waiting) = locked(&self.pending).remove(&id) else {
            return;
        };
        let outcome = match message.get("error") {
            Some(error) => Err(Failure::Refused(
                error["message"].as_str().unwrap_or("an error").to_owned(),
            )),
            None => Ok(message["result"].clone()),
        };
        // The one waiting may have given up already.
        waiting.send(outcome).ok();
    }
}

fn read_message(from: &mut BufReader<impl Read>) -> Option<Value> {
    let mut length = None;
    loop {
        let mut header = String::new();
        if from.read_line(&mut header).ok()? == 0 {
            return None;
        }
        let header = header.trim();
        if header.is_empty() {
            break;
        }
        if let Some((name, value)) = header.split_once(':')
            && name.eq_ignore_ascii_case("content-length")
        {
            length = value.trim().parse::<usize>().ok();
        }
    }
    let mut body = vec![0; length?];
    from.read_exact(&mut body).ok()?;
    serde_json::from_slice(&body).ok()
}
