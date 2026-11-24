use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use guided_review::language::Language;
use guided_review::local_git::LocalGit;
use guided_review::navigation::{
    Answer, AnsweredBy, CodeNavigator, Column, DefinitionKind, FallbackReason, FileSearch,
    LanguageServers, Lookups, NavigationRequest, Place, ServerCatalog, ServerLaunch, ServerName,
    Timeouts, Waiting, Word,
};
use guided_review::pull_request::{RepoName, Sha};
use serde_json::{Value, json};
use tempfile::TempDir;

use crate::support::{FixtureRepo, file_path, line};

const LIB: &str = "/// Adds two numbers.
///
/// @param a first
pub fn add(a: u32, b: u32) -> u32 { a + b }

pub fn total() -> u32 { add(1, 2) }
";

const HOVER: &str = "```rust
acme_shop
```

```rust
pub fn add(a: u32, b: u32) -> u32
```

---

Adds two numbers.

@param a first";

struct FakeCatalog {
    script: PathBuf,
}

impl ServerCatalog for FakeCatalog {
    fn launch_for(&self, _language: Language) -> Result<ServerLaunch, FallbackReason> {
        let fake = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake_lsp.py");
        Ok(
            ServerLaunch::rust_analyzer(PathBuf::from("python3")).with_args(vec![
                fake.to_string_lossy().into_owned(),
                self.script.to_string_lossy().into_owned(),
            ]),
        )
    }
}

struct Rig {
    repo: FixtureRepo,
    head: Sha,
    scratch: TempDir,
    servers: LanguageServers,
}

fn location(repo: &FixtureRepo, path: &str, line: u32, from: u32, to: u32) -> Value {
    json!({
        "uri": format!("file://{}", repo.path().join(path).display()),
        "range": {"start": {"line": line, "character": from}, "end": {"line": line, "character": to}}
    })
}

impl Rig {
    fn new(text: &str, script: impl FnOnce(&FixtureRepo) -> Value) -> Self {
        Self::timed(text, script, Timeouts::default())
    }

    fn timed(text: &str, script: impl FnOnce(&FixtureRepo) -> Value, timeouts: Timeouts) -> Self {
        let repo = FixtureRepo::cloned_from("acme/shop");
        repo.write("src/lib.rs", text);
        let head = Sha::new(&repo.commit("head"));
        let scratch = TempDir::new().unwrap();
        let mut script = script(&repo);
        script["log"] = json!(scratch.path().join("log").to_string_lossy());
        let script_file = scratch.path().join("script.json");
        std::fs::write(&script_file, script.to_string()).unwrap();
        let servers = LanguageServers::new(
            Arc::new(FakeCatalog {
                script: script_file,
            }),
            timeouts,
        );
        Self {
            repo,
            head,
            scratch,
            servers,
        }
    }

    fn answering_add() -> Self {
        Self::new(LIB, |repo| {
            json!({"answers": {
                "textDocument/definition": [location(repo, "src/lib.rs", 3, 7, 10)],
                "textDocument/references": [location(repo, "src/lib.rs", 5, 23, 26)],
                "textDocument/hover": {"contents": {"kind": "markdown", "value": HOVER}},
            }})
        })
    }

    fn by_name(&self) -> Arc<dyn CodeNavigator> {
        Arc::new(Lookups::new(FileSearch::changed_files(Arc::new([
            file_path("src/lib.rs"),
        ]))))
    }

    fn navigate_at(
        &self,
        number: usize,
        column: usize,
        word: &str,
    ) -> (anyhow::Result<Answer>, Vec<Waiting>) {
        let code =
            LocalGit::configured_clone(&RepoName::new("acme", "shop"), self.repo.path()).unwrap();
        let waits = Mutex::new(Vec::new());
        let answer = self.servers.navigator(self.by_name()).navigate(
            &code,
            &NavigationRequest {
                sha: self.head.clone(),
                at: Place {
                    path: file_path("src/lib.rs"),
                    line: line(number),
                },
                column: Column::try_from(column).unwrap(),
                word: Word::new(word).unwrap(),
            },
            &|waiting| waits.lock().unwrap().push(waiting),
        );
        (answer, waits.into_inner().unwrap())
    }

    fn hover_add(&self) -> (anyhow::Result<Answer>, Vec<Waiting>) {
        self.navigate_at(6, 23, "add")
    }

    fn sent(&self) -> Vec<Value> {
        std::fs::read_to_string(self.scratch.path().join("log"))
            .unwrap_or_default()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    fn methods(&self) -> Vec<String> {
        self.sent()
            .iter()
            .filter_map(|m| m["method"].as_str().map(str::to_owned))
            .collect()
    }

    fn params_of(&self, method: &str) -> Vec<Value> {
        self.sent()
            .into_iter()
            .filter(|m| m["method"] == method)
            .map(|m| m["params"].clone())
            .collect()
    }
}

fn rust_analyzer() -> ServerName {
    ServerName::new("rust-analyzer")
}

#[test]
fn a_name_is_answered_with_the_definition_its_usages_signature_and_doc_summary() {
    let rig = Rig::answering_add();

    let (answer, _) = rig.hover_add();
    let answer = answer.unwrap();

    assert_eq!(
        answer.answered_by,
        AnsweredBy::LanguageServer(rust_analyzer())
    );
    let [definition] = answer.lookup.definitions.as_slice() else {
        panic!("one definition")
    };
    assert_eq!(definition.place.path, file_path("src/lib.rs"));
    assert_eq!(definition.place.line, line(4));
    assert_eq!(definition.kind, DefinitionKind::Function);
    assert_eq!(&*definition.signature, "pub fn add(a: u32, b: u32) -> u32");
    assert_eq!(definition.doc.as_deref(), Some("Adds two numbers."));
    let [usage] = &*answer.lookup.usages else {
        panic!("one usage")
    };
    assert_eq!(usage.place.line, line(6));
    assert_eq!(&*usage.text, "pub fn total() -> u32 { add(1, 2) }");
}

#[test]
fn references_are_asked_without_the_declaration() {
    let rig = Rig::answering_add();

    rig.hover_add().0.unwrap();

    let [references] = rig.params_of("textDocument/references").try_into().unwrap();
    assert_eq!(references["context"]["includeDeclaration"], json!(false));
    assert_eq!(references["position"], json!({"line": 5, "character": 23}));
}

#[test]
fn rust_analyzer_is_started_with_build_scripts_proc_macros_and_check_on_save_off() {
    let rig = Rig::answering_add();

    rig.hover_add().0.unwrap();

    let [initialize] = rig.params_of("initialize").try_into().unwrap();
    let options = &initialize["initializationOptions"];
    assert_eq!(options["cargo"]["buildScripts"]["enable"], json!(false));
    assert_eq!(options["procMacro"]["enable"], json!(false));
    assert_eq!(options["checkOnSave"], json!(false));
}

#[test]
fn a_utf16_server_gets_the_column_counted_in_utf16_units() {
    let text = "fn main() { let s = \"😀\"; foo(s); }\n";
    let rig = Rig::new(
        text,
        |_| json!({"answers": {"textDocument/definition": []}}),
    );
    let byte_column = text.find("foo").unwrap();

    rig.navigate_at(1, byte_column, "foo").0.unwrap();

    let [definition] = rig.params_of("textDocument/definition").try_into().unwrap();
    assert_eq!(
        definition["position"],
        json!({"line": 0, "character": byte_column - 2})
    );
}

#[test]
fn a_utf8_server_gets_the_byte_column() {
    let text = "fn main() { let s = \"😀\"; foo(s); }\n";
    let rig = Rig::new(text, |_| json!({"encoding": "utf-8", "answers": {}}));
    let byte_column = text.find("foo").unwrap();

    rig.navigate_at(1, byte_column, "foo").0.unwrap();

    let [initialize] = rig.params_of("initialize").try_into().unwrap();
    assert!(
        initialize["capabilities"]["general"]["positionEncodings"]
            .to_string()
            .contains("utf-8")
    );
    let [definition] = rig.params_of("textDocument/definition").try_into().unwrap();
    assert_eq!(
        definition["position"],
        json!({"line": 0, "character": byte_column})
    );
}

#[test]
fn the_first_hover_reports_starting_and_then_indexing_while_the_server_works() {
    let rig = Rig::new(LIB, |_| json!({"indexing_ms": 300, "answers": {}}));

    let (answer, waits) = rig.hover_add();

    answer.unwrap();
    assert_eq!(
        waits,
        [Waiting::Starting(rust_analyzer()), Waiting::Indexing]
    );
}

#[test]
fn a_crash_falls_back_to_the_by_name_answer_with_a_reason_and_a_later_hover_starts_it_again() {
    let rig = Rig::new(
        LIB,
        |_| json!({"crash_on": ["textDocument/definition"], "answers": {}}),
    );

    let answer = rig.hover_add().0.unwrap();

    let AnsweredBy::ByName {
        because: Some(because),
    } = &answer.answered_by
    else {
        panic!("by name with a reason, got {:?}", answer.answered_by)
    };
    assert!(
        because.as_str().contains("rust-analyzer"),
        "{}",
        because.as_str()
    );
    rig.hover_add().0.unwrap();
    assert_eq!(
        rig.methods().iter().filter(|m| *m == "initialize").count(),
        2
    );
}

#[test]
fn a_server_that_does_not_answer_in_time_falls_back_with_a_reason() {
    let timeouts = Timeouts {
        answer: Duration::from_millis(300),
        ..Timeouts::default()
    };
    let rig = Rig::timed(
        LIB,
        |_| json!({"silent_on": ["textDocument/definition"], "answers": {}}),
        timeouts,
    );

    let answer = rig.hover_add().0.unwrap();

    let AnsweredBy::ByName {
        because: Some(because),
    } = &answer.answered_by
    else {
        panic!("by name with a reason, got {:?}", answer.answered_by)
    };
    assert!(because.as_str().contains("in time"), "{}", because.as_str());
}

#[test]
fn one_server_answers_every_hover_of_the_repo() {
    let rig = Rig::answering_add();

    rig.hover_add().0.unwrap();
    rig.hover_add().0.unwrap();

    assert_eq!(
        rig.methods().iter().filter(|m| *m == "initialize").count(),
        1
    );
    assert_eq!(
        rig.methods()
            .iter()
            .filter(|m| *m == "textDocument/didOpen")
            .count(),
        1
    );
}

#[test]
fn an_idle_server_is_stopped() {
    let timeouts = Timeouts {
        idle: Duration::from_millis(200),
        ..Timeouts::default()
    };
    let rig = Rig::timed(LIB, |_| json!({"answers": {}}), timeouts);
    rig.hover_add().0.unwrap();

    std::thread::sleep(Duration::from_millis(1500));

    assert!(
        rig.methods().contains(&"exit".to_owned()),
        "{:?}",
        rig.methods()
    );
}

#[test]
fn quitting_shuts_every_server_down() {
    let rig = Rig::answering_add();
    rig.hover_add().0.unwrap();

    rig.servers.stop_all();

    let methods = rig.methods();
    assert_eq!(&methods[methods.len() - 2..], ["shutdown", "exit"]);
}

#[test]
fn a_dirty_clone_is_answered_by_name_with_a_reason() {
    let rig = Rig::answering_add();
    rig.repo.write("src/lib.rs", &format!("{LIB}// edited\n"));

    let answer = rig.hover_add().0.unwrap();

    let AnsweredBy::ByName {
        because: Some(because),
    } = &answer.answered_by
    else {
        panic!("by name with a reason, got {:?}", answer.answered_by)
    };
    assert!(because.as_str().contains("clean"), "{}", because.as_str());
    assert_eq!(rig.methods(), Vec::<String>::new());
}

#[test]
fn a_location_outside_the_repo_is_kept_to_open_from_disk() {
    let rig = Rig::new(LIB, |repo| {
        json!({"answers": {"textDocument/definition": [{
            "uri": "file:///opt/rust/library/core/src/option.rs",
            "range": {"start": {"line": 9, "character": 0}, "end": {"line": 9, "character": 4}}
        }, location(repo, "src/lib.rs", 3, 7, 10)]}})
    });

    let answer = rig.hover_add().0.unwrap();

    let on_disk = answer
        .lookup
        .on_disk(&answer.lookup.definitions[0].place.path);
    assert_eq!(
        on_disk,
        Some(Path::new("/opt/rust/library/core/src/option.rs"))
    );
    assert_eq!(answer.lookup.on_disk(&file_path("src/lib.rs")), None);
}

struct Real;

impl ServerCatalog for Real {
    fn launch_for(&self, _: Language) -> Result<ServerLaunch, FallbackReason> {
        Ok(ServerLaunch::rust_analyzer(PathBuf::from("rust-analyzer")))
    }
}

#[test]
fn the_real_rust_analyzer_finds_a_definition() {
    let Ok(found) = std::process::Command::new("rust-analyzer")
        .arg("--version")
        .output()
    else {
        eprintln!("rust-analyzer is not installed: skipped");
        return;
    };
    if !found.status.success() {
        return;
    }
    let repo = FixtureRepo::cloned_from("acme/shop");
    repo.write(
        "Cargo.toml",
        "[package]\nname = \"tiny\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    );
    repo.write("src/lib.rs", LIB);
    let head = Sha::new(&repo.commit("head"));
    let code = LocalGit::configured_clone(&RepoName::new("acme", "shop"), repo.path()).unwrap();
    let servers = LanguageServers::new(Arc::new(Real), Timeouts::default());
    let by_name: Arc<dyn CodeNavigator> =
        Arc::new(Lookups::new(FileSearch::changed_files(Arc::new([
            file_path("src/lib.rs"),
        ]))));

    let answer = servers
        .navigator(by_name)
        .navigate(
            &code,
            &NavigationRequest {
                sha: head,
                at: Place {
                    path: file_path("src/lib.rs"),
                    line: line(6),
                },
                column: Column::try_from(23).unwrap(),
                word: Word::new("add").unwrap(),
            },
            &|_| {},
        )
        .unwrap();
    servers.stop_all();

    assert_eq!(
        answer.answered_by,
        AnsweredBy::LanguageServer(rust_analyzer())
    );
    assert_eq!(answer.lookup.definitions[0].place.line, line(4));
}
