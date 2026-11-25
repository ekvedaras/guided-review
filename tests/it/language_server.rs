use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use guided_review::app_folders::AppFolders;
use guided_review::codebase::Codebase;
use guided_review::github::GitHub;
use guided_review::language::Language;
use guided_review::local_git::LocalGit;
use guided_review::local_work_tree::LocalWorkTree;
use guided_review::navigation::{
    Answer, AnsweredBy, CodeNavigator, Column, DefinitionKind, FallbackReason, FileSearch,
    LanguageServers, Lookups, ManagedServers, MessageKind, NavigationRequest, Place, ServerAnswer,
    ServerCatalog, ServerCheckouts, ServerFolders, ServerLaunch, ServerMessage, ServerMessages,
    ServerName, ServerPins, Timeouts, Waiting, Word,
};
use guided_review::pull_request::{RepoName, Sha};
use guided_review::tour::GuidedTour;
use guided_review::update::Target;
use guided_review::viewer::{
    DefinitionCard, HoveredName, LineLocation, Review, ReviewStateFile, Side,
};
use serde_json::{Value, json};
use tempfile::TempDir;

use crate::support::fake_ports::{IgnoredServerMessages, RecordedServerMessages};
use crate::support::{FixtureRepo, file_path, line, require_python3, write_executable};

fn shop_codebase() -> Codebase {
    Codebase::GitHub(RepoName::new("acme", "shop"))
}

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
    servers: FakeServers,
}

/// Whether every language gets the one fake server, or each its own, named and logged by language.
#[derive(Clone, Copy, PartialEq, Eq)]
enum FakeServers {
    One,
    PerLanguage,
}

impl ServerCatalog for FakeCatalog {
    fn launch_for(
        &self,
        language: Language,
        _codebase: &Codebase,
        _waiting: &dyn Fn(Waiting),
    ) -> Result<ServerLaunch, FallbackReason> {
        require_python3();
        let fake = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake_lsp.py");
        let script: Value =
            serde_json::from_str(&std::fs::read_to_string(&self.script).unwrap()).unwrap();
        let mut args = vec![
            fake.to_string_lossy().into_owned(),
            self.script.to_string_lossy().into_owned(),
        ];
        let mut launch = ServerLaunch::rust_analyzer(PathBuf::from("python3"));
        if self.servers == FakeServers::PerLanguage {
            launch.name = ServerName::of(language);
            args.push(format!("{language:?}"));
        }
        let launch = launch.with_args(args);
        Ok(if script["reports_status"] == json!(true) {
            launch.reporting_status()
        } else {
            launch
        })
    }
}

struct Rig {
    /// The file every hover is in.
    file: &'static str,
    repo: FixtureRepo,
    head: Sha,
    scratch: TempDir,
    /// Where the app's checkout of the repo will be: what a server's locations are under.
    checkout: PathBuf,
    servers: LanguageServers,
    messages: Arc<RecordedServerMessages>,
}

fn location(checkout: &Path, path: &str, line: u32, from: u32, to: u32) -> Value {
    json!({
        "uri": format!("file://{}", checkout.join(path).display()),
        "range": {"start": {"line": line, "character": from}, "end": {"line": line, "character": to}}
    })
}

impl Rig {
    fn new(text: &str, script: impl FnOnce(&Path) -> Value) -> Self {
        Self::timed(text, script, Timeouts::default())
    }

    fn timed(text: &str, script: impl FnOnce(&Path) -> Value, timeouts: Timeouts) -> Self {
        Self::with_file("src/lib.rs", text, script, timeouts)
    }

    fn with_file(
        file: &'static str,
        text: &str,
        script: impl FnOnce(&Path) -> Value,
        timeouts: Timeouts,
    ) -> Self {
        let repo = FixtureRepo::cloned_from("acme/shop");
        repo.write(file, text);
        let head = Sha::new(&repo.commit("head"));
        let scratch = TempDir::new().unwrap();
        let checkout = scratch.path().join("checkouts/acme/shop");
        let mut script = script(&checkout);
        script["log"] = json!(scratch.path().join("log").to_string_lossy());
        let script_file = scratch.path().join("script.json");
        std::fs::write(&script_file, script.to_string()).unwrap();
        let messages = Arc::new(RecordedServerMessages::default());
        let servers = LanguageServers::new(
            Arc::new(FakeCatalog {
                script: script_file,
                servers: FakeServers::One,
            }),
            ServerCheckouts::new(scratch.path().join("checkouts")),
            timeouts,
            Arc::clone(&messages) as Arc<dyn ServerMessages>,
        );
        Self {
            file,
            repo,
            head,
            scratch,
            checkout,
            servers,
            messages,
        }
    }

    fn answering_add() -> Self {
        Self::answering_add_with(json!({}))
    }

    /// The script of `answering_add` with the keys of `more` added.
    fn answering_add_with(more: Value) -> Self {
        Self::new(LIB, |repo| {
            let mut script = json!({"answers": {
                "textDocument/definition": [location(repo, "src/lib.rs", 3, 7, 10)],
                "textDocument/references": [location(repo, "src/lib.rs", 5, 23, 26)],
                "textDocument/hover": {"contents": {"kind": "markdown", "value": HOVER}},
            }});
            script
                .as_object_mut()
                .unwrap()
                .extend(more.as_object().unwrap().clone());
            script
        })
    }

    fn by_name(&self) -> Arc<dyn CodeNavigator> {
        Arc::new(Lookups::new(FileSearch::changed_files(Arc::new([
            file_path(self.file),
        ]))))
    }

