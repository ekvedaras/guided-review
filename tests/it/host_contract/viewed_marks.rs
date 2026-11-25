use guided_review::viewer::{HostError, Viewed, ViewedState, ViewedState::*};

use super::{HostBuilder, PullOnHost, Reach, pr};
use crate::support::file_path;

fn viewed_states(host: &dyn guided_review::viewer::CodeHost) -> Vec<(String, ViewedState)> {
    let mut states: Vec<_> = host
        .viewed_marks()
        .expect("the host offers viewed marks")
        .viewed_files(&pr())
        .unwrap()
        .into_iter()
        .map(|file| (file.path.to_string(), file.state))
        .collect();
    states.sort_by(|a, b| a.0.cmp(&b.0));
    states
}

fn owned(states: &[(&str, ViewedState)]) -> Vec<(String, ViewedState)> {
    states
        .iter()
        .map(|(path, state)| ((*path).to_owned(), *state))
        .collect()
}

pub fn viewed_files_list_every_changed_file_with_its_state(host: HostBuilder) {
    let files = vec![("a.rs", Viewed), ("b.rs", Unviewed), ("c.rs", Dismissed)];
    let host = host(PullOnHost {
        files: files.clone(),
        ..PullOnHost::default()
    });

    assert_eq!(viewed_states(host.as_ref()), owned(&files));
}

pub fn a_file_marked_viewed_reads_back_viewed_and_unmarked_reads_back_unviewed(host: HostBuilder) {
    let host = host(PullOnHost {
        files: vec![("a.rs", Unviewed), ("b.rs", Dismissed), ("c.rs", Viewed)],
        ..PullOnHost::default()
    });
    let marks = host.viewed_marks().expect("the host offers viewed marks");

    marks
        .set_file_viewed(&pr(), &file_path("a.rs"), Viewed::Yes)
        .unwrap();
    marks
        .set_file_viewed(&pr(), &file_path("b.rs"), Viewed::Yes)
        .unwrap();
    marks
        .set_file_viewed(&pr(), &file_path("c.rs"), Viewed::No)
        .unwrap();

    assert_eq!(
        viewed_states(host.as_ref()),
        owned(&[("a.rs", Viewed), ("b.rs", Viewed), ("c.rs", Unviewed)])
    );
}

pub fn viewed_marks_without_a_token_are_refused(host: HostBuilder) {
    let host = host(PullOnHost {
        files: vec![("a.rs", Unviewed)],
        reach: Reach::NoToken,
        ..PullOnHost::default()
    });
    let marks = host.viewed_marks().expect("the host offers viewed marks");

    let read = marks.viewed_files(&pr()).unwrap_err();
    let write = marks
        .set_file_viewed(&pr(), &file_path("a.rs"), Viewed::Yes)
        .unwrap_err();

    assert!(matches!(read, HostError::NoAccess(_)), "{read:?}");
    assert!(matches!(write, HostError::NoAccess(_)), "{write:?}");
}

pub fn viewed_marks_of_a_host_not_answering_fail_as_an_outage(host: HostBuilder) {
    let host = host(PullOnHost {
        files: vec![("a.rs", Unviewed)],
        reach: Reach::ServerError,
        ..PullOnHost::default()
    });
    let marks = host.viewed_marks().expect("the host offers viewed marks");

    let read = marks.viewed_files(&pr()).unwrap_err();
    let write = marks.set_file_viewed(&pr(), &file_path("a.rs"), Viewed::Yes);

    assert!(matches!(read, HostError::NotAnswering(..)), "{read:?}");
    assert!(
        write.is_err(),
        "a write to a host not answering went through"
    );
}
