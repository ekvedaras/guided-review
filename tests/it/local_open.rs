use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use guided_review::app_link::AppFolder;
use guided_review::review_subject::ReviewSubject;
use guided_review::tour::Tour;

use crate::support::fake_app::{CliEnded, accept_before};
use crate::support::{FakeGitHub, FixtureRepo, GuidedReview, Run, folders_in, write_executable};

const BUILD: &str = "test-build";

/// Where GitHub would be: nothing listens there, so any call to it fails the run it is made in.
fn no_github() -> FakeGitHub {
    FakeGitHub {
        url: "http://127.0.0.1:1".to_owned(),
    }
}

/// `main` with a README and a `.gitignore`, then `feature` one commit ahead, with a modified tracked
/// file, an untracked file and an ignored one on disk.
fn shop_with_work_in_progress() -> FixtureRepo {
    let repo = FixtureRepo::cloned_from("acme/shop");
    repo.write("README.md", "hello\n");
    repo.write(".gitignore", "target/\n");
    repo.commit("Start the shop");
    repo.git(&["checkout", "-q", "-b", "feature"]);
    repo.write("src/cart.rs", "pub fn total() {}\n");
    repo.commit("Add the cart");
    repo.write("README.md", "hello\nworld\n");
    repo.write("notes.txt", "draft\n");
    repo.write("target/out.bin", "built\n");
    repo
}

fn key_of(repo: &FixtureRepo, branch: &str) -> String {
    format!(
        "local:{}@{branch}",
        repo.path().canonicalize().unwrap().display()
    )
}

fn subject(key: &str) -> ReviewSubject {
    key.parse().unwrap()
}

