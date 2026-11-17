//! The socket side of a fake running app.

use std::io::{BufRead, BufReader, ErrorKind, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use guided_review::app_link::AppFolder;

use super::folders_in;
use std::thread;
use std::time::Duration;

/// Marked by the test once the CLI process under test has ended. A fake app waits for the CLI until then,
/// never by the clock, so a busy machine cannot fail it.
#[derive(Clone, Default)]
pub struct CliEnded(Arc<AtomicBool>);

impl CliEnded {
    pub fn mark(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    pub fn is_marked(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

const POLL: Duration = Duration::from_millis(10);

/// An app of `build` on the socket of `home` that answers one request line with `answer`, recording
/// the line.
pub fn answering_one_request(
    home: &Path,
    cli_ended: &CliEnded,
    build: &str,
    answer: &str,
) -> (thread::JoinHandle<()>, Arc<Mutex<Vec<String>>>) {
    let cli_ended = cli_ended.clone();
    let socket = AppFolder::of(&folders_in(home)).socket();
    std::fs::create_dir_all(socket.parent().unwrap()).unwrap();
    let listener = UnixListener::bind(socket).unwrap();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&seen);
    let line = format!(r#"{{"version":"{build}","answer":{answer}}}"#);
    let thread = thread::spawn(move || {
        let stream = accept_before(&listener, &cli_ended, "the request from the CLI");
        let mut request = String::new();
        BufReader::new(&stream).read_line(&mut request).unwrap();
        log.lock().unwrap().push(request);
        (&stream).write_all(format!("{line}\n").as_bytes()).unwrap();
    });
    (thread, seen)
}

/// The next connection, or a panic naming `waiting_for` once the CLI ended without making it.
pub fn accept_before(
    listener: &UnixListener,
    cli_ended: &CliEnded,
    waiting_for: &str,
) -> UnixStream {
    listener.set_nonblocking(true).unwrap();
    let stream = loop {
        let last_try = cli_ended.is_marked();
        match listener.accept() {
            Ok((stream, _)) => break stream,
            Err(error) if error.kind() == ErrorKind::WouldBlock && !last_try => thread::sleep(POLL),
            Err(error) => panic!("the CLI ended before it sent {waiting_for}: {error}"),
        }
    };
    stream.set_nonblocking(false).unwrap();
    stream
}
