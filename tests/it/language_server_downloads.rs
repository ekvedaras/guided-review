use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use flate2::Compression;
use flate2::write::GzEncoder;
use guided_review::code_source::{BlobInfo, CodeSource, FileDiff};
use guided_review::codebase::Codebase;
use guided_review::file_path::FilePath;
use guided_review::local_git::LocalGit;
use guided_review::navigation::{
    Answer, AnsweredBy, CodeNavigator, Column, FallbackReason, FileSearch, LanguageServers,
    Lookups, ManagedServers, NavigationRequest, PackagePin, Place, ReleasePin, ServerCheckouts,
    ServerFolders, ServerName, ServerPins, Timeouts, Waiting, Word,
};
use guided_review::platform::Os;
use guided_review::pull_request::{RepoName, Sha};
use guided_review::update::{Cpu, ReleasesPort, Target};
use serde_json::json;
use sha2::{Digest, Sha256, Sha512};
use tempfile::TempDir;

use crate::support::{FixtureRepo, file_path, line, tarball};

const LIB: &str = "pub fn add(a: u32, b: u32) -> u32 { a + b }

pub fn total() -> u32 { add(1, 2) }
";

const TARGET: Target = Target {
    os: Os::MacOs,
    cpu: Cpu::Arm64,
};

const TAG: &str = "2030-01-01";

const NODE: &str = "v99.0.0";

const INTELEPHENSE: &str = "9.9.9";

const TYPESCRIPT_LANGUAGE_SERVER: &str = "8.8.8";

const TYPESCRIPT: &str = "7.7.7";

const SHAPES: &str = "class Circle {
    area(): number { return 1; }

    describe(): string { return `${this.area()}`; }
}
";

const JOBS: &str = "<?php

class Jobs
{
    public function schedule(): void {}

    public function run(): void { $this->schedule(); }
}
";

fn node_url(target: Target) -> String {
    let platform = match target.os {
        Os::MacOs => "darwin",
        Os::Linux => "linux",
    };
    let arch = match target.cpu {
        Cpu::Arm64 => "arm64",
        Cpu::X86_64 => "x64",
    };
    format!("https://nodejs.org/dist/{NODE}/node-{NODE}-{platform}-{arch}.tar.gz")
}

fn intelephense_url() -> String {
    format!("https://registry.npmjs.org/intelephense/-/intelephense-{INTELEPHENSE}.tgz")
}

fn typescript_language_server_url() -> String {
    format!(
        "https://registry.npmjs.org/typescript-language-server/-/typescript-language-server-{TYPESCRIPT_LANGUAGE_SERVER}.tgz"
    )
}

fn typescript_url() -> String {
    format!("https://registry.npmjs.org/typescript/-/typescript-{TYPESCRIPT}.tgz")
}

