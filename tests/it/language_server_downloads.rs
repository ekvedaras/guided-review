use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use flate2::Compression;
use flate2::write::GzEncoder;
use guided_review::local_git::LocalGit;
use guided_review::navigation::{
    Answer, AnsweredBy, CodeNavigator, Column, FileSearch, LanguageServers, Lookups,
    ManagedServers, NavigationRequest, Place, ReleasePin, ServerName, ServerPins, Timeouts,
    Waiting, Word,
};
use guided_review::platform::Os;
use guided_review::pull_request::{RepoName, Sha};
use guided_review::update::{Cpu, ReleasesPort, Target};
use serde_json::json;
use sha2::{Digest, Sha256};
use tempfile::TempDir;

use crate::support::{FixtureRepo, file_path, line};

const LIB: &str = "pub fn add(a: u32, b: u32) -> u32 { a + b }

pub fn total() -> u32 { add(1, 2) }
";

const TARGET: Target = Target {
    os: Os::MacOs,
    cpu: Cpu::Arm64,
};

const TAG: &str = "2030-01-01";

fn url_of(tag: &str, target: Target) -> String {
    format!(
        "https://github.com/rust-lang/rust-analyzer/releases/download/{tag}/rust-analyzer-{}.gz",
        target.triple()
    )
}

fn gzip(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(bytes).unwrap();
    encoder.finish().unwrap()
}

fn sha256_of(bytes: &[u8]) -> String {
    guided_review::hex::lower(&Sha256::digest(bytes))
}

/// Stands in for GitHub's release downloads: serves what it was given and counts the requests.
#[derive(Default)]
struct Host {
    files: Mutex<Vec<(String, Vec<u8>)>>,
    requested: Mutex<Vec<String>>,
    failing: Mutex<bool>,
    slow: Mutex<std::time::Duration>,
}

impl ReleasesPort for Host {
    fn latest_release(&self) -> anyhow::Result<guided_review::update::PublishedRelease> {
        anyhow::bail!("not asked")
    }

    fn download(&self, url: &str) -> anyhow::Result<Vec<u8>> {
        self.requested.lock().unwrap().push(url.to_owned());
        std::thread::sleep(*self.slow.lock().unwrap());
        if *self.failing.lock().unwrap() {
            anyhow::bail!("GitHub is down");
        }
        self.files
            .lock()
            .unwrap()
            .iter()
            .find(|(served, _)| served == url)
            .map(|(_, bytes)| bytes.clone())
            .ok_or_else(|| anyhow::anyhow!("404 for {url}"))
    }
}

struct Rig {
    repo: FixtureRepo,
    head: Sha,
    scratch: TempDir,
    cache: PathBuf,
    host: Arc<Host>,
    servers: LanguageServers,
    managed: Arc<ManagedServers>,
}

impl Rig {
    /// The release serves a "rust-analyzer" that is the scripted fake language server.
    fn new() -> Self {
        let repo = FixtureRepo::cloned_from("acme/shop");
        repo.write("src/lib.rs", LIB);
        let head = Sha::new(&repo.commit("head"));
        let scratch = TempDir::new().unwrap();
        let script = json!({
            "log": scratch.path().join("log").to_string_lossy(),
            "reports_status": true,
            "answers": {
                "textDocument/definition": [{
                    "uri": format!("file://{}", repo.path().join("src/lib.rs").display()),
                    "range": {"start": {"line": 0, "character": 7}, "end": {"line": 0, "character": 10}}
                }],
            },
        });
        let script_file = scratch.path().join("script.json");
        std::fs::write(&script_file, script.to_string()).unwrap();
        let fake = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake_lsp.py");
        let binary = format!(
            "#!/bin/sh\nexec python3 '{}' '{}'\n",
            fake.display(),
            script_file.display()
        );
        let archive = gzip(binary.as_bytes());
        let host = Arc::new(Host::default());
        host.files
            .lock()
            .unwrap()
            .push((url_of(TAG, TARGET), archive.clone()));
        let pins = ServerPins {
            rust_analyzer: ReleasePin {
                tag: TAG.to_owned(),
                sha256: vec![(TARGET, sha256_of(&archive))],
            },
        };
        let cache = scratch.path().join("cache/language-servers");
        let managed = Arc::new(ManagedServers::new(
            cache.clone(),
            Arc::clone(&host) as Arc<dyn ReleasesPort>,
            pins,
            TARGET,
        ));
        let servers = LanguageServers::new(
            Arc::clone(&managed) as Arc<dyn guided_review::navigation::ServerCatalog>,
            Timeouts::default(),
        );
        Self {
            repo,
            head,
            scratch,
            cache,
            host,
            servers,
            managed,
        }
    }

    fn hover_add(&self) -> (anyhow::Result<Answer>, Vec<Waiting>) {
        let code =
            LocalGit::configured_clone(&RepoName::new("acme", "shop"), self.repo.path()).unwrap();
        let by_name: Arc<dyn CodeNavigator> =
            Arc::new(Lookups::new(FileSearch::changed_files(Arc::new([
                file_path("src/lib.rs"),
            ]))));
        let waits = Mutex::new(Vec::new());
        let answer = self.servers.navigator(by_name).navigate(
            &code,
            &NavigationRequest {
                sha: self.head.clone(),
                at: Place {
                    path: file_path("src/lib.rs"),
                    line: line(3),
                },
                column: Column::try_from(23).unwrap(),
                word: Word::new("add").unwrap(),
            },
            &|waiting| waits.lock().unwrap().push(waiting),
        );
        (answer, waits.into_inner().unwrap())
    }