    fn navigate_at(
        &self,
        number: usize,
        column: usize,
        word: &str,
    ) -> (anyhow::Result<Answer>, Vec<Waiting>) {
        let code = LocalGit::configured_clone(&crate::support::shop_on_github(), self.repo.path())
            .unwrap();
        let waits = Mutex::new(Vec::new());
        let answer = self
            .servers
            .navigator(shop_codebase(), self.by_name())
            .navigate(
                &code,
                &NavigationRequest {
                    sha: self.head.clone(),
                    at: Place {
                        path: file_path(self.file),
                        line: line(number),
                    },
                    column: Column::try_from(column).unwrap(),
                    word: Word::new(word).unwrap(),
                },
                &|waiting| waits.lock().unwrap().push(waiting),
            );
        (answer, waits.into_inner().unwrap())
    }

    /// A later review of the repo: the clone gets another commit, and the hover is at that head.
    fn hover_add_at_a_new_head(&self, text: &str) -> (anyhow::Result<Answer>, Vec<Waiting>) {
        self.repo.write("src/lib.rs", text);
        let head = Sha::new(&self.repo.commit("later head"));
        let code = LocalGit::configured_clone(&crate::support::shop_on_github(), self.repo.path())
            .unwrap();
        let answer = self
            .servers
            .navigator(shop_codebase(), self.by_name())
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
            );
        (answer, Vec::new())
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
fn a_message_the_server_asks_to_show_reaches_the_port_with_the_server_s_name() {
    let rig = Rig::new(LIB, |_| {
        json!({"show_on_initialized": [
            {"method": "window/showMessage", "params": {"type": 3, "message": "Licence notice"}},
            {"method": "window/showMessage", "params": {"type": 1, "message": "Index broke"}},
        ]})
    });

    rig.hover_add().0.unwrap();

    assert_eq!(
        rig.messages.shown(),
        vec![
            ServerMessage {
                server: rust_analyzer(),
                kind: MessageKind::Notice,
                text: "Licence notice".to_owned(),
            },
            ServerMessage {
                server: rust_analyzer(),
                kind: MessageKind::Error,
                text: "Index broke".to_owned(),
            },
        ]
    );
}

#[test]
fn a_message_request_reaches_the_port_gets_no_action_as_its_answer_and_the_server_keeps_answering()
{
    let rig = Rig::answering_add_with(json!({"show_on_initialized": [
        {"id": "ask", "method": "window/showMessageRequest", "params": {
            "type": 2,
            "message": "Buy a licence?",
            "actions": [{"title": "Buy"}],
        }},
    ]}));

    let (answer, _) = rig.hover_add();

    assert_eq!(
        answer.unwrap().answered_by,
        AnsweredBy::LanguageServer(rust_analyzer())
    );
    assert_eq!(
        rig.messages.shown(),
        vec![ServerMessage {
            server: rust_analyzer(),
            kind: MessageKind::Notice,
            text: "Buy a licence?".to_owned(),
        }]
    );
    let reply = rig.sent().into_iter().find(|m| m["id"] == "ask").unwrap();
    assert_eq!(reply["result"], Value::Null);
    assert!(reply.get("error").is_none());
}

#[test]
fn a_message_a_restarted_server_says_again_is_shown_once() {
    let timeouts = Timeouts {
        answer: Duration::from_millis(300),
        ..Timeouts::default()
    };
    let rig = Rig::timed(
        LIB,
        |checkout| {
            json!({
                "silent_in_first_start": ["textDocument/definition"],
                "show_on_initialized": [
                    {"method": "window/showMessage", "params": {"type": 3, "message": "Licence notice"}},
                ],
                "answers": {"textDocument/definition": [location(checkout, "src/lib.rs", 3, 7, 10)]},
            })
        },
        timeouts,
    );
    rig.hover_add().0.unwrap();
    rig.hover_add().0.unwrap();

    assert_eq!(
        rig.methods().iter().filter(|m| *m == "initialize").count(),
        2
    );
    assert_eq!(rig.messages.shown().len(), 1);
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

/// What the real Intelephense 1.18.5 says on `$this->schedule(5)` in `BILLING`.
const INTELEPHENSE_HOVER: &str = "__App\\Billing::schedule__

Charges the customer.

```php
<?php
public function schedule(int $amount): void { }
```

_@param_ `int $amount` in cents

_@return_ `void`";

#[test]
fn an_intelephense_answer_shows_the_method_line_its_signature_and_its_doc_summary_without_tags() {
    let rig = Rig::with_file(
        "app/Billing.php",
        BILLING,
        |repo| {
            json!({"answers": {
                // Intelephense's range starts at the doc comment and ends with the method.
                "textDocument/definition": [{
                    "uri": format!("file://{}", repo.join("app/Billing.php").display()),
                    "range": {"start": {"line": 6, "character": 4}, "end": {"line": 14, "character": 5}}
                }],
                "textDocument/references": [location(repo, "app/Billing.php", 18, 15, 23)],
                "textDocument/hover": {"contents": {"kind": "markdown", "value": INTELEPHENSE_HOVER}},
            }})
        },
        Timeouts::default(),
    );

    let (answer, _) = rig.navigate_at(19, 15, "schedule");
    let answer = answer.unwrap();

    let [definition] = answer.lookup.definitions.as_slice() else {
        panic!("one definition")
    };
    assert_eq!(definition.place.line, line(13));
    assert_eq!(definition.kind, DefinitionKind::Method);
    assert_eq!(
        &*definition.signature,
        "public function schedule(int $amount): void { }"
    );
    assert_eq!(definition.doc.as_deref(), Some("Charges the customer."));
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
fn a_server_that_reports_its_status_only_after_the_first_reply_is_still_waited_for() {
    let rig = Rig::new(LIB, |repo| {
        json!({"reports_status": true, "busy_ms": 400, "answers": {
            "textDocument/definition": [location(repo, "src/lib.rs", 3, 7, 10)],
        }})
    });

    let (answer, waits) = rig.hover_add();

    assert_eq!(answer.unwrap().lookup.definitions.len(), 1);
    assert_eq!(waits.last(), Some(&Waiting::Indexing));
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

/// Run in a process of its own whose PATH has relative and empty entries, by the test after it.
#[test]
#[ignore = "run by a_server_starts_with_no_relative_or_empty_entries_in_its_path"]
fn a_server_started_by_a_process_whose_path_has_relative_entries() {
    let rig = Rig::new(LIB, |_| json!({"reports_path": true, "answers": {}}));

    rig.hover_add().0.unwrap();

    let path = rig
        .sent()
        .iter()
        .find_map(|m| m["path"].as_str().map(str::to_owned))
        .unwrap();
    let entries: Vec<&str> = path.split(':').collect();
    assert!(entries.contains(&"/usr/bin"), "{path}");
    assert!(entries.iter().all(|entry| entry.starts_with('/')), "{path}");
}

#[test]
fn a_server_starts_with_no_relative_or_empty_entries_in_its_path() {
    let path = std::env::var("PATH").unwrap();
    let run = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "language_server::a_server_started_by_a_process_whose_path_has_relative_entries",
            "--ignored",
        ])
        .env("PATH", format!("bin::vendor/bin:{path}:/usr/bin:."))
        .output()
        .unwrap();

    let said = String::from_utf8_lossy(&run.stdout);
    assert!(run.status.success(), "{said}");
    assert!(said.contains("1 passed"), "{said}");
}

#[test]
fn a_server_that_did_not_answer_in_time_is_stopped_and_a_later_hover_starts_it_again() {
    let timeouts = Timeouts {
        answer: Duration::from_millis(300),
        ..Timeouts::default()
    };
    let rig = Rig::timed(
        LIB,
        |checkout| {
            json!({"silent_in_first_start": ["textDocument/definition"], "answers": {
                "textDocument/definition": [location(checkout, "src/lib.rs", 3, 7, 10)],
            }})
        },
        timeouts,
    );
    let first = rig.hover_add().0.unwrap();
    assert!(matches!(first.answered_by, AnsweredBy::ByName { .. }));

    let later = rig.hover_add().0.unwrap();

    assert_eq!(
        later.answered_by,
        AnsweredBy::LanguageServer(rust_analyzer())
    );
    assert_eq!(
        rig.methods().iter().filter(|m| *m == "initialize").count(),
        2
    );
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
fn a_question_slower_than_the_idle_time_is_answered_by_the_server() {
    let timeouts = Timeouts {
        idle: Duration::from_millis(200),
        ..Timeouts::default()
    };
    let rig = Rig::timed(
        LIB,
        |_| json!({"slow": {"method": "textDocument/definition", "ms": 1500}, "answers": {}}),
        timeouts,
    );

    let answer = rig.hover_add().0.unwrap();

    assert_eq!(
        answer.answered_by,
        AnsweredBy::LanguageServer(rust_analyzer())
    );
}

#[test]
fn hovers_that_get_no_answer_do_not_keep_an_idle_server_running() {
    let timeouts = Timeouts {
        indexing: Duration::from_millis(50),
        idle: Duration::from_millis(600),
        ..Timeouts::default()
    };
    let rig = Rig::timed(
        LIB,
        |_| json!({"indexing_ms": 60_000, "answers": {}}),
        timeouts,
    );

    for _ in 0..12 {
        rig.hover_add().0.unwrap();
        std::thread::sleep(Duration::from_millis(150));
    }

    assert!(
        rig.methods().contains(&"exit".to_owned()),
        "{:?}",
        rig.methods()
    );
}

#[test]
fn a_slow_server_holds_up_no_hover_another_server_of_the_repo_answers() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    repo.write("src/lib.rs", LIB);
    repo.write("app/Billing.php", BILLING);
    let head = Sha::new(&repo.commit("head"));
    let scratch = TempDir::new().unwrap();
    let script_file = scratch.path().join("script.json");
    let script = json!({
        "log": scratch.path().join("log").to_string_lossy(),
        "slow": {"server": "Php", "method": "textDocument/definition", "ms": 4000},
        "answers": {},
    });
    std::fs::write(&script_file, script.to_string()).unwrap();
    let servers = LanguageServers::new(
        Arc::new(FakeCatalog {
            script: script_file,
            servers: FakeServers::PerLanguage,
        }),
        ServerCheckouts::new(scratch.path().join("checkouts")),
        Timeouts::default(),
        Arc::new(IgnoredServerMessages),
    );
    let by_name: Arc<dyn CodeNavigator> =
        Arc::new(Lookups::new(FileSearch::changed_files(Arc::new([
            file_path("src/lib.rs"),
            file_path("app/Billing.php"),
        ]))));
    let navigator = servers.navigator(shop_codebase(), by_name);
    let hover = |path: &str, number: usize, column: usize, word: &str| {
        let code =
            LocalGit::configured_clone(&crate::support::shop_on_github(), repo.path()).unwrap();
        navigator
            .navigate(
                &code,
                &NavigationRequest {
                    sha: head.clone(),
                    at: Place {
                        path: file_path(path),
                        line: line(number),
                    },
                    column: Column::try_from(column).unwrap(),
                    word: Word::new(word).unwrap(),
                },
                &|_| {},
            )
            .unwrap()
    };

    std::thread::scope(|scope| {
        let php = scope.spawn(|| hover("app/Billing.php", 19, 15, "schedule"));
        std::thread::sleep(Duration::from_millis(1500));

        let rust = hover("src/lib.rs", 6, 23, "add");

        assert!(!php.is_finished(), "the PHP hover ended first");
        assert_eq!(
            rust.answered_by,
            AnsweredBy::LanguageServer(ServerName::of(Language::Rust))
        );
        assert_eq!(
            php.join().unwrap().answered_by,
            AnsweredBy::LanguageServer(ServerName::of(Language::Php))
        );
    });
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
fn a_dirty_clone_is_answered_by_the_server_from_the_checkout_and_stays_as_it_was() {
    let rig = Rig::answering_add();
    let edited = format!("{LIB}// edited\n");
    rig.repo.write("src/lib.rs", &edited);
    let status = rig.repo.git(&["status", "--porcelain"]);

    let answer = rig.hover_add().0.unwrap();

    assert_eq!(
        answer.answered_by,
        AnsweredBy::LanguageServer(rust_analyzer())
    );
    let [initialize] = rig.params_of("initialize").try_into().unwrap();
    assert_eq!(
        initialize["rootUri"],
        json!(format!("file://{}", rig.checkout.display()))
    );
    let [opened] = rig.params_of("textDocument/didOpen").try_into().unwrap();
    assert_eq!(opened["textDocument"]["text"], json!(LIB));
    assert_eq!(
        std::fs::read_to_string(rig.repo.path().join("src/lib.rs")).unwrap(),
        edited
    );
    assert_eq!(rig.repo.git(&["status", "--porcelain"]), status);
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

#[test]
fn a_location_in_the_borrowed_vendor_folder_shows_its_repo_path_and_opens_from_disk() {
    let rig = Rig::with_file(
        "app/Billing.php",
        BILLING,
        |repo| {
            json!({"answers": {"textDocument/definition": [
                location(repo, "vendor/laravel/framework/src/Clock.php", 9, 4, 12),
            ]}})
        },
        Timeouts::default(),
    );

    let (answer, _) = rig.navigate_at(19, 15, "schedule");
    let answer = answer.unwrap();

    let place = &answer.lookup.definitions[0].place;
    assert_eq!(
        place.path,
        file_path("vendor/laravel/framework/src/Clock.php")
    );
    assert_eq!(
        answer.lookup.on_disk(&place.path),
        Some(
            rig.checkout
                .join("vendor/laravel/framework/src/Clock.php")
                .as_path()
        )
    );
}

/// The pinned server of `language` the app downloaded into its cache folder. It is not downloaded here
/// unless `GUIDED_REVIEW_DOWNLOAD_SMOKE` is set, so a pin bump is tried by hand with that set.
fn downloaded_server(language: Language) -> Option<Arc<ManagedServers>> {
    let folders = AppFolders::xdg().ok()?;
    let managed = ManagedServers::new(
        ServerFolders {
            downloads: folders.language_servers(),
            indexes: folders.server_indexes(),
        },
        Arc::new(GitHub::from_env()),
        ServerPins::pinned(),
        Target::running(),
    );
    let installed = managed.is_downloaded(language);
    let may_download = std::env::var_os("GUIDED_REVIEW_DOWNLOAD_SMOKE").is_some();
    (installed || may_download).then(|| Arc::new(managed))
}

#[test]
fn the_real_rust_analyzer_finds_a_definition() {
    let Some(managed) = downloaded_server(Language::Rust) else {
        eprintln!("the pinned rust-analyzer is not downloaded: skipped");
        return;
    };
    let repo = FixtureRepo::cloned_from("acme/shop");
    repo.write(
        "Cargo.toml",
        "[package]\nname = \"tiny\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    );
    repo.write("src/lib.rs", LIB);
    let head = Sha::new(&repo.commit("head"));
    let code = LocalGit::configured_clone(&crate::support::shop_on_github(), repo.path()).unwrap();
    let scratch = TempDir::new().unwrap();
    let servers = LanguageServers::new(
        managed,
        ServerCheckouts::new(scratch.path().join("checkouts")),
        Timeouts::default(),
        Arc::new(IgnoredServerMessages),
    );
    let by_name: Arc<dyn CodeNavigator> =
        Arc::new(Lookups::new(FileSearch::changed_files(Arc::new([
            file_path("src/lib.rs"),
        ]))));

    let answer = servers
        .navigator(shop_codebase(), by_name)
        .navigate(
            &code,
            &NavigationRequest {
                sha: head,
                at: Place {
                    path: file_path("src/lib.rs"),
                    line: line(6),
                },
                column: Column::try_from(24).unwrap(),
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

/// Hovers `add` with the real rust-analyzer in a repo whose head also holds what `plant` writes, given
/// the server checkout's folder and a marker file; says whether anything wrote the marker. `None`
/// when the pinned rust-analyzer is not downloaded.
fn the_real_rust_analyzer_wrote_a_marker_with(
    plant: impl FnOnce(&FixtureRepo, &Path, &Path),
) -> Option<bool> {
    let managed = downloaded_server(Language::Rust)?;
    let repo = FixtureRepo::cloned_from("acme/shop");
    repo.write(
        "Cargo.toml",
        "[package]\nname = \"tiny\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    );
    repo.write("src/lib.rs", LIB);
    let scratch = TempDir::new().unwrap();
    let checkouts = scratch.path().join("checkouts");
    let marker = scratch.path().join("ran");
    plant(&repo, &checkouts.join("acme/shop"), &marker);
    let head = Sha::new(&repo.commit("head"));
    let code = LocalGit::configured_clone(&crate::support::shop_on_github(), repo.path()).unwrap();
    let servers = LanguageServers::new(
        managed,
        ServerCheckouts::new(checkouts),
        Timeouts::default(),
        Arc::new(IgnoredServerMessages),
    );
    let by_name: Arc<dyn CodeNavigator> =
        Arc::new(Lookups::new(FileSearch::changed_files(Arc::new([
            file_path("src/lib.rs"),
        ]))));

    servers
        .navigator(shop_codebase(), by_name)
        .navigate(
            &code,
            &NavigationRequest {
                sha: head,
                at: Place {
                    path: file_path("src/lib.rs"),
                    line: line(6),
                },
                column: Column::try_from(24).unwrap(),
                word: Word::new("add").unwrap(),
            },
            &|_| {},
        )
        .unwrap();
    servers.stop_all();

    Some(marker.exists())
}

fn marking(marker: &Path) -> String {
    format!("#!/bin/sh\ntouch '{}'\n", marker.display())
}

#[test]
fn the_real_rust_analyzer_runs_no_rustc_wrapper_that_a_cargo_config_in_the_prs_files_names() {
    let ran = the_real_rust_analyzer_wrote_a_marker_with(|repo, _, marker| {
        repo.write(
            ".cargo/config.toml",
            "[build]\nrustc-wrapper = \"tools/wrap.sh\"\n",
        );
        std::fs::create_dir_all(repo.path().join("tools")).unwrap();
        write_executable(
            &repo.path().join("tools/wrap.sh"),
            format!("{}exec \"$@\"\n", marking(marker)),
        );
    });

    let Some(ran) = ran else {
        eprintln!("the pinned rust-analyzer is not downloaded: skipped");
        return;
    };
    assert!(!ran, "the PR's rustc wrapper ran");
}

#[test]
fn the_real_rust_analyzer_runs_no_sysroot_tool_that_a_rust_project_file_in_the_prs_files_names() {
    let ran = the_real_rust_analyzer_wrote_a_marker_with(|repo, _, marker| {
        repo.write(
            "rust-project.json",
            r#"{"sysroot": "sysroot", "crates": [{"root_module": "src/lib.rs", "edition": "2021", "deps": []}]}"#,
        );
        std::fs::create_dir_all(repo.path().join("sysroot/bin")).unwrap();
        for tool in ["cargo", "rustc"] {
            write_executable(
                &repo.path().join("sysroot/bin").join(tool),
                format!("{}exit 1\n", marking(marker)),
            );
        }
    });

    let Some(ran) = ran else {
        eprintln!("the pinned rust-analyzer is not downloaded: skipped");
        return;
    };
    assert!(!ran, "the PR's sysroot tool ran");
}

const BILLING: &str = "<?php

namespace App;

class Billing
{
    /**
     * Charges the customer.
     *
     * @param int $amount in cents
     * @return void
     */
    public function schedule(int $amount): void
    {
    }

    public function run(): void
    {
        $this->schedule(5);
    }
}
";

const SHIPPING: &str = "<?php

namespace App;

class Shipping
{
    public function schedule(): void
    {
    }
}
";

#[test]
fn the_real_intelephense_finds_the_method_of_the_class_the_code_calls() {
    let Some(managed) = downloaded_server(Language::Php) else {
        eprintln!("the pinned Intelephense is not downloaded: skipped");
        return;
    };
    let repo = FixtureRepo::cloned_from("acme/shop");
    repo.write("app/Billing.php", BILLING);
    repo.write("app/Shipping.php", SHIPPING);
    let head = Sha::new(&repo.commit("head"));
    let code = LocalGit::configured_clone(&crate::support::shop_on_github(), repo.path()).unwrap();
    let scratch = TempDir::new().unwrap();
    let servers = LanguageServers::new(
        managed,
        ServerCheckouts::new(scratch.path().join("checkouts")),
        Timeouts::default(),
        Arc::new(IgnoredServerMessages),
    );
    let by_name: Arc<dyn CodeNavigator> =
        Arc::new(Lookups::new(FileSearch::changed_files(Arc::new([
            file_path("app/Billing.php"),
            file_path("app/Shipping.php"),
        ]))));

    let answer = servers
        .navigator(shop_codebase(), by_name)
        .navigate(
            &code,
            &NavigationRequest {
                sha: head,
                at: Place {
                    path: file_path("app/Billing.php"),
                    line: line(19),
                },
                column: Column::try_from(15).unwrap(),
                word: Word::new("schedule").unwrap(),
            },
            &|_| {},
        )
        .unwrap();
    servers.stop_all();

    assert_eq!(
        answer.answered_by,
        AnsweredBy::LanguageServer(ServerName::new("Intelephense"))
    );
    let [definition] = &answer.lookup.definitions[..] else {
        panic!("one definition: {:?}", answer.lookup.definitions);
    };
    assert_eq!(definition.place.path, file_path("app/Billing.php"));
    assert_eq!(definition.place.line, line(13));
}

fn answering_typescript(hover: &str) -> Rig {
    let hover = hover.to_owned();
    Rig::with_file(
        "src/shapes.ts",
        SHAPES,
        move |checkout| {
            json!({"answers": {
                "textDocument/definition": [location(checkout, "src/shapes.ts", 0, 13, 19)],
                "textDocument/hover": {"contents": {"kind": "markdown", "value": hover}},
            }})
        },
        Timeouts::default(),
    )
}

#[test]
fn a_typescript_method_hover_shows_its_signature_as_a_method_and_its_doc_summary() {
    let rig = answering_typescript(
        "\n```typescript\n(method) Circle.area(): number\n```\nThe area in square units.\n\n*@returns* — the area",
    );

    let answer = rig.navigate_at(12, 28, "area").0.unwrap();

    let definition = &answer.lookup.definitions[0];
    assert_eq!(&*definition.signature, "(method) Circle.area(): number");
    assert_eq!(definition.kind, DefinitionKind::Method);
    assert_eq!(definition.doc.as_deref(), Some("The area in square units."));
}

#[test]
fn a_typescript_import_hover_shows_the_line_that_declares_the_imported_name() {
    let rig =
        answering_typescript("\n```typescript\n(alias) new Circle(): Circle\nimport Circle\n```\n");

    let answer = rig.navigate_at(12, 28, "Circle").0.unwrap();

    let definition = &answer.lookup.definitions[0];
    assert_eq!(&*definition.signature, "export class Circle {");
    assert_eq!(definition.kind, DefinitionKind::Class);
}

const SHAPES: &str = "export class Circle {
    /**
     * The area in square units.
     *
     * @returns the area
     */
    area(): number {
        return 3;
    }

    describe(): string {
        return `area ${this.area()}`;
    }
}

export class Square {
    area(): number {
        return 4;
    }
}
";

const MAIN: &str = "import { Circle } from './shapes';

export const text = new Circle().describe();
";

#[test]
fn the_real_typescript_language_server_finds_the_method_of_the_class_the_code_calls_and_an_import()
{
    let Some(managed) = downloaded_server(Language::TypeScript) else {
        eprintln!("the pinned typescript-language-server is not downloaded: skipped");
        return;
    };
    let repo = FixtureRepo::cloned_from("acme/shop");
    repo.write("src/shapes.ts", SHAPES);
    repo.write("src/main.ts", MAIN);
    let head = Sha::new(&repo.commit("head"));
    repo.write(
        "node_modules/typescript/package.json",
        r#"{"name": "typescript", "version": "5.9.3"}"#,
    );
    repo.write(
        "node_modules/typescript/lib/tsserver.js",
        "process.exit(3); // the PR's own TypeScript, never run\n",
    );
    let code = LocalGit::configured_clone(&crate::support::shop_on_github(), repo.path()).unwrap();
    let scratch = TempDir::new().unwrap();
    let servers = LanguageServers::new(
        managed,
        ServerCheckouts::new(scratch.path().join("checkouts")),
        Timeouts::default(),
        Arc::new(IgnoredServerMessages),
    );
    let by_name: Arc<dyn CodeNavigator> =
        Arc::new(Lookups::new(FileSearch::changed_files(Arc::new([
            file_path("src/shapes.ts"),
            file_path("src/main.ts"),
        ]))));
    let navigator = servers.navigator(shop_codebase(), by_name);
    let ask = |path: &str, number: usize, column: usize, word: &str| {
        navigator
            .navigate(
                &code,
                &NavigationRequest {
                    sha: head.clone(),
                    at: Place {
                        path: file_path(path),
                        line: line(number),
                    },
                    column: Column::try_from(column).unwrap(),
                    word: Word::new(word).unwrap(),
                },
                &|_| {},
            )
            .unwrap()
    };

    let area = ask("src/shapes.ts", 12, 28, "area");
    let circle = ask("src/main.ts", 3, 25, "Circle");
    servers.stop_all();

    let server = AnsweredBy::LanguageServer(ServerName::new("typescript-language-server"));
    assert_eq!(area.answered_by, server);
    let [definition] = &area.lookup.definitions[..] else {
        panic!("one definition: {:?}", area.lookup.definitions);
    };
    eprintln!("{definition:?}");
    assert_eq!(definition.place.path, file_path("src/shapes.ts"));
    assert_eq!(definition.place.line, line(7));
    assert_eq!(circle.answered_by, server);
    let [definition] = &circle.lookup.definitions[..] else {
        panic!("one definition: {:?}", circle.lookup.definitions);
    };
    eprintln!("{definition:?}");
    assert_eq!(definition.place.path, file_path("src/shapes.ts"));
    assert_eq!(definition.place.line, line(1));
}

#[test]
fn the_real_typescript_language_server_loads_no_tsserver_plugin_that_the_prs_tsconfig_names() {
    let Some(managed) = downloaded_server(Language::TypeScript) else {
        eprintln!("the pinned typescript-language-server is not downloaded: skipped");
        return;
    };
    let scratch = TempDir::new().unwrap();
    let marker = scratch.path().join("loaded");
    let repo = FixtureRepo::cloned_from("acme/shop");
    repo.write("src/shapes.ts", SHAPES);
    repo.write(
        "tsconfig.json",
        r#"{"compilerOptions": {"plugins": [{"name": "evil-plugin"}]}}"#,
    );
    let head = Sha::new(&repo.commit("head"));
    repo.write(
        "node_modules/evil-plugin/package.json",
        r#"{"name": "evil-plugin", "main": "index.js"}"#,
    );
    repo.write(
        "node_modules/evil-plugin/index.js",
        &format!(
            "require('fs').writeFileSync({:?}, '');\nmodule.exports = () => ({{ create: (info) => info.languageService }});\n",
            marker.display().to_string()
        ),
    );
    let code = LocalGit::configured_clone(&crate::support::shop_on_github(), repo.path()).unwrap();
    let servers = LanguageServers::new(
        managed,
        ServerCheckouts::new(scratch.path().join("checkouts")),
        Timeouts::default(),
        Arc::new(IgnoredServerMessages),
    );
    let by_name: Arc<dyn CodeNavigator> =
        Arc::new(Lookups::new(FileSearch::changed_files(Arc::new([
            file_path("src/shapes.ts"),
        ]))));

    let area = servers
        .navigator(shop_codebase(), by_name)
        .navigate(
            &code,
            &NavigationRequest {
                sha: head,
                at: Place {
                    path: file_path("src/shapes.ts"),
                    line: line(12),
                },
                column: Column::try_from(28).unwrap(),
                word: Word::new("area").unwrap(),
            },
            &|_| {},
        )
        .unwrap();
    servers.stop_all();

    assert_eq!(
        area.answered_by,
        AnsweredBy::LanguageServer(ServerName::new("typescript-language-server"))
    );
    assert!(!marker.exists(), "the PR's tsserver plugin was loaded");
}

#[test]
fn a_review_at_another_head_moves_the_checkout_and_tells_the_running_server_which_files_changed() {
    let rig = Rig::answering_add();
    rig.hover_add().0.unwrap();
    let later = format!("{LIB}// later\n");

    let answer = rig.hover_add_at_a_new_head(&later).0.unwrap();

    assert_eq!(
        answer.answered_by,
        AnsweredBy::LanguageServer(rust_analyzer())
    );
    let methods = rig.methods();
    assert_eq!(methods.iter().filter(|m| *m == "initialize").count(), 1);
    assert!(!methods.contains(&"shutdown".to_owned()), "{methods:?}");
    let file = format!("file://{}/src/lib.rs", rig.checkout.display());
    let [watched] = rig
        .params_of("workspace/didChangeWatchedFiles")
        .try_into()
        .unwrap();
    assert_eq!(watched["changes"], json!([{"uri": file, "type": 2}]));
    let opened = rig.params_of("textDocument/didOpen");
    assert_eq!(opened.len(), 2, "the changed file is opened again");
    assert_eq!(opened[1]["textDocument"]["text"], json!(later));
    assert_eq!(rig.params_of("textDocument/didClose").len(), 1);
    assert_eq!(
        std::fs::read_to_string(rig.checkout.join("src/lib.rs")).unwrap(),
        later
    );
}

#[test]
fn a_server_hears_nothing_of_program_settings_the_checkout_keeps_out() {
    let rig = Rig::answering_add();
    rig.hover_add().0.unwrap();
    rig.repo.write(
        ".cargo/config.toml",
        "[build]\nrustc-wrapper = \"wrap.sh\"\n",
    );

    rig.hover_add_at_a_new_head(&format!("{LIB}// later\n"))
        .0
        .unwrap();

    let file = format!("file://{}/src/lib.rs", rig.checkout.display());
    let [watched] = rig
        .params_of("workspace/didChangeWatchedFiles")
        .try_into()
        .unwrap();
    assert_eq!(watched["changes"], json!([{"uri": file, "type": 2}]));
}

#[test]
fn a_location_in_a_borrowed_dependency_shows_as_a_repo_path_and_opens_from_disk() {
    let rig = Rig::new(
        LIB,
        |checkout| json!({"answers": {"textDocument/definition": [location(checkout, "vendor/laravel/framework.php", 0, 6, 9)]}}),
    );
    rig.repo
        .write("vendor/laravel/framework.php", "<?php // installed\n");

    let answer = rig.hover_add().0.unwrap();

    let [definition] = answer.lookup.definitions.as_slice() else {
        panic!("one definition")
    };
    assert_eq!(
        definition.place.path,
        file_path("vendor/laravel/framework.php")
    );
    assert_eq!(
        answer.lookup.on_disk(&definition.place.path),
        Some(rig.checkout.join("vendor/laravel/framework.php").as_path())
    );
    assert_eq!(&*definition.signature, "<?php // installed");
}

#[test]
fn a_typescript_definition_in_the_borrowed_node_modules_shows_its_repo_path_and_opens_from_disk() {
    let rig = Rig::with_file(
        "src/total.ts",
        "import { sum } from 'lodash';

export const total = sum([1, 2]);
",
        |checkout| {
            json!({"answers": {"textDocument/definition": [
                location(checkout, "node_modules/lodash/sum.d.ts", 0, 16, 19),
            ]}})
        },
        Timeouts::default(),
    );
    rig.repo.write(
        "node_modules/lodash/sum.d.ts",
        "export function sum(values: number[]): number;
",
    );

    let answer = rig.navigate_at(3, 21, "sum").0.unwrap();

    let [definition] = answer.lookup.definitions.as_slice() else {
        panic!("one definition: {:?}", answer.lookup.definitions)
    };
    assert_eq!(
        definition.place.path,
        file_path("node_modules/lodash/sum.d.ts")
    );
    assert_eq!(
        answer.lookup.on_disk(&definition.place.path),
        Some(rig.checkout.join("node_modules/lodash/sum.d.ts").as_path())
    );
    assert_eq!(
        &*definition.signature,
        "export function sum(values: number[]): number;"
    );
    assert_eq!(
        rig.params_of("textDocument/didOpen")[0]["textDocument"]["languageId"],
        "typescript"
    );
}

#[test]
fn a_question_the_server_cancels_because_files_changed_is_asked_again() {
    let rig = Rig::new(LIB, |checkout| {
        json!({"content_modified_once": ["textDocument/definition"], "answers": {
            "textDocument/definition": [location(checkout, "src/lib.rs", 3, 7, 10)],
        }})
    });

    let answer = rig.hover_add().0.unwrap();

    assert_eq!(
        answer.answered_by,
        AnsweredBy::LanguageServer(rust_analyzer())
    );
    assert_eq!(answer.lookup.definitions.len(), 1);
    assert_eq!(rig.params_of("textDocument/definition").len(), 2);
}

#[test]
fn a_local_review_is_answered_in_the_server_checkout_at_its_snapshot_and_update_moves_it_to_the_next()
 {
    let rig = Rig::answering_add();
    rig.repo
        .write("src/lib.rs", &format!("{LIB}pub fn first_draft() {{}}\n"));
    let work_tree = LocalWorkTree::containing(rig.repo.path()).unwrap();
    let first = work_tree.make_snapshot().unwrap().head;
    rig.repo
        .write("src/lib.rs", &format!("{LIB}pub fn second_draft() {{}}\n"));
    let second = work_tree.make_snapshot().unwrap().head;
    let code = LocalGit::at_worktree(work_tree.change_set().unwrap().folder());
    let local_review_of = |snapshot: Sha| {
        rig.servers
            .navigator(shop_codebase(), rig.by_name())
            .navigate(
                &code,
                &NavigationRequest {
                    sha: snapshot,
                    at: Place {
                        path: file_path("src/lib.rs"),
                        line: line(6),
                    },
                    column: Column::try_from(23).unwrap(),
                    word: Word::new("add").unwrap(),
                },
                &|_| {},
            )
            .unwrap()
    };

    let answer = local_review_of(first);
    assert_eq!(
        answer.answered_by,
        AnsweredBy::LanguageServer(rust_analyzer())
    );
    assert!(read_checkout(&rig, "src/lib.rs").contains("first_draft"));

    local_review_of(second);

    assert!(read_checkout(&rig, "src/lib.rs").contains("second_draft"));
    assert_eq!(
        rig.methods().iter().filter(|m| *m == "initialize").count(),
        1
    );
}

/// A review of the rig's repo, whose names the rig's servers may answer.
fn review_of(rig: &Rig, answer: Option<ServerAnswer>) -> (Review, LocalGit) {
    let base = rig.repo.git(&["rev-parse", "HEAD"]);
    rig.repo
        .write("src/lib.rs", &format!("{LIB}pub fn later() {{}}\n"));
    let head = rig.repo.commit("head of the PR");
    let tour: GuidedTour = serde_json::from_value(json!({
        "pr": "https://github.com/acme/shop/pull/7",
        "base": base,
        "head": head,
        "title": "Adds later",
        "summary": [],
        "steps": [{"id": "lib", "title": "Later", "why": "because", "pieces": [{"file": "src/lib.rs"}]}],
    }))
    .unwrap();
    let source =
        LocalGit::configured_clone(&crate::support::shop_on_github(), rig.repo.path()).unwrap();
    let state_file = ReviewStateFile::for_subject(
        &rig.repo.folders(),
        &"https://github.com/acme/shop/pull/7".parse().unwrap(),
    );
    let mut review = Review::open(
        tour,
        &source,
        state_file,
        crate::support::fake_ports::every_capability(),
    )
    .unwrap();
    review.use_language_server_pool(&rig.servers, shop_codebase(), answer);
    (review, source)
}

/// Hovers a head line of `src/lib.rs` as the app does, running the lookup where the test runs.
fn hover_in(
    review: &mut Review,
    source: &LocalGit,
    number: usize,
    column: usize,
    word: &str,
) -> DefinitionCard {
    let hovered = HoveredName {
        at: LineLocation {
            path: file_path("src/lib.rs"),
            side: Side::Head,
            line: line(number),
        },
        column: Column::try_from(column).unwrap(),
        word: Word::new(word).unwrap(),
    };
    if let Some(work) = review.look_up_name(&hovered) {
        review.name_looked_up(work.run(source));
    }
    (*review.definition_card(&hovered).unwrap()).clone()
}

#[test]
fn a_no_in_settings_stops_the_running_server_and_an_open_reviews_next_hover_is_answered_by_name() {
    let rig = Rig::answering_add();
    let (mut review, source) = review_of(&rig, Some(ServerAnswer::Yes));
    hover_in(&mut review, &source, 6, 23, "add");
    assert_eq!(rig.params_of("textDocument/definition").len(), 1);

    rig.servers.answered(&shop_codebase(), ServerAnswer::No);

    assert!(
        rig.methods().contains(&"exit".to_owned()),
        "{:?}",
        rig.methods()
    );
    let DefinitionCard::Found(card) = hover_in(&mut review, &source, 6, 7, "total") else {
        panic!("a found card")
    };
    assert_eq!(card.question, None);
    assert_eq!(rig.params_of("textDocument/definition").len(), 1);
}

#[test]
fn a_yes_in_settings_lets_an_open_reviews_next_hover_be_answered_by_the_server_without_asking() {
    let rig = Rig::answering_add();
    let (mut review, source) = review_of(&rig, Some(ServerAnswer::No));
    hover_in(&mut review, &source, 6, 23, "add");
    assert!(rig.methods().is_empty(), "{:?}", rig.methods());

    rig.servers.answered(&shop_codebase(), ServerAnswer::Yes);

    let DefinitionCard::Found(card) = hover_in(&mut review, &source, 6, 23, "add") else {
        panic!("a found card")
    };
    assert_eq!(card.question, None);
    assert_eq!(rig.params_of("textDocument/definition").len(), 1);
}

fn read_checkout(rig: &Rig, file: &str) -> String {
    std::fs::read_to_string(rig.checkout.join(file)).unwrap()
}

#[test]
fn a_server_hears_every_change_since_its_own_head_when_another_servers_review_moved_the_checkout() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    repo.write("src/lib.rs", LIB);
    repo.write("app/Billing.php", BILLING);
    let first = Sha::new(&repo.commit("first"));
    let scratch = TempDir::new().unwrap();
    let script_file = scratch.path().join("script.json");
    let log = scratch.path().join("log");
    std::fs::write(
        &script_file,
        json!({"log": log.to_string_lossy(), "answers": {}}).to_string(),
    )
    .unwrap();
    let servers = LanguageServers::new(
        Arc::new(FakeCatalog {
            script: script_file,
            servers: FakeServers::PerLanguage,
        }),
        ServerCheckouts::new(scratch.path().join("checkouts")),
        Timeouts::default(),
        Arc::new(IgnoredServerMessages),
    );
    let checkout = scratch.path().join("checkouts/acme/shop");
    let code = LocalGit::configured_clone(&crate::support::shop_on_github(), repo.path()).unwrap();
    let by_name: Arc<dyn CodeNavigator> =
        Arc::new(Lookups::new(FileSearch::changed_files(Arc::new([
            file_path("src/lib.rs"),
            file_path("app/Billing.php"),
        ]))));
    let navigator = servers.navigator(shop_codebase(), by_name);
    let hover = |sha: &Sha, path: &str, number: usize, column: usize, word: &str| {
        navigator
            .navigate(
                &code,
                &NavigationRequest {
                    sha: sha.clone(),
                    at: Place {
                        path: file_path(path),
                        line: line(number),
                    },
                    column: Column::try_from(column).unwrap(),
                    word: Word::new(word).unwrap(),
                },
                &|_| {},
            )
            .unwrap()
    };
    hover(&first, "src/lib.rs", 6, 23, "add");
    repo.write("src/lib.rs", &format!("{LIB}// later\n"));
    repo.write("app/Billing.php", &format!("{BILLING}// later\n"));
    let second = Sha::new(&repo.commit("second"));
    hover(&second, "app/Billing.php", 19, 15, "schedule");

    hover(&second, "src/lib.rs", 6, 23, "add");

    let rust_heard: Vec<Value> = std::fs::read_to_string(format!("{}.Rust", log.display()))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .filter(|m| m["method"] == "workspace/didChangeWatchedFiles")
        .map(|m| m["params"]["changes"].clone())
        .collect();
    let uri = |path: &str| format!("file://{}", checkout.join(path).display());
    assert_eq!(
        rust_heard,
        [json!([
            {"uri": uri("app/Billing.php"), "type": 2},
            {"uri": uri("src/lib.rs"), "type": 2},
        ])]
    );
}