fn sha512_of(bytes: &[u8]) -> String {
    guided_review::hex::lower(&Sha512::digest(bytes))
}

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
        repo.write("app/Jobs.php", JOBS);
        repo.write("src/shapes.ts", SHAPES);
        repo.write("src/shapes.tsx", SHAPES);
        let head = Sha::new(&repo.commit("head"));
        let scratch = TempDir::new().unwrap();
        let script = json!({
            "log": scratch.path().join("log").to_string_lossy(),
            "reports_status": true,
            "answers": {
                "textDocument/definition": [{
                    "uri": format!("file://{}", scratch.path().join("checkouts/acme/shop/src/lib.rs").display()),
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
        let node = format!(
            "#!/bin/sh\necho \"$@\" > '{}'\nexec python3 '{}' '{}'\n",
            scratch.path().join("node-args").display(),
            fake.display(),
            script_file.display()
        );
        let node_archive = tarball(&[
            (format!("node-{NODE}-darwin-arm64/bin/node"), node, 0o755),
            (
                format!("node-{NODE}-darwin-arm64/README.md"),
                "Node".to_owned(),
                0o644,
            ),
        ]);
        let intelephense_archive = tarball(&[
            ("package/package.json", "{}", 0o644),
            ("package/lib/intelephense.js", "// the server", 0o644),
        ]);
        let typescript_language_server_archive = tarball(&[
            ("package/package.json", "{}", 0o644),
            ("package/lib/cli.mjs", "// the server", 0o644),
        ]);
        let typescript_archive = tarball(&[
            ("package/package.json", "{}", 0o644),
            ("package/lib/tsserver.js", "// tsserver", 0o644),
        ]);
        let host = Arc::new(Host::default());
        host.files.lock().unwrap().extend([
            (url_of(TAG, TARGET), archive.clone()),
            (node_url(TARGET), node_archive.clone()),
            (intelephense_url(), intelephense_archive.clone()),
            (
                typescript_language_server_url(),
                typescript_language_server_archive.clone(),
            ),
            (typescript_url(), typescript_archive.clone()),
        ]);
        let pins = ServerPins {
            rust_analyzer: ReleasePin {
                tag: TAG.to_owned(),
                sha256: vec![(TARGET, sha256_of(&archive))],
            },
            node: ReleasePin {
                tag: NODE.to_owned(),
                sha256: vec![(TARGET, sha256_of(&node_archive))],
            },
            intelephense: PackagePin {
                version: INTELEPHENSE.to_owned(),
                sha512: sha512_of(&intelephense_archive),
            },
            typescript_language_server: PackagePin {
                version: TYPESCRIPT_LANGUAGE_SERVER.to_owned(),
                sha512: sha512_of(&typescript_language_server_archive),
            },
            typescript: PackagePin {
                version: TYPESCRIPT.to_owned(),
                sha512: sha512_of(&typescript_archive),
            },
        };
        let cache = scratch.path().join("cache/language-servers");
        let managed = Arc::new(ManagedServers::new(
            ServerFolders {
                downloads: cache.clone(),
                indexes: scratch.path().join("state/server-indexes"),
            },
            Arc::clone(&host) as Arc<dyn ReleasesPort>,
            pins,
            TARGET,
        ));
        let servers = LanguageServers::new(
            Arc::clone(&managed) as Arc<dyn guided_review::navigation::ServerCatalog>,
            ServerCheckouts::new(scratch.path().join("checkouts")),
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
        self.hover("src/lib.rs", 3, 23, "add")
    }

    /// `$this->schedule()` in `run`.
    fn hover_schedule(&self) -> (anyhow::Result<Answer>, Vec<Waiting>) {
        self.hover("app/Jobs.php", 7, 41, "schedule")
    }

    /// `this.area()` in `describe`.
    fn hover_area(&self) -> (anyhow::Result<Answer>, Vec<Waiting>) {
        self.hover("src/shapes.ts", 4, 40, "area")
    }

    fn hover(
        &self,
        path: &str,
        number: usize,
        column: usize,
        word: &str,
    ) -> (anyhow::Result<Answer>, Vec<Waiting>) {
        self.hover_read_from(&self.clone_source(), path, number, column, word)
    }

    fn clone_source(&self) -> LocalGit {
        LocalGit::configured_clone(&RepoName::new("acme", "shop"), self.repo.path()).unwrap()
    }

    fn hover_read_from(
        &self,
        code: &dyn CodeSource,
        path: &str,
        number: usize,
        column: usize,
        word: &str,
    ) -> (anyhow::Result<Answer>, Vec<Waiting>) {
        let by_name: Arc<dyn CodeNavigator> =
            Arc::new(Lookups::new(FileSearch::changed_files(Arc::new([
                file_path(path),
            ]))));
        let waits = Mutex::new(Vec::new());
        let answer = self
            .servers
            .navigator(Codebase::GitHub(RepoName::new("acme", "shop")), by_name)
            .navigate(
                code,
                &NavigationRequest {
                    sha: self.head.clone(),
                    at: Place {
                        path: file_path(path),
                        line: line(number),
                    },
                    column: Column::try_from(column).unwrap(),
                    word: Word::new(word).unwrap(),
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
        self.serving_at(&url_of(TAG, TARGET), archive);
    }

    fn serving_at(&self, url: &str, archive: &[u8]) {
        let mut files = self.host.files.lock().unwrap();
        let served = files.iter_mut().find(|(served, _)| served == url).unwrap();
        served.1 = archive.to_vec();
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

/// The clone's files, read as a review without a local clone reads them from GitHub.
struct ReadFromGitHub(LocalGit);

impl CodeSource for ReadFromGitHub {
    fn file_at(&self, sha: &Sha, path: &FilePath) -> anyhow::Result<String> {
        self.0.file_at(sha, path)
    }

    fn blob_at(&self, sha: &Sha, path: &FilePath) -> anyhow::Result<Arc<[u8]>> {
        self.0.blob_at(sha, path)
    }

    fn blob_info_at(&self, sha: &Sha, path: &FilePath) -> anyhow::Result<BlobInfo> {
        self.0.blob_info_at(sha, path)
    }

    fn diff(&self, base: &Sha, head: &Sha) -> anyhow::Result<Vec<FileDiff>> {
        self.0.diff(base, head)
    }

    fn merge_base(&self, a: &Sha, b: &Sha) -> anyhow::Result<Sha> {
        self.0.merge_base(a, b)
    }

    fn working_tree_is_at(&self, _head: &Sha) -> bool {
        false
    }
}

#[test]
fn a_review_without_a_local_clone_downloads_nothing_and_is_answered_by_name_with_a_note() {
    let rig = Rig::new();

    let (answer, waits) = rig.hover_read_from(
        &ReadFromGitHub(rig.clone_source()),
        "src/lib.rs",
        3,
        23,
        "add",
    );
    let answer = answer.unwrap();

    assert_eq!(rig.requested(), Vec::<String>::new());
    assert_eq!(waits, []);
    assert_eq!(
        answer.answered_by,
        AnsweredBy::ByName {
            because: Some(FallbackReason::new(
                "rust-analyzer needs a local clone, and the code is read from GitHub"
            ))
        }
    );
}

fn intelephense() -> ServerName {
    ServerName::new("Intelephense")
}

#[test]
fn the_first_php_hover_downloads_the_pinned_node_and_intelephense_and_runs_intelephense_on_that_node()
 {
    let rig = Rig::new();

    let (answer, waits) = rig.hover_schedule();
    let answer = answer.unwrap();

    assert_eq!(
        answer.answered_by,
        AnsweredBy::LanguageServer(intelephense())
    );
    assert_eq!(
        waits[..2],
        [
            Waiting::Downloading(ServerName::new("Node")),
            Waiting::Downloading(intelephense())
        ]
    );
    assert_eq!(rig.requested(), vec![node_url(TARGET), intelephense_url()]);
    let args = std::fs::read_to_string(rig.scratch.path().join("node-args")).unwrap();
    assert_eq!(
        args.trim(),
        format!(
            "{}/intelephense-{INTELEPHENSE}/package/lib/intelephense.js --stdio",
            rig.cache.display()
        )
    );
}

#[test]
fn intelephense_keeps_its_index_in_a_folder_of_the_repo_under_the_state_folder() {
    let rig = Rig::new();

    rig.hover_schedule().0.unwrap();

    let log = std::fs::read_to_string(rig.scratch.path().join("log")).unwrap();
    let initialize: serde_json::Value = log
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .find(|message| message["method"] == "initialize")
        .unwrap();
    let index = rig
        .scratch
        .path()
        .join("state/server-indexes/acme/shop/intelephense");
    assert_eq!(
        initialize["params"]["initializationOptions"],
        json!({
            "storagePath": index.join("storage"),
            "globalStoragePath": index.join("global"),
        })
    );
}

fn typescript_language_server() -> ServerName {
    ServerName::new("typescript-language-server")
}

#[test]
fn the_first_typescript_hover_downloads_node_typescript_language_server_and_typescript_and_runs_the_server_on_that_node()
 {
    let rig = Rig::new();

    let (answer, waits) = rig.hover_area();
    let answer = answer.unwrap();

    assert_eq!(
        answer.answered_by,
        AnsweredBy::LanguageServer(typescript_language_server())
    );
    assert_eq!(
        waits[..2],
        [
            Waiting::Downloading(ServerName::new("Node")),
            Waiting::Downloading(typescript_language_server())
        ]
    );
    assert_eq!(
        rig.requested(),
        vec![
            node_url(TARGET),
            typescript_language_server_url(),
            typescript_url()
        ]
    );
    let args = std::fs::read_to_string(rig.scratch.path().join("node-args")).unwrap();
    assert_eq!(
        args.trim(),
        format!(
            "{}/typescript-language-server-{TYPESCRIPT_LANGUAGE_SERVER}/package/lib/cli.mjs --stdio",
            rig.cache.display()
        )
    );
}

#[test]
fn typescript_language_server_is_told_the_pinned_tsserver_even_when_the_repo_installs_typescript() {
    let rig = Rig::new();
    let installed = rig.repo.path().join("node_modules/typescript/lib");
    std::fs::create_dir_all(&installed).unwrap();
    std::fs::write(installed.join("tsserver.js"), "// the PR's tsserver").unwrap();

    rig.hover_area().0.unwrap();

    let log = std::fs::read_to_string(rig.scratch.path().join("log")).unwrap();
    let initialize: serde_json::Value = log
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .find(|message| message["method"] == "initialize")
        .unwrap();
    assert_eq!(
        initialize["params"]["initializationOptions"],
        json!({
            "tsserver": {"path": rig.cache.join(format!("typescript-{TYPESCRIPT}/package/lib/tsserver.js"))},
            "disableAutomaticTypingAcquisition": true,
        })
    );
}

#[test]
fn a_typescript_tarball_that_is_not_the_pinned_one_is_refused_unpacked_nowhere_and_never_run() {
    let rig = Rig::new();
    rig.serving_at(
        &typescript_url(),
        &tarball(&[("package/lib/tsserver.js", "// another tsserver", 0o644)]),
    );

    let (answer, _) = rig.hover_area();
    let answer = answer.unwrap();

    let note = note_of(&answer).expect("answered by name with a note");
    assert!(note.contains("typescript-language-server"), "{note}");
    assert!(note.contains("does not match its checksum"), "{note}");
    let mut downloads = rig.downloads();
    downloads.sort();
    assert_eq!(
        downloads,
        vec![
            format!("node-{NODE}"),
            format!("typescript-language-server-{TYPESCRIPT_LANGUAGE_SERVER}")
        ]
    );
    assert!(!rig.server_started());
}

#[test]
fn a_tsx_hover_is_answered_by_typescript_language_server() {
    let rig = Rig::new();

    let (answer, _) = rig.hover("src/shapes.tsx", 4, 40, "area");

    assert_eq!(
        answer.unwrap().answered_by,
        AnsweredBy::LanguageServer(typescript_language_server())
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
fn an_intelephense_tarball_that_is_not_the_pinned_one_is_refused_unpacked_nowhere_and_never_run() {
    let rig = Rig::new();
    rig.serving_at(
        &intelephense_url(),
        &tarball(&[("package/lib/intelephense.js", "// another server", 0o644)]),
    );

    let (answer, _) = rig.hover_schedule();
    let answer = answer.unwrap();

    let note = note_of(&answer).expect("answered by name with a note");
    assert!(note.contains("Intelephense"), "{note}");
    assert!(note.contains("does not match its checksum"), "{note}");
    assert_eq!(rig.downloads(), vec![format!("node-{NODE}")]);
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
fn removing_unpinned_downloads_keeps_the_pinned_node_and_intelephense() {
    let rig = Rig::new();
    rig.hover_schedule().0.unwrap();
    std::fs::create_dir_all(rig.cache.join("node-v1.0.0")).unwrap();
    std::fs::create_dir_all(rig.cache.join("intelephense-0.0.1")).unwrap();

    rig.managed.remove_unpinned().unwrap();

    let mut downloads = rig.downloads();
    downloads.sort();
    assert_eq!(
        downloads,
        vec![
            format!("intelephense-{INTELEPHENSE}"),
            format!("node-{NODE}")
        ]
    );
}

#[test]
fn removing_unpinned_downloads_keeps_the_pinned_typescript_language_server_and_typescript() {
    let rig = Rig::new();
    rig.hover_area().0.unwrap();
    std::fs::create_dir_all(rig.cache.join("typescript-language-server-0.0.1")).unwrap();
    std::fs::create_dir_all(rig.cache.join("typescript-0.0.1")).unwrap();

    rig.managed.remove_unpinned().unwrap();

    let mut downloads = rig.downloads();
    downloads.sort();
    assert_eq!(
        downloads,
        vec![
            format!("node-{NODE}"),
            format!("typescript-{TYPESCRIPT}"),
            format!("typescript-language-server-{TYPESCRIPT_LANGUAGE_SERVER}"),
        ]
    );
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
