use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use guided_review::app_link::AppFolder;

const BUILD: &str = "test-build";

use crate::support::fake_app::accept_request;
use crate::support::{FakeGitHub, FixtureRepo, GuidedReview};

const PR: &str = "https://github.com/acme/shop/pull/7";

type Seen = Arc<Mutex<Vec<String>>>;

fn shop_with_a_tour_file() -> (FixtureRepo, FakeGitHub) {
    let repo = FixtureRepo::cloned_from("acme/shop");
    let sha = repo.commit("base");
    let tour = guided_review::tour::Tour::path_for(repo.home(), &PR.parse().unwrap());
    std::fs::create_dir_all(tour.parent().unwrap()).unwrap();
    std::fs::write(tour, "{}").unwrap();
    let github = FakeGitHub::serving_pull("acme/shop", 7, &sha, &sha);
    (repo, github)
}

/// A script that stands for the app binary: it only records that it was started.
fn start_script(home: &Path) -> PathBuf {
    let script = home.join("fake-app");
    std::fs::write(&script, "#!/bin/sh\necho \"$@\" > \"$HOME/started\"\n").unwrap();
    std::process::Command::new("chmod")
        .arg("+x")
        .arg(&script)
        .status()
        .unwrap();
    script
}

fn answer_line(version: &str, answer: &str) -> String {
    format!(r#"{{"version":"{version}","answer":{answer}}}"#)
}

fn opening_line() -> String {
    answer_line(BUILD, &format!(r#"{{"opening":{{"pr":"{PR}"}}}}"#))
}

/// Waits for `ready`, then answers each request line with the next of `answers`, recording the requests.
fn fake_app(
    home: &Path,
    ready: Option<PathBuf>,
    answers: Vec<String>,
    seen: Seen,
) -> std::thread::JoinHandle<()> {
    let socket = AppFolder::in_home(home).socket();
    let listener = ready.is_none().then(|| bind(&socket));
    std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(20);
        while ready.as_ref().is_some_and(|file| !file.exists()) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        let listener = listener.unwrap_or_else(|| bind(&socket));
        for answer in answers {
            let stream = accept_request(&listener, "a request line from the CLI");
            let mut line = String::new();
            BufReader::new(&stream).read_line(&mut line).unwrap();
            seen.lock().unwrap().push(line);
            (&stream)
                .write_all(format!("{answer}\n").as_bytes())
                .unwrap();
        }
    })
}

fn bind(socket: &Path) -> UnixListener {
    std::fs::create_dir_all(socket.parent().unwrap()).unwrap();
    std::fs::remove_file(socket).ok();
    UnixListener::bind(socket).unwrap()
}

#[test]
fn open_sends_the_request_to_the_running_app_and_says_what_it_did() {
    let (repo, github) = shop_with_a_tour_file();
    let seen = Seen::default();
    let app = fake_app(repo.home(), None, vec![opening_line()], Arc::clone(&seen));

    let run = GuidedReview::new(&repo, &github)
        .with_env("HC_TERMINAL_ID", Path::new("t1b"))
        .with_env("GUIDED_REVIEW_BUILD", Path::new(BUILD))
        .run(&["open", PR]);

    app.join().unwrap();
    assert!(run.success, "stderr: {}", run.stderr);
    assert!(
        run.stdout.contains(&format!("opening {PR}")),
        "stdout: {}",
        run.stdout
    );
    let request: serde_json::Value = serde_json::from_str(&seen.lock().unwrap()[0]).unwrap();
    assert_eq!(request["version"], BUILD);
    assert_eq!(request["request"]["open"]["pr"], PR);
    assert_eq!(request["request"]["open"]["terminal"], "t1b");
    let folder = request["request"]["open"]["folder"].as_str().unwrap();
    assert_eq!(
        Path::new(folder).canonicalize().unwrap(),
        repo.path().canonicalize().unwrap()
    );
}

#[test]
fn open_starts_the_app_when_none_answers_then_hands_it_the_pr() {
    let (repo, github) = shop_with_a_tour_file();
    let script = start_script(repo.home());
    let seen = Seen::default();
    let app = fake_app(
        repo.home(),
        Some(repo.home().join("started")),
        vec![opening_line()],
        Arc::clone(&seen),
    );

    let run = GuidedReview::new(&repo, &github)
        .with_env("GUIDED_REVIEW_APP_COMMAND", &script)
        .with_env("GUIDED_REVIEW_BUILD", Path::new(BUILD))
        .run(&["open", PR]);

    app.join().unwrap();
    assert!(run.success, "stderr: {}", run.stderr);
    assert!(repo.home().join("started").exists());
    assert_eq!(seen.lock().unwrap().len(), 1);
}

#[test]
fn open_asks_an_app_of_an_older_build_to_quit_and_starts_a_new_one() {
    let (repo, github) = shop_with_a_tour_file();
    let script = start_script(repo.home());
    let seen = Seen::default();
    let old_app = fake_app(
        repo.home(),
        None,
        vec![
            answer_line("0.0.0-older", r#""other_build""#),
            answer_line("0.0.0-older", r#""quitting""#),
        ],
        Arc::clone(&seen),
    );
    let new_app = {
        let home = repo.home().to_path_buf();
        let seen = Arc::clone(&seen);
        std::thread::spawn(move || {
            old_app.join().unwrap();
            std::fs::remove_file(AppFolder::in_home(&home).socket()).unwrap();
            fake_app(
                &home,
                Some(home.join("started")),
                vec![opening_line()],
                seen,
            )
            .join()
            .unwrap();
        })
    };

    let run = GuidedReview::new(&repo, &github)
        .with_env("GUIDED_REVIEW_APP_COMMAND", &script)
        .with_env("GUIDED_REVIEW_BUILD", Path::new(BUILD))
        .run(&["open", PR]);

    new_app.join().unwrap();
    assert!(run.success, "stderr: {}", run.stderr);
    let requests: Vec<serde_json::Value> = seen
        .lock()
        .unwrap()
        .iter()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(requests.len(), 3);
    assert!(requests[0]["request"]["open"].is_object());
    assert_eq!(requests[1]["request"], "quit");
    assert!(requests[2]["request"]["open"].is_object());
}

#[test]
fn a_refusal_from_the_app_fails_the_open_with_its_reason() {
    let (repo, github) = shop_with_a_tour_file();
    let app = fake_app(
        repo.home(),
        None,
        vec![answer_line(
            BUILD,
            r#"{"refused":{"reason":"busy elsewhere"}}"#,
        )],
        Seen::default(),
    );

    let run = GuidedReview::new(&repo, &github)
        .with_env("GUIDED_REVIEW_BUILD", Path::new(BUILD))
        .run(&["open", PR]);

    app.join().unwrap();
    assert!(!run.success);
    assert!(
        run.stderr.contains("busy elsewhere"),
        "stderr: {}",
        run.stderr
    );
}

#[test]
fn the_no_tour_refusal_comes_before_any_app_is_asked_or_started() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    let sha = repo.commit("base");
    let github = FakeGitHub::serving_pull("acme/shop", 7, &sha, &sha);
    let script = start_script(repo.home());

    let run = GuidedReview::new(&repo, &github)
        .with_env("GUIDED_REVIEW_APP_COMMAND", &script)
        .with_env("GUIDED_REVIEW_BUILD", Path::new(BUILD))
        .run(&["open", PR]);

    assert!(!run.success);
    assert!(run.stderr.contains("run `/tour"), "stderr: {}", run.stderr);
    assert!(!repo.home().join("started").exists());
}
