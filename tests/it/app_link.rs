use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;

use guided_review::app_link::{
    Answer, AnswerLine, AppFolder, AppListener, BindError, OpenRequest, Request, RequestLine, build,
};

use crate::support::FixtureRepo;

fn open_request() -> OpenRequest {
    OpenRequest {
        pr: "https://github.com/acme/shop/pull/7".to_owned(),
        folder: "/work/shop".into(),
        terminal: Some("t1b".to_owned()),
        at_step: None,
    }
}

fn ask(folder: &AppFolder, request: &RequestLine) -> AnswerLine {
    let mut stream = UnixStream::connect(folder.socket()).unwrap();
    writeln!(stream, "{}", request.to_line().unwrap()).unwrap();
    let mut reply = String::new();
    BufReader::new(stream).read_line(&mut reply).unwrap();
    serde_json::from_str(&reply).unwrap()
}

#[test]
fn a_request_is_one_json_line_with_the_version_and_the_open_fields() {
    let line = RequestLine::new(Request::Open(open_request()))
        .to_line()
        .unwrap();

    assert!(!line.contains('\n'));
    let json: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert_eq!(json["version"], build());
    assert_eq!(
        json["request"]["open"]["pr"],
        "https://github.com/acme/shop/pull/7"
    );
    assert_eq!(json["request"]["open"]["folder"], "/work/shop");
    assert_eq!(json["request"]["open"]["terminal"], "t1b");
}

#[test]
fn a_second_app_cannot_take_the_lock_while_the_first_lives() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    let folder = AppFolder::in_home(repo.home());

    let first = AppListener::bind(&folder).unwrap();
    let second = AppListener::bind(&folder);

    assert!(matches!(second, Err(BindError::AlreadyRunning)));
    drop(first);
    assert!(bind_when_free(&folder).is_ok());
}

#[test]
fn a_socket_file_with_no_app_behind_it_is_cleaned_up() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    let folder = AppFolder::in_home(repo.home());
    drop(AppListener::bind(&folder).unwrap());
    assert!(folder.socket().exists());

    assert!(bind_when_free(&folder).is_ok());
}

#[test]
fn the_listener_answers_each_request_line_through_the_handler() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    let folder = AppFolder::in_home(repo.home());
    AppListener::bind(&folder)
        .unwrap()
        .serve(|request| match request {
            Request::Open(open) => Answer::Opening { pr: open.pr },
            Request::AgentReplied { pr, .. } => Answer::HandOffAnswered { pr },
            Request::ShowSettings => Answer::ShowingSettings,
            Request::Quit => Answer::Quitting,
        });

    let answer = ask(&folder, &RequestLine::new(Request::Open(open_request())));

    assert_eq!(answer.version, build());
    assert_eq!(
        answer.answer,
        Answer::Opening {
            pr: "https://github.com/acme/shop/pull/7".to_owned()
        }
    );
}

#[test]
fn a_request_of_another_build_is_not_acted_on_but_a_quit_is() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    let folder = AppFolder::in_home(repo.home());
    AppListener::bind(&folder)
        .unwrap()
        .serve(|_| Answer::Quitting);
    let mut older_open = RequestLine::new(Request::Open(open_request()));
    older_open.version = "0.0.0-older".to_owned();
    let mut older_quit = RequestLine::new(Request::Quit);
    older_quit.version = "0.0.0-older".to_owned();

    let open = ask(&folder, &older_open);
    let quit = ask(&folder, &older_quit);

    assert_eq!(open.answer, Answer::OtherBuild);
    assert_eq!(open.version, build());
    assert_eq!(quit.answer, Answer::Quitting);
}

/// Other tests fork child processes, and a child holds a copy of the lock until it has exec'd.
fn bind_when_free(folder: &AppFolder) -> Result<AppListener, BindError> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        match AppListener::bind(folder) {
            Err(BindError::AlreadyRunning) if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            other => return other,
        }
    }
}
