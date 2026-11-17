use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;

use guided_review::app_link::{
    Answer, AnswerLine, AppFolder, AppListener, BindError, Caller, OnOtherBuild, OpenRequest,
    OtherBuildRuns, Request, RequestLine, build,
};

use crate::support::FixtureRepo;

fn open_request() -> OpenRequest {
    OpenRequest {
        subject: "https://github.com/acme/shop/pull/7".to_owned(),
        folder: "/work/shop".into(),
        terminal: Some("t1b".to_owned()),
        at_step: None,
    }
}

const LOCAL_SUBJECT: &str = "local:/work/shop@feature/cart";

fn open_json_of(request: OpenRequest) -> serde_json::Value {
    let line = RequestLine::new(Request::Open(request)).to_line().unwrap();
    let json: serde_json::Value = serde_json::from_str(&line).unwrap();
    json["request"]["open"].clone()
}

fn open_request_read_from(open: &serde_json::Value) -> OpenRequest {
    let line = serde_json::json!({"version": build(), "request": {"open": open}}).to_string();
    let RequestLine {
        request: Request::Open(request),
        ..
    } = serde_json::from_str(&line).unwrap()
    else {
        panic!("not an open request: {line}");
    };
    request
}

#[test]
fn an_open_request_of_a_pr_names_its_subject_and_also_the_pr_an_older_app_reads() {
    let open = open_json_of(open_request());

    assert_eq!(open["subject"], "https://github.com/acme/shop/pull/7");
    assert_eq!(open["pr"], "https://github.com/acme/shop/pull/7");
}

#[test]
fn an_open_request_of_a_local_change_set_names_only_its_subject() {
    let open = open_json_of(OpenRequest {
        subject: LOCAL_SUBJECT.to_owned(),
        ..open_request()
    });

    assert_eq!(open["subject"], LOCAL_SUBJECT);
    assert!(open.get("pr").is_none(), "{open}");
}

#[test]
fn an_open_request_from_an_older_build_names_its_subject_by_the_pr() {
    let request = open_request_read_from(&serde_json::json!({
        "pr": "https://github.com/acme/shop/pull/7",
        "folder": "/work/shop",
        "terminal": "t1b",
    }));

    assert_eq!(request, open_request());
}

#[test]
fn an_open_request_reads_back_as_it_was_written() {
    let local = OpenRequest {
        subject: LOCAL_SUBJECT.to_owned(),
        ..open_request()
    };

    for request in [open_request(), local] {
        assert_eq!(
            open_request_read_from(&open_json_of(request.clone())),
            request
        );
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
    let folder = AppFolder::of(&repo.folders());

    let first = AppListener::bind(&folder).unwrap();
    let second = AppListener::bind(&folder);

    assert!(matches!(second, Err(BindError::AlreadyRunning)));
    drop(first);
    assert!(bind_when_free(&folder).is_ok());
}

#[test]
fn a_socket_file_with_no_app_behind_it_is_cleaned_up() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    let folder = AppFolder::of(&repo.folders());
    drop(AppListener::bind(&folder).unwrap());
    assert!(folder.socket().exists());

    assert!(bind_when_free(&folder).is_ok());
}

#[test]
fn the_listener_answers_each_request_line_through_the_handler() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    let folder = AppFolder::of(&repo.folders());
    AppListener::bind(&folder)
        .unwrap()
        .serve(|request| match request {
            Request::Open(open) => Answer::Opening { pr: open.subject },
            Request::AgentReplied { pr, .. } => Answer::HandOffAnswered { pr },
            Request::ShowSettings => Answer::ShowingSettings,
            Request::ShowSwitcher => Answer::ShowingSwitcher,
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

fn of_older_build(mut line: RequestLine) -> RequestLine {
    line.version = "0.0.0-older".to_owned();
    line
}

fn listening_app(repo: &FixtureRepo) -> AppFolder {
    let folder = AppFolder::of(&repo.folders());
    AppListener::bind(&folder)
        .unwrap()
        .serve(|request| match request {
            Request::Quit => Answer::Quitting,
            _ => Answer::ShowingSwitcher,
        });
    folder
}

#[test]
fn a_request_of_another_build_is_acted_on_only_when_it_says_any_build_may_take_it() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    let folder = listening_app(&repo);

    let unmarked = ask(
        &folder,
        &of_older_build(RequestLine::new(Request::ShowSwitcher)),
    );
    let marked = ask(
        &folder,
        &of_older_build(RequestLine::for_any_build(Request::ShowSwitcher)),
    );

    assert_eq!(unmarked.answer, Answer::OtherBuild);
    assert_eq!(unmarked.version, build());
    assert_eq!(marked.answer, Answer::ShowingSwitcher);
}

#[test]
fn a_quit_of_another_build_is_acted_on_only_when_it_names_this_build_as_the_one_it_ends() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    let folder = listening_app(&repo);

    let blind = ask(&folder, &of_older_build(RequestLine::new(Request::Quit)));
    let for_any_build = ask(
        &folder,
        &of_older_build(RequestLine::for_any_build(Request::Quit)),
    );
    let of_another_app = ask(
        &folder,
        &of_older_build(RequestLine::quit_replacing("0.0.0-elsewhere")),
    );
    let of_this_app = ask(
        &folder,
        &of_older_build(RequestLine::quit_replacing(&build())),
    );

    assert_eq!(blind.answer, Answer::OtherBuild);
    assert_eq!(for_any_build.answer, Answer::OtherBuild);
    assert_eq!(of_another_app.answer, Answer::OtherBuild);
    assert_eq!(of_this_app.answer, Answer::Quitting);
}

#[test]
fn a_request_this_build_cannot_read_is_answered_as_another_builds() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    let folder = listening_app(&repo);
    let mut stream = UnixStream::connect(folder.socket()).unwrap();
    writeln!(
        stream,
        r#"{{"version":"9.9.9-newer","request":{{"from_the_future":{{}}}}}}"#
    )
    .unwrap();
    let mut reply = String::new();
    BufReader::new(stream).read_line(&mut reply).unwrap();

    let answer: AnswerLine = serde_json::from_str(&reply).unwrap();

    assert_eq!(answer.answer, Answer::OtherBuild);
    assert_eq!(answer.version, build());
}

#[test]
fn a_command_never_ends_an_app_of_another_build() {
    assert_eq!(
        Caller::Command.on_other_build("0.0.0-older"),
        OnOtherBuild::Stop(OtherBuildRuns {
            running: "0.0.0-older".to_owned(),
            this: build(),
        })
    );
}

#[test]
fn a_launch_from_the_os_launcher_replaces_an_app_of_another_build() {
    assert_eq!(
        Caller::Launcher.on_other_build("0.0.0-older"),
        OnOtherBuild::Replace
    );
}

#[test]
fn the_apps_restart_ends_only_the_app_that_ran_it() {
    let restart = Caller::AppRestart {
        replacing: "0.0.0-older".to_owned(),
    };

    assert_eq!(restart.on_other_build("0.0.0-older"), OnOtherBuild::Replace);
    assert!(matches!(
        restart.on_other_build("0.0.0-elsewhere"),
        OnOtherBuild::Stop(_)
    ));
}

#[test]
fn the_stop_names_both_builds_and_says_the_app_was_left_running() {
    let message = OtherBuildRuns {
        running: "0.0.0-older".to_owned(),
        this: "0.0.0-newer".to_owned(),
    }
    .to_string();

    assert!(message.contains("0.0.0-older"), "{message}");
    assert!(message.contains("0.0.0-newer"), "{message}");
    assert!(message.contains("left running"), "{message}");
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
