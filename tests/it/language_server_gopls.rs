//! The real gopls, built by the app, against Go modules with files that would make it or its `go`
//! command run a program. Each test skips when the pinned gopls is not downloaded.

use std::path::Path;
use std::sync::Arc;

use guided_review::codebase::Codebase;
use guided_review::language::Language;
use guided_review::local_git::LocalGit;
use guided_review::navigation::{
    Answer, AnsweredBy, CodeNavigator, Column, FileSearch, LanguageServers, Lookups,
    NavigationRequest, Place, ServerCheckouts, ServerName, Timeouts, Word,
};
use guided_review::pull_request::Sha;
use tempfile::TempDir;

use crate::language_server::downloaded_server_indexing_in;
use crate::support::fake_ports::IgnoredServerMessages;
use crate::support::{FixtureRepo, file_path, line, write_executable};

fn shop_codebase() -> Codebase {
    Codebase::Hosted(crate::support::shop_on_github())
}

const BASKET: &str = "package shop

// Basket holds items.
type Basket struct{}

// Total adds up the basket.
func (b Basket) Total() int { return 3 }

func (b Basket) Summary() int { return b.Total() }

type Order struct{}

func (o Order) Total() int { return 4 }
";

/// Hovers `Total` in `b.Total()` with the real gopls in a Go module whose head also holds what
/// `plant` writes. `None` when the pinned gopls is not downloaded.
fn the_real_gopls_hovering_total_with(plant: impl FnOnce(&FixtureRepo)) -> Option<Answer> {
    let scratch = TempDir::new().unwrap();
    let managed = downloaded_server_indexing_in(Language::Go, scratch.path().join("indexes"))?;
    let repo = FixtureRepo::cloned_from("acme/shop");
    repo.write("go.mod", "module example.com/shop\n\ngo 1.27\n");
    repo.write("basket.go", BASKET);
    plant(&repo);
    let head = Sha::new(&repo.commit("head"));
    let code = LocalGit::configured_clone(&crate::support::shop_on_github(), repo.path()).unwrap();
    let servers = LanguageServers::new(
        managed,
        ServerCheckouts::new(scratch.path().join("checkouts")),
        Timeouts::default(),
        Arc::new(IgnoredServerMessages),
    );
    let by_name: Arc<dyn CodeNavigator> =
        Arc::new(Lookups::new(FileSearch::changed_files(Arc::new([
            file_path("basket.go"),
        ]))));

    let answer = servers
        .navigator(shop_codebase(), by_name)
        .navigate(
            &code,
            &NavigationRequest {
                sha: head,
                at: Place {
                    path: file_path("basket.go"),
                    line: line(9),
                },
                column: Column::try_from(41).unwrap(),
                word: Word::new("Total").unwrap(),
            },
            &|_| {},
        )
        .unwrap();
    servers.stop_all();

    Some(answer)
}

#[test]
fn the_real_gopls_finds_the_method_of_the_type_the_code_calls() {
    let Some(answer) = the_real_gopls_hovering_total_with(|_| {}) else {
        eprintln!("the pinned gopls is not downloaded: skipped");
        return;
    };

    assert_eq!(
        answer.answered_by,
        AnsweredBy::LanguageServer(ServerName::gopls())
    );
    let [definition] = &answer.lookup.definitions[..] else {
        panic!("one definition: {:?}", answer.lookup.definitions);
    };
    assert_eq!(definition.place.path, file_path("basket.go"));
    assert_eq!(definition.place.line, line(7));
    assert_eq!(&*definition.signature, "func (b Basket) Total() int");
    assert_eq!(definition.doc.as_deref(), Some("Total adds up the basket."));
}

/// The file the PR's `plant` makes, for the test after it.
fn plant_for_gopls(plant: &str, repo: &FixtureRepo) {
    match plant {
        "go.mod" => repo.write(
            "go.mod",
            "module example.com/shop\n\ngo 1.27\n\ntoolchain go1.99.0\n",
        ),
        "go.work" => repo.write("go.work", "go 1.27\n\ntoolchain go1.99.0\n\nuse .\n"),
        "cgo" => repo.write(
            "answer.go",
            "package shop\n\n// #cgo CFLAGS: -DANSWER=42\n// int answer(void) { return ANSWER; }\nimport \"C\"\n\nfunc Answer() int { return int(C.answer()) }\n",
        ),
        _ => {}
    }
}

/// Run in a process of its own by `the_real_gopls_in_a_process_with`, which gives it an environment that
/// offers programs, the file to plant and the marker.
#[test]
#[ignore = "run by the real gopls tests that give it an environment"]
fn the_real_gopls_started_by_a_process_with_the_environment_it_was_given() {
    let plant = std::env::var("GUIDED_REVIEW_GOPLS_PLANT").unwrap();
    let Some(answer) = the_real_gopls_hovering_total_with(|repo| plant_for_gopls(&plant, repo))
    else {
        eprintln!("the pinned gopls is not downloaded: skipped");
        return;
    };
    assert_eq!(
        answer.answered_by,
        AnsweredBy::LanguageServer(ServerName::gopls())
    );
}

