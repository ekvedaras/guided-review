use std::sync::Mutex;

use guided_review::viewer::{AvatarPort, Avatars};

const ALICE: &str = "https://avatars.githubusercontent.com/u/1?v=4";

/// Answers every avatar with its address as bytes, or fails, and logs what it was asked for.
#[derive(Default)]
struct FakeAvatars {
    asked: Mutex<Vec<String>>,
    fails: bool,
}

impl AvatarPort for FakeAvatars {
    fn avatar(&self, url: &str) -> anyhow::Result<Vec<u8>> {
        self.asked.lock().unwrap().push(url.to_owned());
        if self.fails {
            anyhow::bail!("no network");
        }
        Ok(url.as_bytes().to_vec())
    }
}

#[test]
fn an_avatar_wanted_twice_is_downloaded_once_and_then_shown() {
    let port = FakeAvatars::default();
    let mut avatars = Avatars::default();

    let request = avatars.want(ALICE).expect("first want downloads");
    assert!(avatars.want(ALICE).is_none(), "already on its way");
    assert!(avatars.get(ALICE).is_none(), "not there yet");
    avatars.landed(request.run(&port));

    assert_eq!(
        avatars.get(ALICE).map(|bytes| &bytes[..]),
        Some(ALICE.as_bytes())
    );
    assert!(avatars.want(ALICE).is_none());
    assert_eq!(*port.asked.lock().unwrap(), [ALICE]);
}

#[test]
fn an_avatar_that_failed_shows_none_and_is_not_asked_for_again() {
    let port = FakeAvatars {
        fails: true,
        ..FakeAvatars::default()
    };
    let mut avatars = Avatars::default();

    let request = avatars.want(ALICE).unwrap();
    avatars.landed(request.run(&port));

    assert!(avatars.get(ALICE).is_none());
    assert!(avatars.want(ALICE).is_none());
}
