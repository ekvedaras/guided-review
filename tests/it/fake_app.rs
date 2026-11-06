use std::os::unix::net::UnixListener;
use std::time::Duration;

use crate::support::fake_app::accept_within;

#[test]
#[should_panic(expected = "gave up waiting for the open request")]
fn a_fake_app_whose_cli_never_connects_fails_naming_the_request_it_waited_for() {
    let folder = tempfile::tempdir().unwrap();
    let listener = UnixListener::bind(folder.path().join("app.sock")).unwrap();

    accept_within(&listener, Duration::from_millis(200), "the open request");
}