/// Runs the test above in a child process whose environment also holds what `environment` gives for a
/// scratch folder and the marker. Says whether anything wrote the marker; `None` when the pinned gopls
/// is not downloaded.
fn the_real_gopls_in_a_process_with(
    plant: &str,
    environment: impl FnOnce(&Path, &Path) -> Vec<(&'static str, String)>,
) -> Option<bool> {
    let scratch = TempDir::new().unwrap();
    downloaded_server_indexing_in(Language::Go, scratch.path().join("indexes"))?;
    let marker = scratch.path().join("ran");

    let run = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "language_server_gopls::the_real_gopls_started_by_a_process_with_the_environment_it_was_given",
            "--ignored",
        ])
        .env("GUIDED_REVIEW_GOPLS_PLANT", plant)
        .envs(environment(scratch.path(), &marker))
        .output()
        .unwrap();

    let ran = marker.exists();
    if !ran {
        let said = String::from_utf8_lossy(&run.stdout);
        assert!(run.status.success(), "{said}");
        assert!(said.contains("1 passed"), "{said}");
    }
    Some(ran)
}

/// A program that writes the marker, named by full path since the app gives Go a PATH of its own.
fn marking_by_full_path(marker: &Path) -> String {
    format!("#!/bin/sh\n/usr/bin/touch '{}'\nexit 1\n", marker.display())
}

/// A module proxy in `folder` serving Go 1.99.0 for this machine, whose `go` writes the marker: what
/// the go command downloads and runs when a `toolchain` line names a newer Go than its own.
fn toolchain_proxy(folder: &Path, marker: &Path) -> Vec<(&'static str, String)> {
    let os = match std::env::consts::OS {
        "macos" => "darwin",
        other => other,
    };
    let arch = match std::env::consts::ARCH {
        "aarch64" => "arm64",
        "x86_64" => "amd64",
        other => other,
    };
    let version = format!("v0.0.1-go1.99.0.{os}-{arch}");
    let versions = folder.join("proxy/golang.org/toolchain/@v");
    std::fs::create_dir_all(&versions).unwrap();
    std::fs::write(versions.join("list"), format!("{version}\n")).unwrap();
    std::fs::write(
        versions.join(format!("{version}.info")),
        format!("{{\"Version\": \"{version}\", \"Time\": \"2030-01-01T00:00:00Z\"}}"),
    )
    .unwrap();
    std::fs::write(
        versions.join(format!("{version}.mod")),
        "module golang.org/toolchain\n",
    )
    .unwrap();
    let zipped = std::process::Command::new("python3")
        .args([
            "-I",
            "-c",
            "import sys, zipfile
out, top, go = sys.argv[1:4]
with zipfile.ZipFile(out, 'w') as z:
    for name, text in [('bin/go', go), ('bin/gofmt', '#!/bin/sh\\n')]:
        entry = zipfile.ZipInfo(top + '/' + name)
        entry.external_attr = 0o755 << 16
        z.writestr(entry, text)
    for name in ['go.mod', 'lib/README', 'pkg/tool/README']:
        z.writestr(top + '/' + name, 'module golang.org/toolchain\\n')
",
        ])
        .arg(versions.join(format!("{version}.zip")))
        .arg(format!("golang.org/toolchain@{version}"))
        .arg(marking_by_full_path(marker))
        .status()
        .unwrap();
    assert!(zipped.success());
    vec![
        ("GOTOOLCHAIN", "auto".to_owned()),
        (
            "GOPROXY",
            format!("file://{}", folder.join("proxy").display()),
        ),
        ("GOSUMDB", "off".to_owned()),
    ]
}

fn assert_gopls_ran_nothing(ran: Option<bool>) {
    match ran {
        None => eprintln!("the pinned gopls is not downloaded: skipped"),
        Some(ran) => assert!(!ran, "gopls ran a program the PR or the environment named"),
    }
}

#[test]
fn the_real_gopls_runs_no_toolchain_that_the_prs_go_mod_names() {
    assert_gopls_ran_nothing(the_real_gopls_in_a_process_with("go.mod", toolchain_proxy));
}

#[test]
fn the_real_gopls_runs_no_toolchain_that_the_prs_go_work_names() {
    assert_gopls_ran_nothing(the_real_gopls_in_a_process_with("go.work", toolchain_proxy));
}

#[test]
fn the_real_gopls_runs_no_c_compiler_on_the_prs_cgo_files() {
    assert_gopls_ran_nothing(the_real_gopls_in_a_process_with("cgo", |folder, marker| {
        let compiler = folder.join("cc");
        write_executable(&compiler, marking_by_full_path(marker));
        vec![
            ("CC", compiler.to_string_lossy().into_owned()),
            ("CGO_ENABLED", "1".to_owned()),
        ]
    }));
}

#[test]
fn the_real_gopls_runs_no_packages_driver_that_the_environment_names() {
    assert_gopls_ran_nothing(the_real_gopls_in_a_process_with(
        "nothing",
        |folder, marker| {
            let driver = folder.join("gopackagesdriver");
            write_executable(&driver, marking_by_full_path(marker));
            vec![("GOPACKAGESDRIVER", driver.to_string_lossy().into_owned())]
        },
    ));
}
