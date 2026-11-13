use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use guided_review::claude_cli::ClaudeCli;
use guided_review::file_path::FilePath;
use guided_review::pieces::PieceId;
use guided_review::pull_request::Sha;
use guided_review::repin::RepinStatus;
use guided_review::viewer::{AgentLaunch, AgentLauncher, Asks, Place, ReviewStateFile};
use serde_json::{Value, json};
use tempfile::TempDir;

const SESSION: &str = "7f2cd173-4eda-4770-aeb2-c82977b1b673";

/// A stand-in `claude` that logs how it was started and what it was sent,
/// and answers every line of input with one text delta and a result.
fn fake_claude(dir: &Path) -> PathBuf {
    let script = dir.join("claude");
    std::fs::write(
        &script,
        format!(
            r#"#!/bin/sh
echo "$@" > "{dir}/args"
pwd > "{dir}/cwd"
while IFS= read -r line; do
  echo "$line" >> "{dir}/received"
  echo '{{"type":"stream_event","event":{{"delta":{{"type":"text_delta","text":"hi"}}}}}}'
  echo '{{"type":"result","is_error":false,"result":"hi","session_id":"{SESSION}"}}'
done
"#,
            dir = dir.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    script
}

fn launch(cwd: &Path, resume: Option<&str>) -> AgentLaunch {
    AgentLaunch {
        cwd: cwd.to_path_buf(),
        model: "opus".to_owned(),
        effort: "medium".to_owned(),
        system_prompt: "be read-only".to_owned(),
        resume: resume.map(str::to_owned),
    }
}

#[test]
fn claude_is_started_read_only_in_the_clone_and_answers_follow_ups_in_the_same_process() {
    let dir = TempDir::new().unwrap();
    let clone = TempDir::new().unwrap();
    let launcher = ClaudeCli::with_binary(fake_claude(dir.path()));

    let mut agent = launcher.start(&launch(clone.path(), None)).unwrap();
    let mut lines = Vec::new();
    agent
        .ask("first", &mut |line| lines.push(line.to_owned()))
        .unwrap();
    agent
        .ask("second", &mut |line| lines.push(line.to_owned()))
        .unwrap();

    assert_eq!(lines.len(), 4);
    let args = std::fs::read_to_string(dir.path().join("args")).unwrap();
    for flag in [
        "-p",
        "--input-format stream-json",
        "--output-format stream-json",
        "--verbose",
        "--include-partial-messages",
        "--permission-mode dontAsk",
        "--allowedTools Read,Grep,Glob,Bash(guided-review show:*)",
        "--model opus",
        "--effort medium",
        "--append-system-prompt be read-only",
    ] {
        assert!(args.contains(flag), "{flag} missing in: {args}");
    }
    assert!(!args.contains("--bare") && !args.contains("--resume"));
    let cwd = std::fs::read_to_string(dir.path().join("cwd")).unwrap();
    assert_eq!(
        Path::new(cwd.trim()).canonicalize().unwrap(),
        clone.path().canonicalize().unwrap()
    );
    let received: Vec<Value> = std::fs::read_to_string(dir.path().join("received"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(
        received[0],
        json!({"type":"user","message":{"role":"user","content":"first"}})
    );
    assert_eq!(received[1]["message"]["content"], "second");
}

#[test]
fn a_saved_session_is_resumed() {
    let dir = TempDir::new().unwrap();
    let launcher = ClaudeCli::with_binary(fake_claude(dir.path()));

    let mut agent = launcher.start(&launch(dir.path(), Some(SESSION))).unwrap();
    agent.ask("again", &mut |_| {}).unwrap();

    let args = std::fs::read_to_string(dir.path().join("args")).unwrap();
    assert!(args.contains(&format!("--resume {SESSION}")), "{args}");
}

#[test]
fn a_missing_claude_binary_says_so() {
    let launcher = ClaudeCli::with_binary(PathBuf::from("/nonexistent/claude"));

    let error = launcher
        .start(&launch(Path::new("/tmp"), None))
        .err()
        .unwrap();

    assert!(
        format!("{error:#}").contains("could not start `/nonexistent/claude`"),
        "{error:#}"
    );
}

#[test]
fn asks_go_into_the_per_pr_state_file_next_to_whatever_else_it_holds() {
    let home = TempDir::new().unwrap();
    let pr = "https://github.com/acme/shop/pull/7".parse().unwrap();
    let file = home.path().join(".guided-review/state/acme/shop/7.json");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, r#"{"read_ticks":{"order":{"head":"abc"}}}"#).unwrap();
    let store = Arc::new(ReviewStateFile::for_pr(home.path(), &pr));

    let mut asks = Asks::load(Arc::<ReviewStateFile>::clone(&store)).unwrap();
    let id = asks.start(Place::Tour, &Sha::new("head1"));
    asks.begin_turn(id, "What is this?", "context");

    let state: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(state["read_ticks"], json!({"order":{"head":"abc"}}));
    assert_eq!(state["asks"][0]["messages"][0]["text"], "What is this?");
    let reloaded = Asks::load(store).unwrap();
    assert_eq!(reloaded.count_at(&Place::Tour), 1);
}

#[test]
fn without_a_state_file_there_are_no_asks() {
    let home = TempDir::new().unwrap();
    let pr = "https://github.com/acme/shop/pull/7".parse().unwrap();

    let asks = Asks::load(Arc::new(ReviewStateFile::for_pr(home.path(), &pr))).unwrap();

    assert_eq!(asks.count_at(&Place::Tour), 0);
}

#[test]
fn saving_asks_keeps_the_read_ticks_and_the_other_way_round() {
    let home = TempDir::new().unwrap();
    let pr = "https://github.com/acme/shop/pull/7".parse().unwrap();
    let file = ReviewStateFile::for_pr(home.path(), &pr);
    let mut asks = Asks::load(Arc::new(ReviewStateFile::for_pr(home.path(), &pr))).unwrap();

    file.update(|state| state.tick(&"order".into(), &Sha::new("abc")))
        .unwrap();
    let id = asks.start(Place::Tour, &Sha::new("head1"));
    asks.begin_turn(id, "What is this?", "context");
    file.update(|state| state.untick(&"other".into())).unwrap();

    let text =
        std::fs::read_to_string(home.path().join(".guided-review/state/acme/shop/7.json")).unwrap();
    let state: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(state["read_ticks"]["order"]["head"], "abc");
    assert_eq!(state["asks"][0]["messages"][0]["text"], "What is this?");
}

#[test]
fn stopping_a_turn_asks_claude_to_interrupt_over_its_input_and_keeps_the_process() {
    let dir = TempDir::new().unwrap();
    let clone = TempDir::new().unwrap();
    let launcher = ClaudeCli::with_binary(fake_claude(dir.path()));
    let mut agent = launcher.start(&launch(clone.path(), None)).unwrap();

    agent.turn_stopper().stop_turn().unwrap();
    agent
        .ask("still there?", &mut |_| {})
        .expect("the process outlives the stopped turn");

    let lines = std::fs::read_to_string(dir.path().join("received")).unwrap();
    let sent: Value = serde_json::from_str(lines.lines().next().unwrap()).unwrap();
    assert_eq!(sent["type"], "control_request");
    assert_eq!(sent["request"]["subtype"], "interrupt");
}

#[test]
fn a_state_file_written_with_text_piece_ids_loads_and_saves_back_unchanged() {
    let home = TempDir::new().unwrap();
    let pr = "https://github.com/acme/shop/pull/7".parse().unwrap();
    let file = ReviewStateFile::for_pr(home.path(), &pr);
    let written = json!({
        "asks": [
            {
                "id": 1,
                "place": {"kind": "piece", "step": "order", "piece": "app/Order.php#Order::total"},
                "head": "abc",
                "messages": [{"author": "reviewer", "text": "Why?"}]
            },
            {
                "id": 2,
                "place": {
                    "kind": "lines",
                    "step": "order",
                    "start": {"path": "app/Order.php", "side": "Head", "line": 3},
                    "end": {"path": "app/Order.php", "side": "Head", "line": 4},
                    "pieces": ["app/Order.php#Order::total", "config.yml#hunk-1"]
                },
                "head": "abc"
            }
        ],
        "last_repin": {
            "from_base": "b1",
            "from_head": "h1",
            "to_base": "b2",
            "to_head": "h2",
            "pieces": {
                "app/Order.php#Order::total": "changed",
                "config.yml#hunk-1": "new"
            },
            "gone_by_step": {"order": ["app/Old.php#Old::run"]}
        }
    });
    let path = home.path().join(".guided-review/state/acme/shop/7.json");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, written.to_string()).unwrap();

    let state = file.load().unwrap();
    let asks = Asks::load(Arc::new(ReviewStateFile::for_pr(home.path(), &pr))).unwrap();
    let total = "app/Order.php#Order::total".parse().unwrap();
    assert_eq!(
        asks.all()[0].place,
        Place::Piece {
            step: "order".into(),
            piece: total
        }
    );
    assert_eq!(
        state
            .last_repin()
            .unwrap()
            .status_of(&"config.yml#hunk-1".parse().unwrap()),
        Some(RepinStatus::New)
    );

    file.save(&state).unwrap();

    let saved: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(saved["last_repin"], written["last_repin"]);
    assert_eq!(saved["asks"][0]["place"], written["asks"][0]["place"]);
    assert_eq!(saved["asks"][1]["place"], written["asks"][1]["place"]);
    let reloaded = Asks::load(Arc::new(ReviewStateFile::for_pr(home.path(), &pr))).unwrap();
    assert_eq!(reloaded.all(), asks.all());
}

#[test]
fn a_state_file_naming_files_with_a_hash_loads_and_finds_their_pieces() {
    let home = TempDir::new().unwrap();
    let pr = "https://github.com/acme/shop/pull/7".parse().unwrap();
    let file = ReviewStateFile::for_pr(home.path(), &pr);
    let written = json!({
        "asks": [{
            "id": 1,
            "place": {"kind": "piece", "step": "docs", "piece": "docs/C#.md"},
            "head": "abc"
        }],
        "last_repin": {
            "from_base": "b1",
            "from_head": "h1",
            "to_base": "b2",
            "to_head": "h2",
            "pieces": {
                "docs/C#.md": "changed",
                "icons/a#1@2x.png": "new",
                "#notes.md": "changed",
                "/not/a/repo/path": "new"
            },
            "gone_by_step": {"docs": ["#old.md", ""]}
        }
    });
    let path = home.path().join(".guided-review/state/acme/shop/7.json");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, written.to_string()).unwrap();

    let state = file.load().unwrap();
    let record = state.last_repin().unwrap();
    let whole = |name: &str| PieceId::whole_file(FilePath::new(name).unwrap());
    assert_eq!(
        record.status_of(&whole("docs/C#.md")),
        Some(RepinStatus::Changed)
    );
    assert_eq!(
        record.status_of(&whole("icons/a#1@2x.png")),
        Some(RepinStatus::New)
    );
    assert_eq!(
        record.status_of(&whole("#notes.md")),
        Some(RepinStatus::Changed)
    );
    assert_eq!(record.gone_count(&"docs".into()), 1);

    let asks = Asks::load(Arc::new(ReviewStateFile::for_pr(home.path(), &pr))).unwrap();
    let Place::Piece { piece, .. } = &asks.all()[0].place else {
        panic!("not a piece place");
    };
    assert!(whole("docs/C#.md").may_be(piece));
}

#[test]
fn a_saved_review_keeps_whole_files_with_a_hash_apart_from_symbols() {
    let home = TempDir::new().unwrap();
    let pr = "https://github.com/acme/shop/pull/7".parse().unwrap();
    let file = ReviewStateFile::for_pr(home.path(), &pr);
    let path = home.path().join(".guided-review/state/acme/shop/7.json");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(
        &path,
        json!({"last_repin": {
            "from_base": "b1", "from_head": "h1", "to_base": "b2", "to_head": "h2",
            "pieces": {"docs/C#.md": "changed"},
            "gone_by_step": {}
        }})
        .to_string(),
    )
    .unwrap();

    file.save(&file.load().unwrap()).unwrap();
    let state = file.load().unwrap();

    assert_eq!(
        state
            .last_repin()
            .unwrap()
            .status_of(&PieceId::whole_file(FilePath::new("docs/C#.md").unwrap())),
        Some(RepinStatus::Changed)
    );
}

#[test]
fn re_pin_keys_are_saved_in_the_order_of_their_text() {
    let home = TempDir::new().unwrap();
    let pr = "https://github.com/acme/shop/pull/7".parse().unwrap();
    let file = ReviewStateFile::for_pr(home.path(), &pr);
    let path = home.path().join(".guided-review/state/acme/shop/7.json");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(
        &path,
        r#"{"last_repin":{"from_base":"b1","from_head":"h1","to_base":"b2","to_head":"h2","pieces":{"docs/a b.md":"new","docs/a#hunk-1":"changed"},"gone_by_step":{}}}"#,
    )
    .unwrap();

    file.save(&file.load().unwrap()).unwrap();

    let saved = std::fs::read_to_string(&path).unwrap();
    let space = saved.find("docs/a b.md").unwrap();
    let hunk = saved.find("docs/a#hunk-1").unwrap();
    assert!(space < hunk, "{saved}");
}
