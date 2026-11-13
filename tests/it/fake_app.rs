use std::os::unix::net::{UnixListener, UnixStream};

use crate::support::fake_app::{CliEnded, accept_before};

#[test]
#[should_panic(expected = "the CLI ended before it sent the open request")]
fn a_fake_app_whose_cli_ended_without_connecting_fails_naming_the_request_it_waited_for() {
    let folder = tempfile::tempdir().unwrap();
    let listener = UnixListener::bind(folder.path().join("app.sock")).unwrap();
    let ended = CliEnded::default();
    ended.mark();

    accept_before(&listener, &ended, "the open request");
}

#[test]
fn a_connection_made_before_the_cli_ended_is_still_accepted() {
    let folder = tempfile::tempdir().unwrap();
    let socket = folder.path().join("app.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    let _cli = UnixStream::connect(&socket).unwrap();
    let ended = CliEnded::default();
    ended.mark();

    accept_before(&listener, &ended, "the open request");
}
