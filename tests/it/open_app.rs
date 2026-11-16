use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use guided_review::app_link::AppFolder;
use guided_review::platform::{APP_ID, IN_LOGIN_SHELL};
use guided_review::update::ReleaseInstall;

const BUILD: &str = "test-build";

use crate::support::fake_app::{CliEnded, accept_before};
use crate::support::{FakeGitHub, FixtureRepo, GuidedReview, folders_in, write_executable};

const PR: &str = "https://github.com/acme/shop/pull/7";

type Seen = Arc<Mutex<Vec<String>>>;

fn shop_with_a_tour_file() -> (FixtureRepo, FakeGitHub) {
    let repo = FixtureRepo::cloned_from("acme/shop");
    let sha = repo.commit("base");
    let tour = guided_review::tour::Tour::path_for(&repo.folders(), &PR.parse().unwrap());
    std::fs::create_dir_all(tour.parent().unwrap()).unwrap();
    std::fs::write(tour, "{}").unwrap();
    let github = FakeGitHub::serving_pull("acme/shop", 7, &sha, &sha);
    (repo, github)
}

/// A script that stands for the app binary: it only records that it was started.
fn start_script(home: &Path) -> PathBuf {
    let script = home.join("fake-app");
    write_executable(&script, "#!/bin/sh\necho \"$@\" > \"$HOME/started\"\n");
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
    cli_ended: &CliEnded,
    ready: Option<PathBuf>,
    answers: Vec<String>,
    seen: Seen,
) -> std::thread::JoinHandle<()> {
    let socket = AppFolder::of(&folders_in(home)).socket();
    let listener = ready.is_none().then(|| bind(&socket));
    let cli_ended = cli_ended.clone();
    std::thread::spawn(move || {
        while ready.as_ref().is_some_and(|file| !file.exists()) && !cli_ended.is_marked() {
            std::thread::sleep(Duration::from_millis(10));
        }
        let listener = listener.unwrap_or_else(|| bind(&socket));
        for answer in answers {
            let stream = accept_before(&listener, &cli_ended, "a request line from the CLI");
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
    let cli_ended = CliEnded::default();
    let seen = Seen::default();
    let app = fake_app(
        repo.home(),
        &cli_ended,
        None,
        vec![opening_line()],
        Arc::clone(&seen),
    );

    let run = GuidedReview::new(&repo, &github)
        .with_env("HC_TERMINAL_ID", Path::new("t1b"))
        .with_env("GUIDED_REVIEW_BUILD", Path::new(BUILD))
        .run(&["open", PR]);

    cli_ended.mark();
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
    let cli_ended = CliEnded::default();
    let script = start_script(repo.home());
    let seen = Seen::default();
    let app = fake_app(
        repo.home(),
        &cli_ended,
        Some(repo.home().join("started")),
        vec![opening_line()],
        Arc::clone(&seen),
    );

    let run = GuidedReview::new(&repo, &github)
        .with_env("GUIDED_REVIEW_APP_COMMAND", &script)
        .with_env("GUIDED_REVIEW_BUILD", Path::new(BUILD))
        .run(&["open", PR]);

    cli_ended.mark();
    app.join().unwrap();
    assert!(run.success, "stderr: {}", run.stderr);
    assert!(repo.home().join("started").exists());
    assert_eq!(seen.lock().unwrap().len(), 1);
}

/// An app of the `old` build that answers `old_answers`, then, once the CLI started a new app, one of
/// this build that answers `new_answers`.
fn old_app_then_new(
    repo: &FixtureRepo,
    cli_ended: &CliEnded,
    old_answers: Vec<String>,
    new_answers: Vec<String>,
    seen: &Seen,
) -> std::thread::JoinHandle<()> {
    let old_app = fake_app(repo.home(), cli_ended, None, old_answers, Arc::clone(seen));
    let home = repo.home().to_path_buf();
    let seen = Arc::clone(seen);
    let cli_ended = cli_ended.clone();
    std::thread::spawn(move || {
        old_app.join().unwrap();
        if new_answers.is_empty() {
            return;
        }
        std::fs::remove_file(AppFolder::of(&folders_in(&home)).socket()).unwrap();
        fake_app(
            &home,
            &cli_ended,
            Some(home.join("started")),
            new_answers,
            seen,
        )
        .join()
        .unwrap();
    })
}

fn requests(seen: &Seen) -> Vec<serde_json::Value> {
    seen.lock()
        .unwrap()
        .iter()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[test]
fn open_leaves_an_app_of_another_build_that_does_not_take_it_running_and_names_both_builds() {
    let (repo, github) = shop_with_a_tour_file();
    let cli_ended = CliEnded::default();
    let script = start_script(repo.home());
    let seen = Seen::default();
    let app = old_app_then_new(
        &repo,
        &cli_ended,
        vec![answer_line("0.0.0-older", r#""other_build""#)],
        vec![],
        &seen,
    );

    let run = GuidedReview::new(&repo, &github)
        .with_env("GUIDED_REVIEW_APP_COMMAND", &script)
        .with_env("GUIDED_REVIEW_BUILD", Path::new(BUILD))
        .run(&["open", PR]);

    cli_ended.mark();
    app.join().unwrap();
    assert!(!run.success);
    assert!(run.stderr.contains("0.0.0-older"), "stderr: {}", run.stderr);
    assert!(run.stderr.contains(BUILD), "stderr: {}", run.stderr);
    assert!(!repo.home().join("started").exists());
    let requests = requests(&seen);
    assert_eq!(requests.len(), 1);
    assert!(requests[0]["request"]["open"].is_object());
    assert_eq!(requests[0]["across_builds"], "any_build");
}

#[test]
fn a_launch_from_the_os_launcher_replaces_an_app_of_another_build() {
    let (repo, github) = shop_with_a_tour_file();
    let cli_ended = CliEnded::default();
    let script = start_script(repo.home());
    let seen = Seen::default();
    let app = old_app_then_new(
        &repo,
        &cli_ended,
        vec![
            answer_line("0.0.0-older", r#""other_build""#),
            answer_line("0.0.0-older", r#""quitting""#),
        ],
        vec![answer_line(BUILD, r#""showing_switcher""#)],
        &seen,
    );
    let launched_app_variable = ReleaseInstall::in_home(repo.home())
        .desktop_entry()
        .launched_app_variable();

    let run = GuidedReview::new(&repo, &github)
        .with_env("GUIDED_REVIEW_APP_COMMAND", &script)
        .with_env("GUIDED_REVIEW_BUILD", Path::new(BUILD))
        .with_env(launched_app_variable, Path::new(APP_ID))
        .with_env(IN_LOGIN_SHELL, Path::new("1"))
        .run(&[]);

    cli_ended.mark();
    app.join().unwrap();
    assert!(run.success, "stderr: {}", run.stderr);
    let requests = requests(&seen);
    assert_eq!(requests.len(), 3);
    assert_eq!(requests[0]["request"], "show_switcher");
    assert_eq!(requests[1]["request"], "quit");
    assert_eq!(requests[1]["replacing"], "0.0.0-older");
    assert_eq!(requests[2]["request"], "show_switcher");
}

#[test]
fn a_launch_from_a_terminal_leaves_an_app_of_another_build_running() {
    let (repo, github) = shop_with_a_tour_file();
    let cli_ended = CliEnded::default();
    let script = start_script(repo.home());
    let seen = Seen::default();
    let app = old_app_then_new(
        &repo,
        &cli_ended,
        vec![answer_line("0.0.0-older", r#""other_build""#)],
        vec![],
        &seen,
    );

    let run = GuidedReview::new(&repo, &github)
        .with_env("GUIDED_REVIEW_APP_COMMAND", &script)
        .with_env("GUIDED_REVIEW_BUILD", Path::new(BUILD))
        .run(&[]);

    cli_ended.mark();
    app.join().unwrap();
    assert!(!run.success);
    assert!(
        run.stderr.contains("left running"),
        "stderr: {}",
        run.stderr
    );
    assert_eq!(requests(&seen).len(), 1);
    assert!(!repo.home().join("started").exists());
}

#[test]
fn restart_leaves_an_app_of_another_build_running() {
    let (repo, github) = shop_with_a_tour_file();
    let cli_ended = CliEnded::default();
    let script = start_script(repo.home());
    let seen = Seen::default();
    let app = old_app_then_new(
        &repo,
        &cli_ended,
        vec![answer_line("0.0.0-older", r#""other_build""#)],
        vec![],
        &seen,
    );

    let run = GuidedReview::new(&repo, &github)
        .with_env("GUIDED_REVIEW_APP_COMMAND", &script)
        .with_env("GUIDED_REVIEW_BUILD", Path::new(BUILD))
        .run(&["restart", PR]);

    cli_ended.mark();
    app.join().unwrap();
    assert!(!run.success);
    assert!(run.stderr.contains("0.0.0-older"), "stderr: {}", run.stderr);
    let requests = requests(&seen);
    assert_eq!(requests.len(), 1);
    assert_ne!(requests[0]["request"], "quit");
    assert!(!repo.home().join("started").exists());
}

#[test]
fn the_apps_restart_ends_the_app_of_the_build_that_ran_it_and_opens_the_pr_in_a_new_one() {
    let (repo, github) = shop_with_a_tour_file();
    let cli_ended = CliEnded::default();
    let script = start_script(repo.home());
    let seen = Seen::default();
    let app = old_app_then_new(
        &repo,
        &cli_ended,
        vec![
            answer_line("0.0.0-older", r#""other_build""#),
            answer_line("0.0.0-older", r#""quitting""#),
        ],
        vec![opening_line()],
        &seen,
    );

    let run = GuidedReview::new(&repo, &github)
        .with_env("GUIDED_REVIEW_APP_COMMAND", &script)
        .with_env("GUIDED_REVIEW_BUILD", Path::new(BUILD))
        .run(&["restart", PR, "--replacing", "0.0.0-older"]);

    cli_ended.mark();
    app.join().unwrap();
    assert!(run.success, "stderr: {}", run.stderr);
    let requests = requests(&seen);
    assert_eq!(requests.len(), 3);
    assert_ne!(requests[0]["request"], "quit");
    assert_eq!(requests[1]["request"], "quit");
    assert_eq!(requests[1]["replacing"], "0.0.0-older");
    assert_eq!(requests[2]["request"]["open"]["pr"], PR);
}

#[test]
fn an_open_the_app_keeps_in_the_background_succeeds_and_says_so() {
    let (repo, github) = shop_with_a_tour_file();
    let cli_ended = CliEnded::default();
    let app = fake_app(
        repo.home(),
        &cli_ended,
        None,
        vec![answer_line(
            BUILD,
            &format!(r#"{{"kept_in_background":{{"pr":"{PR}"}}}}"#),
        )],
        Seen::default(),
    );

    let run = GuidedReview::new(&repo, &github)
        .with_env("GUIDED_REVIEW_BUILD", Path::new(BUILD))
        .run(&["open", PR]);

    cli_ended.mark();
    app.join().unwrap();
    assert!(run.success, "stderr: {}", run.stderr);
    assert!(run.stdout.contains("PR switcher"), "stdout: {}", run.stdout);
}

#[test]
fn a_refusal_from_the_app_fails_the_open_with_its_reason() {
    let (repo, github) = shop_with_a_tour_file();
    let cli_ended = CliEnded::default();
    let app = fake_app(
        repo.home(),
        &cli_ended,
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

    cli_ended.mark();
    app.join().unwrap();
    assert!(!run.success);
    assert!(
        run.stderr.contains("busy elsewhere"),
        "stderr: {}",
        run.stderr
    );
}

fn shop_without_a_tour_file() -> (FixtureRepo, FakeGitHub) {
    let repo = FixtureRepo::cloned_from("acme/shop");
    let sha = repo.commit("base");
    let github = FakeGitHub::serving_pull("acme/shop", 7, &sha, &sha);
    (repo, github)
}

fn writing_line() -> String {
    answer_line(BUILD, &format!(r#"{{"writing":{{"pr":"{PR}"}}}}"#))
}

#[test]
fn open_of_a_pr_without_a_tour_asks_the_running_app_and_says_it_is_writing_one() {
    let (repo, github) = shop_without_a_tour_file();
    let cli_ended = CliEnded::default();
    let seen = Seen::default();
    let app = fake_app(
        repo.home(),
        &cli_ended,
        None,
        vec![writing_line()],
        Arc::clone(&seen),
    );

    let run = GuidedReview::new(&repo, &github)
        .with_env("GUIDED_REVIEW_BUILD", Path::new(BUILD))
        .run(&["open", PR]);

    cli_ended.mark();
    app.join().unwrap();
    assert!(run.success, "stderr: {}", run.stderr);
    assert!(
        run.stdout.contains("No tour") && run.stdout.contains("writing one"),
        "stdout: {}",
        run.stdout
    );
    let request: serde_json::Value = serde_json::from_str(&seen.lock().unwrap()[0]).unwrap();
    assert_eq!(request["request"]["open"]["pr"], PR);
}

#[test]
fn open_of_a_pr_without_a_tour_starts_the_app_when_none_answers_and_hands_it_the_pr() {
    let (repo, github) = shop_without_a_tour_file();
    let cli_ended = CliEnded::default();
    let script = start_script(repo.home());
    let seen = Seen::default();
    let app = fake_app(
        repo.home(),
        &cli_ended,
        Some(repo.home().join("started")),
        vec![writing_line()],
        Arc::clone(&seen),
    );

    let run = GuidedReview::new(&repo, &github)
        .with_env("GUIDED_REVIEW_APP_COMMAND", &script)
        .with_env("GUIDED_REVIEW_BUILD", Path::new(BUILD))
        .run(&["open", PR]);

    cli_ended.mark();
    app.join().unwrap();
    assert!(run.success, "stderr: {}", run.stderr);
    assert!(repo.home().join("started").exists());
    let request: serde_json::Value = serde_json::from_str(&seen.lock().unwrap()[0]).unwrap();
    assert_eq!(request["request"]["open"]["pr"], PR);
}

fn showing_settings_line() -> String {
    answer_line(BUILD, r#""showing_settings""#)
}

#[test]
fn settings_asks_the_running_app_to_show_settings_and_says_so() {
    let (repo, github) = shop_with_a_tour_file();
    let cli_ended = CliEnded::default();
    let seen = Seen::default();
    let app = fake_app(
        repo.home(),
        &cli_ended,
        None,
        vec![showing_settings_line()],
        Arc::clone(&seen),
    );

    let run = GuidedReview::new(&repo, &github)
        .with_env("GUIDED_REVIEW_BUILD", Path::new(BUILD))
        .run(&["settings"]);

    cli_ended.mark();
    app.join().unwrap();
    assert!(run.success, "stderr: {}", run.stderr);
    assert!(run.stdout.contains("Settings"), "stdout: {}", run.stdout);
    let request: serde_json::Value = serde_json::from_str(&seen.lock().unwrap()[0]).unwrap();
    assert_eq!(request["request"], "show_settings");
}

#[test]
fn a_launch_with_no_command_asks_the_running_app_to_show_the_pr_switcher() {
    let (repo, github) = shop_with_a_tour_file();
    let cli_ended = CliEnded::default();
    let seen = Seen::default();
    let app = fake_app(
        repo.home(),
        &cli_ended,
        None,
        vec![answer_line(BUILD, r#""showing_switcher""#)],
        Arc::clone(&seen),
    );

    let run = GuidedReview::new(&repo, &github)
        .with_env("GUIDED_REVIEW_BUILD", Path::new(BUILD))
        .run(&[]);

    cli_ended.mark();
    app.join().unwrap();
    assert!(run.success, "stderr: {}", run.stderr);
    assert!(run.stdout.contains("PR switcher"), "stdout: {}", run.stdout);
    let request: serde_json::Value = serde_json::from_str(&seen.lock().unwrap()[0]).unwrap();
    assert_eq!(request["request"], "show_switcher");
}

#[test]
fn restart_asks_even_an_app_of_this_build_to_quit_then_opens_the_pr_on_the_step_in_a_new_one() {
    let (repo, github) = shop_with_a_tour_file();
    let cli_ended = CliEnded::default();
    let script = start_script(repo.home());
    let seen = Seen::default();
    let app = old_app_then_new(
        &repo,
        &cli_ended,
        vec![
            answer_line(BUILD, r#""other_build""#),
            answer_line(BUILD, r#""quitting""#),
        ],
        vec![opening_line()],
        &seen,
    );

    let run = GuidedReview::new(&repo, &github)
        .with_env("GUIDED_REVIEW_APP_COMMAND", &script)
        .with_env("GUIDED_REVIEW_BUILD", Path::new(BUILD))
        .run(&["restart", PR, "--at", "readme"]);

    cli_ended.mark();
    app.join().unwrap();
    assert!(run.success, "stderr: {}", run.stderr);
    let requests = requests(&seen);
    assert_eq!(requests.len(), 3);
    assert_ne!(requests[0]["request"], "quit");
    assert_eq!(requests[1]["request"], "quit");
    assert_eq!(requests[2]["request"]["open"]["pr"], PR);
    assert_eq!(requests[2]["request"]["open"]["at_step"], "readme");
}

fn shop_with_a_pull_of_one_changed_file() -> (FixtureRepo, FakeGitHub) {
    let repo = FixtureRepo::cloned_from("acme/shop");
    repo.write("README.md", "hello\n");
    let base = repo.commit("base");
    repo.write("README.md", "hello\nworld\n");
    let head = repo.commit("head");
    let github = FakeGitHub::serving_pull("acme/shop", 7, &base, &head);
    (repo, github)
}

fn basic_tour_file(repo: &FixtureRepo) -> PathBuf {
    guided_review::tour::Tour::basic_path_for(&repo.folders(), &PR.parse().unwrap())
}

fn run_open_with_a_fake_app(repo: &FixtureRepo, github: &FakeGitHub) -> crate::support::Run {
    let cli_ended = CliEnded::default();
    let app = fake_app(
        repo.home(),
        &cli_ended,
        None,
        vec![opening_line()],
        Seen::default(),
    );
    let run = GuidedReview::new(repo, github)
        .with_env("GUIDED_REVIEW_BUILD", Path::new(BUILD))
        .run(&["open", PR]);
    cli_ended.mark();
    app.join().unwrap();
    run
}

#[test]
fn open_of_a_pr_without_a_tour_makes_its_basic_tour_and_says_it_opened_in_basic_mode() {
    let (repo, github) = shop_with_a_pull_of_one_changed_file();

    let run = run_open_with_a_fake_app(&repo, &github);

    assert!(run.success, "stderr: {}", run.stderr);
    assert!(run.stdout.contains("Basic mode"), "stdout: {}", run.stdout);
    let tour: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(basic_tour_file(&repo)).unwrap()).unwrap();
    assert_eq!(tour["kind"], "basic");
    assert_eq!(tour["pr_title"], "Add the cart");
    assert_eq!(tour["pr"], PR);
    assert!(tour["base"].as_str().is_some_and(|sha| !sha.is_empty()));
    assert!(tour["head"].as_str().is_some_and(|sha| !sha.is_empty()));
}

#[test]
fn open_of_a_pr_with_a_guided_tour_makes_no_basic_tour_and_does_not_say_basic_mode() {
    let (repo, github) = shop_with_a_tour_file();

    let run = run_open_with_a_fake_app(&repo, &github);

    assert!(run.success, "stderr: {}", run.stderr);
    assert!(!basic_tour_file(&repo).exists());
    assert!(!run.stdout.contains("Basic mode"), "stdout: {}", run.stdout);
}

#[test]
fn open_of_a_pr_github_does_not_have_fails_in_the_terminal_and_makes_no_tour() {
    let (repo, github) = shop_with_a_pull_of_one_changed_file();
    let other = "https://github.com/acme/shop/pull/8";

    let run = GuidedReview::new(&repo, &github).run(&["open", other]);

    assert!(!run.success);
    assert!(run.stderr.contains("pull/8"), "stderr: {}", run.stderr);
    assert!(!run.stdout.contains("Basic mode"));
}

#[test]
fn restart_at_a_step_of_a_basic_tour_is_refused_because_it_has_no_steps() {
    let (repo, github) = shop_with_a_pull_of_one_changed_file();
    let made = run_open_with_a_fake_app(&repo, &github);
    assert!(made.success, "stderr: {}", made.stderr);

    let run = GuidedReview::new(&repo, &github).run(&["restart", PR, "--at", "readme"]);

    assert!(!run.success);
    assert!(run.stderr.contains("no steps"), "stderr: {}", run.stderr);
}

#[test]
fn restart_at_a_step_of_a_pr_with_no_tour_is_refused_before_a_basic_tour_is_made() {
    let (repo, github) = shop_with_a_pull_of_one_changed_file();

    let run = GuidedReview::new(&repo, &github).run(&["restart", PR, "--at", "readme"]);

    assert!(!run.success);
    assert!(run.stderr.contains("no steps"), "stderr: {}", run.stderr);
    assert!(!basic_tour_file(&repo).exists());
}

#[test]
fn a_basic_tour_does_not_change_the_guided_commands() {
    let (repo, github) = shop_with_a_pull_of_one_changed_file();
    let made = run_open_with_a_fake_app(&repo, &github);
    assert!(made.success, "stderr: {}", made.stderr);

    let path = GuidedReview::new(&repo, &github).run(&["tour-path", PR]);
    let check = GuidedReview::new(&repo, &github).run(&["check", PR]);

    assert!(
        path.stdout.trim_end().ends_with("acme/shop/7.json"),
        "{}",
        path.stdout
    );
    assert!(!check.success);
    assert!(!Path::new(path.stdout.trim_end()).exists());
}
