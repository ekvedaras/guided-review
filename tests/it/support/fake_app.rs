//! The socket side of a fake running app.

use std::io::ErrorKind;
use std::os::unix::net::{UnixListener, UnixStream};
use std::thread;
use std::time::{Duration, Instant};

/// Long enough for a CLI process to start and connect, short enough that a red run fails in seconds.
pub const REQUEST_DEADLINE: Duration = Duration::from_secs(5);

/// The next connection, or a panic naming `waiting_for` once `deadline` passes.
pub fn accept_within(listener: &UnixListener, deadline: Duration, waiting_for: &str) -> UnixStream {
    listener.set_nonblocking(true).unwrap();
    let give_up = Instant::now() + deadline;
    let stream = loop {
        match listener.accept() {
            Ok((stream, _)) => break stream,
            Err(error) if error.kind() == ErrorKind::WouldBlock && Instant::now() < give_up => {
                thread::sleep(Duration::from_millis(10));
            }
            Err(error) => panic!("fake app gave up waiting for {waiting_for}: {error}"),
        }
    };
    stream.set_nonblocking(false).unwrap();
    stream
}

/// The next connection, or a panic naming `waiting_for` after `REQUEST_DEADLINE`.
pub fn accept_request(listener: &UnixListener, waiting_for: &str) -> UnixStream {
    accept_within(listener, REQUEST_DEADLINE, waiting_for)
}