fn basic_tour_of(repo: &FixtureRepo, key: &str) -> serde_json::Value {
    let path = Tour::basic_path_for(&repo.folders(), &subject(key));
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn has_basic_tour(repo: &FixtureRepo, key: &str) -> bool {
    Tour::basic_path_for(&repo.folders(), &subject(key)).exists()
}

type Seen = Arc<Mutex<Vec<String>>>;

fn answer_line(answer: &str) -> String {
    format!(r#"{{"version":"{BUILD}","answer":{answer}}}"#)
}

fn opening_line(key: &str) -> String {
    answer_line(&format!(r#"{{"opening":{{"pr":"{key}"}}}}"#))
}

/// A running app that answers its next requests with `answers`, recording them.
fn fake_app(
    home: &Path,
    cli_ended: &CliEnded,
    started: Option<PathBuf>,
    answers: Vec<String>,
    seen: Seen,
) -> std::thread::JoinHandle<()> {
    let socket = AppFolder::of(&folders_in(home)).socket();
    let bind = move || {
        std::fs::create_dir_all(socket.parent().unwrap()).unwrap();
        std::fs::remove_file(&socket).ok();
        UnixListener::bind(&socket).unwrap()
    };
    let early_listener = started.is_none().then(&bind);
    let cli_ended = cli_ended.clone();
    std::thread::spawn(move || {
        while started.as_ref().is_some_and(|file| !file.exists()) && !cli_ended.is_marked() {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let listener = early_listener.unwrap_or_else(&bind);
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

fn requests(seen: &Seen) -> Vec<serde_json::Value> {
    seen.lock()
        .unwrap()
        .iter()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

/// Runs `args` in `dir` with a fake app that takes one open and answers that it is opening `key`.
fn run_with_a_fake_app(repo: &FixtureRepo, dir: &Path, key: &str, args: &[&str]) -> (Run, Seen) {
    let cli_ended = CliEnded::default();
    let seen = Seen::default();
    let app = fake_app(
        repo.home(),
        &cli_ended,
        None,
        vec![opening_line(key)],
        Arc::clone(&seen),
    );
    let run = GuidedReview::new(repo, &no_github())
        .in_dir(dir)
        .with_env("HC_TERMINAL_ID", Path::new("t1b"))
        .with_env("GUIDED_REVIEW_BUILD", Path::new(BUILD))
        .run(args);
    cli_ended.mark();
    app.join().unwrap();
    (run, seen)
}

fn open_dot(repo: &FixtureRepo, key: &str) -> (Run, Seen) {
    run_with_a_fake_app(repo, repo.path(), key, &["open", "."])
}

fn refused_open(repo: &FixtureRepo) -> Run {
    GuidedReview::new(repo, &no_github())
        .with_env("GUIDED_REVIEW_BUILD", Path::new(BUILD))
        .run(&["open", "."])
}

fn snapshot_refs(repo: &FixtureRepo) -> String {
    repo.git(&[
        "for-each-ref",
        "--format=%(refname) %(objectname)",
        "refs/guided-review",
    ])
}

fn reviewers_state(repo: &FixtureRepo) -> Vec<String> {
    vec![
        repo.git(&["status", "--porcelain", "--untracked-files=all"]),
        repo.git(&["stash", "list"]),
        repo.git(&["rev-parse", "HEAD"]),
        repo.git(&["diff", "--cached"]),
        repo.git(&["ls-files", "--stage"]),
        std::fs::read_to_string(repo.path().join("README.md")).unwrap(),
    ]
}

#[test]
fn open_makes_the_basic_tour_of_the_branch_and_hands_the_app_its_key() {
    let repo = shop_with_work_in_progress();
    let key = key_of(&repo, "feature");
    let main_tip = repo.git(&["rev-parse", "main"]);

    let (run, seen) = open_dot(&repo, &key);

    assert!(run.success, "stderr: {}", run.stderr);
    assert!(
        run.stdout.contains(&format!("{key} opened in Basic mode")),
        "stdout: {}",
        run.stdout
    );
    let tour = basic_tour_of(&repo, &key);
    assert_eq!(tour["kind"], "basic");
    assert_eq!(tour["pr_title"], "feature");
    assert_eq!(tour["local"], key.as_str());
    assert_eq!(tour["base"], main_tip.as_str());
    let snapshot = repo.git(&[
        "for-each-ref",
        "--format=%(objectname)",
        "refs/guided-review",
    ]);
    assert_eq!(tour["head"], snapshot.as_str());
    let request = &requests(&seen)[0]["request"]["open"];
    assert_eq!(request["subject"], key.as_str());
    assert_eq!(request["terminal"], "t1b");
    assert_eq!(
        Path::new(request["folder"].as_str().unwrap())
            .canonicalize()
            .unwrap(),
        repo.path().canonicalize().unwrap()
    );
}

#[test]
fn the_outline_of_a_local_review_shows_commits_uncommitted_and_untracked_files_but_not_ignored_ones()
 {
    let repo = shop_with_work_in_progress();
    let key = key_of(&repo, "feature");
    open_dot(&repo, &key);

    let run = GuidedReview::new(&repo, &no_github()).run(&["outline", &key]);

    assert!(run.success, "stderr: {}", run.stderr);
    assert!(
        run.stdout.starts_with(&format!("local: {key}\n")),
        "{}",
        run.stdout
    );
    assert!(!run.stdout.contains("pr: "), "{}", run.stdout);
    for shown in ["src/cart.rs", "README.md", "notes.txt"] {
        assert!(run.stdout.contains(shown), "{shown} in:\n{}", run.stdout);
    }
    assert!(!run.stdout.contains("target/out.bin"), "{}", run.stdout);
    assert!(
        run.stdout.contains("worktree at head: yes"),
        "{}",
        run.stdout
    );
}

#[test]
fn show_prints_the_code_of_an_untracked_file_from_the_snapshot() {
    let repo = shop_with_work_in_progress();
    let key = key_of(&repo, "feature");
    open_dot(&repo, &key);
    repo.write("notes.txt", "edited after the snapshot\n");

    let run = GuidedReview::new(&repo, &no_github()).run(&["show", &key, "notes.txt"]);

    assert!(run.success, "stderr: {}", run.stderr);
    assert!(run.stdout.contains("draft"), "{}", run.stdout);
    assert!(!run.stdout.contains("edited after"), "{}", run.stdout);
}

#[test]
fn outline_says_the_worktree_is_no_longer_at_the_snapshot_once_a_file_changed() {
    let repo = shop_with_work_in_progress();
    let key = key_of(&repo, "feature");
    open_dot(&repo, &key);
    repo.write("notes.txt", "edited after the snapshot\n");

    let run = GuidedReview::new(&repo, &no_github()).run(&["outline", &key]);

    assert!(run.success, "stderr: {}", run.stderr);
    assert!(
        run.stdout.contains("worktree at head: no"),
        "{}",
        run.stdout
    );
}

#[test]
fn tour_path_prints_where_the_local_tour_lives_even_with_no_tour() {
    let repo = shop_with_work_in_progress();
    let key = key_of(&repo, "feature");

    let run = GuidedReview::new(&repo, &no_github()).run(&["tour-path", &key]);

    assert!(run.success, "stderr: {}", run.stderr);
    let expected = Tour::path_for(&repo.folders(), &subject(&key));
    assert_eq!(run.stdout.trim_end(), expected.display().to_string());
    assert!(!expected.exists());
}

#[test]
fn outline_with_no_tour_says_to_open_the_folder_first() {
    let repo = shop_with_work_in_progress();
    let key = key_of(&repo, "feature");

    let run = GuidedReview::new(&repo, &no_github()).run(&["outline", &key]);

    assert!(!run.success);
    assert!(
        run.stderr.contains(&format!(
            "no tour for {key}; run `guided-review open <folder>` first"
        )),
        "stderr: {}",
        run.stderr
    );
}

#[test]
fn open_works_from_a_subfolder_of_the_clone() {
    let repo = shop_with_work_in_progress();
    let key = key_of(&repo, "feature");

    let (run, seen) = run_with_a_fake_app(&repo, &repo.path().join("src"), &key, &["open", "."]);

    assert!(run.success, "stderr: {}", run.stderr);
    assert!(has_basic_tour(&repo, &key));
    assert_eq!(
        requests(&seen)[0]["request"]["open"]["subject"],
        key.as_str()
    );
}

#[test]
fn open_takes_a_path_argument_that_is_not_the_current_folder() {
    let repo = shop_with_work_in_progress();
    let key = key_of(&repo, "feature");
    let elsewhere = tempfile::TempDir::new().unwrap();

    let (run, _) = run_with_a_fake_app(
        &repo,
        elsewhere.path(),
        &key,
        &["open", repo.path().to_str().unwrap()],
    );

    assert!(run.success, "stderr: {}", run.stderr);
    assert!(has_basic_tour(&repo, &key));
}

#[test]
fn open_of_a_detached_head_is_refused_with_the_reason_and_makes_nothing() {
    let repo = shop_with_work_in_progress();
    repo.git(&["checkout", "-q", "--detach"]);

    let run = refused_open(&repo);

    assert!(!run.success);
    assert!(
        run.stderr.contains("detached HEAD"),
        "stderr: {}",
        run.stderr
    );
    assert_eq!(snapshot_refs(&repo), "");
}

#[test]
fn open_of_a_clean_branch_with_nothing_beyond_the_default_branch_is_refused_and_leaves_no_ref() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    repo.write("README.md", "hello\n");
    repo.commit("Start the shop");
    repo.git(&["checkout", "-q", "-b", "feature"]);

    let run = refused_open(&repo);

    assert!(!run.success);
    assert!(
        run.stderr.contains("nothing to review"),
        "stderr: {}",
        run.stderr
    );
    assert_eq!(snapshot_refs(&repo), "");
    assert!(!has_basic_tour(&repo, &key_of(&repo, "feature")));
}

#[test]
fn open_of_a_clone_with_no_default_branch_is_refused_naming_the_branches_looked_for() {
    let repo = shop_with_work_in_progress();
    repo.git(&["branch", "-m", "main", "develop"]);

    let run = refused_open(&repo);

    assert!(!run.success);
    for looked_for in ["refs/remotes/origin/HEAD", "main", "master"] {
        assert!(run.stderr.contains(looked_for), "stderr: {}", run.stderr);
    }
}

#[test]
fn open_of_a_folder_that_is_no_clone_is_refused() {
    let repo = shop_with_work_in_progress();
    let folder = tempfile::TempDir::new().unwrap();

    let run = GuidedReview::new(&repo, &no_github())
        .in_dir(folder.path())
        .run(&["open", "."]);

    assert!(!run.success);
    assert!(
        run.stderr.contains("not inside a git clone"),
        "stderr: {}",
        run.stderr
    );
}

#[test]
fn open_keeps_the_snapshot_by_a_ref_and_leaves_the_reviewers_files_index_and_stash_alone() {
    let repo = shop_with_work_in_progress();
    repo.write("README.md", "stashed\n");
    repo.git(&["stash", "push", "-q", "README.md"]);
    repo.write("README.md", "hello\nworld\n");
    repo.git(&["add", "notes.txt"]);
    let key = key_of(&repo, "feature");
    let before = reviewers_state(&repo);

    let (run, _) = open_dot(&repo, &key);

    assert!(run.success, "stderr: {}", run.stderr);
    assert_eq!(reviewers_state(&repo), before);
    let refs = snapshot_refs(&repo);
    let (name, value) = refs.split_once(' ').unwrap();
    assert!(name.starts_with("refs/guided-review/"), "{refs}");
    assert_eq!(basic_tour_of(&repo, &key)["head"], value);
    assert_ne!(before[1], "");
}

#[test]
fn reopening_opens_the_review_as_it_is_with_no_second_snapshot() {
    let repo = shop_with_work_in_progress();
    let key = key_of(&repo, "feature");
    open_dot(&repo, &key);
    let refs = snapshot_refs(&repo);
    let tour = basic_tour_of(&repo, &key);
    repo.write("notes.txt", "changed since the first open\n");

    let (run, seen) = open_dot(&repo, &key);

    assert!(run.success, "stderr: {}", run.stderr);
    assert_eq!(snapshot_refs(&repo), refs);
    assert_eq!(basic_tour_of(&repo, &key)["head"], tour["head"]);
    assert_eq!(
        requests(&seen)[0]["request"]["open"]["subject"],
        key.as_str()
    );
}

#[test]
fn a_local_key_opens_when_the_folder_is_on_that_branch_and_is_refused_when_it_is_not() {
    let repo = shop_with_work_in_progress();
    let key = key_of(&repo, "feature");
    let other_key = key_of(&repo, "other");

    let refused = GuidedReview::new(&repo, &no_github()).run(&["open", &other_key]);
    let (opened, _) = run_with_a_fake_app(&repo, repo.path(), &key, &["open", &key]);

    assert!(!refused.success);
    assert!(
        refused.stderr.contains("feature") && refused.stderr.contains("other"),
        "stderr: {}",
        refused.stderr
    );
    assert!(!has_basic_tour(&repo, &other_key));
    assert!(opened.success, "stderr: {}", opened.stderr);
    assert!(has_basic_tour(&repo, &key));
}

#[test]
fn two_worktrees_on_different_branches_are_two_reviews() {
    let repo = shop_with_work_in_progress();
    let linked = tempfile::TempDir::new().unwrap();
    let linked_folder = linked.path().join("shop-fixes");
    repo.git(&[
        "worktree",
        "add",
        "-q",
        "-b",
        "fixes",
        linked_folder.to_str().unwrap(),
        "main",
    ]);
    std::fs::write(linked_folder.join("fix.txt"), "fix\n").unwrap();
    let key = key_of(&repo, "feature");
    let linked_key = format!(
        "local:{}@fixes",
        linked_folder.canonicalize().unwrap().display()
    );
    open_dot(&repo, &key);

    let (run, _) = run_with_a_fake_app(&repo, &linked_folder, &linked_key, &["open", "."]);

    assert!(run.success, "stderr: {}", run.stderr);
    assert!(has_basic_tour(&repo, &key) && has_basic_tour(&repo, &linked_key));
    assert_ne!(
        Tour::basic_path_for(&repo.folders(), &subject(&key)),
        Tour::basic_path_for(&repo.folders(), &subject(&linked_key))
    );
    let outline = GuidedReview::new(&repo, &no_github()).run(&["outline", &linked_key]);
    assert!(outline.stdout.contains("fix.txt"), "{}", outline.stdout);
    assert!(!outline.stdout.contains("notes.txt"), "{}", outline.stdout);
}

#[test]
fn restart_of_a_folder_makes_the_basic_tour_and_hands_the_app_the_key() {
    let repo = shop_with_work_in_progress();
    let key = key_of(&repo, "feature");
    let started = repo.home().join("started");
    let script = repo.home().join("fake-app");
    write_executable(&script, "#!/bin/sh\necho \"$@\" > \"$HOME/started\"\n");
    let cli_ended = CliEnded::default();
    let seen = Seen::default();
    let app = fake_app(
        repo.home(),
        &cli_ended,
        Some(started),
        vec![opening_line(&key)],
        Arc::clone(&seen),
    );

    let run = GuidedReview::new(&repo, &no_github())
        .with_env("GUIDED_REVIEW_APP_COMMAND", &script)
        .with_env("GUIDED_REVIEW_BUILD", Path::new(BUILD))
        .run(&["restart", "."]);

    cli_ended.mark();
    app.join().unwrap();
    assert!(run.success, "stderr: {}", run.stderr);
    assert!(has_basic_tour(&repo, &key));
    assert_eq!(
        requests(&seen)[0]["request"]["open"]["subject"],
        key.as_str()
    );
}

#[test]
fn restart_at_a_step_of_a_local_basic_tour_is_refused_because_it_has_no_steps() {
    let repo = shop_with_work_in_progress();
    let key = key_of(&repo, "feature");
    open_dot(&repo, &key);

    let run = GuidedReview::new(&repo, &no_github()).run(&["restart", &key, "--at", "readme"]);

    assert!(!run.success);
    assert!(run.stderr.contains("no steps"), "stderr: {}", run.stderr);
}

#[test]
fn open_of_something_that_is_no_link_key_or_folder_says_what_it_takes() {
    let repo = shop_with_work_in_progress();

    let run = GuidedReview::new(&repo, &no_github()).run(&["open", "acme/shop#7"]);

    assert!(!run.success);
    assert!(
        run.stderr
            .contains("neither a GitHub PR link, a local change set key nor a folder"),
        "stderr: {}",
        run.stderr
    );
}

#[test]
fn a_review_opened_from_a_parsed_key_still_knows_its_repo_from_the_folder() {
    let repo = shop_with_work_in_progress();
    let parsed = subject(&key_of(&repo, "feature"));

    let opened =
        guided_review::open_review::open_review(&parsed, repo.path(), &repo.folders()).unwrap();

    let title = opened.review.window_title();
    assert!(title.contains("acme/shop"), "title: {title}");
    let overview = opened.review.overview_page().local.unwrap();
    assert_eq!(overview.repo.unwrap().to_string(), "acme/shop");
}

#[test]
fn open_of_a_key_that_is_malformed_says_what_is_wrong_with_it() {
    let repo = shop_with_work_in_progress();

    let run = GuidedReview::new(&repo, &no_github()).run(&["open", "local:/somewhere"]);

    assert!(!run.success);
    assert!(
        run.stderr.contains("names no branch"),
        "stderr: {}",
        run.stderr
    );
}