    fn requested(&self) -> Vec<String> {
        self.host.requested.lock().unwrap().clone()
    }

    fn server_started(&self) -> bool {
        self.scratch.path().join("log").exists()
    }

    fn serving(&self, archive: &[u8]) {
        self.host.files.lock().unwrap()[0].1 = archive.to_vec();
    }

    fn downloads(&self) -> Vec<String> {
        std::fs::read_dir(&self.cache).map_or_else(
            |_| Vec::new(),
            |entries| {
                entries
                    .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
                    .collect()
            },
        )
    }
}

fn rust_analyzer() -> ServerName {
    ServerName::new("rust-analyzer")
}

#[test]
fn the_first_rust_hover_downloads_the_pinned_rust_analyzer_for_this_machine_and_starts_it() {
    let rig = Rig::new();

    let (answer, waits) = rig.hover_add();
    let answer = answer.unwrap();

    assert_eq!(
        answer.answered_by,
        AnsweredBy::LanguageServer(rust_analyzer())
    );
    assert_eq!(waits.first(), Some(&Waiting::Downloading(rust_analyzer())));
    assert_eq!(rig.requested(), vec![url_of(TAG, TARGET)]);
    assert!(
        rig.cache
            .join(format!("rust-analyzer-{TAG}/rust-analyzer"))
            .is_file()
    );
}

fn note_of(answer: &Answer) -> Option<String> {
    match &answer.answered_by {
        AnsweredBy::ByName { because } => because.as_ref().map(|reason| reason.as_str().to_owned()),
        AnsweredBy::LanguageServer(_) => None,
    }
}

#[test]
fn a_download_that_is_not_the_pinned_one_is_refused_unpacked_nowhere_and_never_run() {
    let rig = Rig::new();
    rig.serving(&gzip(b"#!/bin/sh\ntouch /tmp/never\n"));

    let (answer, _) = rig.hover_add();
    let answer = answer.unwrap();

    let note = note_of(&answer).expect("answered by name with a note");
    assert!(note.contains("rust-analyzer"), "{note}");
    assert!(note.contains("does not match its checksum"), "{note}");
    assert!(rig.downloads().is_empty(), "{:?}", rig.downloads());
    assert!(!rig.server_started());
}

#[test]
fn a_failed_download_falls_back_by_name_and_the_next_hover_tries_again() {
    let rig = Rig::new();
    *rig.host.failing.lock().unwrap() = true;

    let (failed, _) = rig.hover_add();
    let failed = failed.unwrap();
    let note = note_of(&failed).expect("answered by name with a note");
    assert!(note.contains("could not be downloaded"), "{note}");
    assert_eq!(rig.downloads(), Vec::<String>::new());
    *rig.host.failing.lock().unwrap() = false;
    let (retried, _) = rig.hover_add();

    assert_eq!(
        retried.unwrap().answered_by,
        AnsweredBy::LanguageServer(rust_analyzer())
    );
    assert_eq!(rig.requested().len(), 2);
}

#[test]
fn two_hovers_while_downloading_start_one_download() {
    let rig = Rig::new();
    *rig.host.slow.lock().unwrap() = std::time::Duration::from_millis(400);

    let answers = std::thread::scope(|scope| {
        let hovers = [
            scope.spawn(|| rig.hover_add()),
            scope.spawn(|| rig.hover_add()),
        ];
        hovers.map(|hover| hover.join().unwrap())
    });

    assert_eq!(rig.requested().len(), 1);
    for (answer, waits) in answers {
        assert_eq!(
            answer.unwrap().answered_by,
            AnsweredBy::LanguageServer(rust_analyzer())
        );
        assert!(waits.contains(&Waiting::Downloading(rust_analyzer())));
    }
}

#[test]
fn a_server_already_downloaded_is_neither_downloaded_nor_waited_for_again() {
    let rig = Rig::new();
    rig.hover_add().0.unwrap();

    let (_, waits) = rig.hover_add();

    assert_eq!(rig.requested().len(), 1);
    assert!(!waits.contains(&Waiting::Downloading(rust_analyzer())));
}

#[test]
fn downloads_of_versions_the_app_no_longer_pins_are_removed_at_start() {
    let rig = Rig::new();
    rig.hover_add().0.unwrap();
    let old = rig.cache.join("rust-analyzer-2000-01-01");
    std::fs::create_dir_all(&old).unwrap();
    std::fs::write(old.join("rust-analyzer"), "old").unwrap();

    rig.managed.remove_unpinned().unwrap();

    assert_eq!(rig.downloads(), vec![format!("rust-analyzer-{TAG}")]);
}

#[test]
fn every_target_the_app_runs_on_has_a_pinned_sha256() {
    let pins = ServerPins::pinned().rust_analyzer;

    for os in [Os::MacOs, Os::Linux] {
        for cpu in [Cpu::Arm64, Cpu::X86_64] {
            let target = Target { os, cpu };
            let sha = pins
                .sha256
                .iter()
                .find(|(pinned, _)| *pinned == target)
                .map(|(_, sha)| sha.as_str());
            let sha = sha.unwrap_or_else(|| panic!("{} is pinned", target.triple()));
            assert!(
                sha.len() == 64 && sha.chars().all(|c| c.is_ascii_hexdigit()),
                "{sha}"
            );
        }
    }
}
