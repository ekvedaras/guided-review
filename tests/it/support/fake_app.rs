//! The socket side of a fake running app.

use std::io::ErrorKind;
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
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
