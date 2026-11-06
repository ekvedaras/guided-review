#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "a panic in a test helper is the test failing"
)]
#![allow(
    clippy::needless_pass_by_value,
    reason = "test helpers take owned arguments so a case reads as a plain call"
)]

pub mod app_path;
pub mod fake_ports;
pub mod fake_tour_agent;
pub mod graphql;

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;

use assert_cmd::cargo::cargo_bin_cmd;
use guided_review::ide::NoIde;
use guided_review::viewer::{CodeLink, Destination, Review};
use tempfile::TempDir;

pub fn file_path(path: &str) -> guided_review::file_path::FilePath {
    guided_review::file_path::FilePath::new(path).unwrap()
}

pub fn line(number: usize) -> guided_review::line_number::LineNumber {
    guided_review::line_number::LineNumber::try_from(number).unwrap()
}

pub struct FixtureRepo {
    root: TempDir,
    home: TempDir,
}

impl FixtureRepo {
    pub fn cloned_from(github_repo: &str) -> Self {
        let repo = Self {
            root: TempDir::new().unwrap(),
            home: TempDir::new().unwrap(),
        };
        repo.git(&["init", "-q", "-b", "main"]);
        repo.git(&[
            "remote",
            "add",
            "origin",
            &format!("git@github.com:{github_repo}.git"),
        ]);
        repo
    }

    pub fn path(&self) -> &Path {
        self.root.path()
    }

    pub fn home(&self) -> &Path {
        self.home.path()
    }

    pub fn write(&self, path: &str, content: &str) {
        let file = self.root.path().join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, content).unwrap();
    }

    pub fn write_bytes(&self, path: &str, content: &[u8]) {
        let file = self.root.path().join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, content).unwrap();
    }

    pub fn commit(&self, message: &str) -> String {
        self.git(&["add", "-A"]);
        self.git(&["commit", "-q", "--allow-empty", "-m", message]);
        self.git(&["rev-parse", "HEAD"])
    }

    pub fn branch_from(&self, branch: &str, sha: &str) {
        self.git(&["checkout", "-q", "-B", branch, sha]);
    }

    pub fn git(&self, args: &[&str]) -> String {
        let output = Command::new("git")
            .args(args)
            .current_dir(self.root.path())
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "Fixture")
            .env("GIT_AUTHOR_EMAIL", "fixture@example.com")
            .env("GIT_COMMITTER_NAME", "Fixture")
            .env("GIT_COMMITTER_EMAIL", "fixture@example.com")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    }
}

pub struct GuidedReview<'a> {
    repo: &'a FixtureRepo,
    github: &'a FakeGitHub,
    current_dir: PathBuf,
    env: Vec<(String, PathBuf)>,
}

impl<'a> GuidedReview<'a> {
    pub fn new(repo: &'a FixtureRepo, github: &'a FakeGitHub) -> Self {
        Self {
            repo,
            github,
            current_dir: repo.path().to_path_buf(),
            env: Vec::new(),
        }
    }

    pub fn with_env(mut self, name: &str, value: &Path) -> Self {
        self.env.push((name.to_owned(), value.to_path_buf()));
        self
    }

    pub fn in_dir(mut self, dir: &Path) -> Self {
        self.current_dir = dir.to_path_buf();
        self
    }

    pub fn run(&self, args: &[&str]) -> Run {
        let empty_gh_config = self.repo.home().join(".config/gh");
        std::fs::create_dir_all(&empty_gh_config).unwrap();
        let mut command = cargo_bin_cmd!("guided-review");
        command.envs(self.env.iter().map(|(name, value)| (name, value)));
        let output = command
            .args(args)
            .current_dir(&self.current_dir)
            .env("HOME", self.repo.home())
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GH_CONFIG_DIR", empty_gh_config)
            .env_remove("GH_TOKEN")
            .env("GITHUB_TOKEN", "test-token")
            .env("GUIDED_REVIEW_GITHUB_API", &self.github.url)
            .output()
            .unwrap();
        Run {
            success: output.status.success(),
            stdout: String::from_utf8(output.stdout).unwrap(),
            stderr: String::from_utf8(output.stderr).unwrap(),
        }
    }
}

pub struct Run {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

impl Run {
    pub fn stdout_with(&self, shas: &[(&str, &str)]) -> String {
        assert!(self.success, "guided-review failed:\n{}", self.stderr);
        shas.iter()
            .fold(self.stdout.clone(), |out, (sha, placeholder)| {
                out.replace(sha, placeholder)
            })
    }
}

pub struct FakeGitHub {
    pub url: String,
}

impl FakeGitHub {
    pub fn serving_pull(github_repo: &str, number: u32, base_sha: &str, head_sha: &str) -> Self {
        Self::serving_pull_described(github_repo, number, base_sha, head_sha, "null")
    }

    /// `body_json` is the JSON value of the PR's `body`: a quoted string, or `null`.
    pub fn serving_pull_described(
        github_repo: &str,
        number: u32,
        base_sha: &str,
        head_sha: &str,
        body_json: &str,
    ) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let pull_path = format!("/repos/{github_repo}/pulls/{number}");
        let pull_json = format!(
            r#"{{"base":{{"sha":"{base_sha}"}},"head":{{"sha":"{head_sha}"}},"body":{body_json}}}"#
        );

        thread::spawn(move || {
            for stream in listener.incoming() {
                let mut stream = stream.unwrap();
                let mut request_line = String::new();
                let mut reader = BufReader::new(&stream);
                reader.read_line(&mut request_line).unwrap();
                loop {
                    let mut header = String::new();
                    reader.read_line(&mut header).unwrap();
                    if header.trim().is_empty() {
                        break;
                    }
                }
                let requested_path = request_line.split_whitespace().nth(1).unwrap_or_default();
                let (status, body) = if requested_path == pull_path {
                    ("200 OK", pull_json.as_str())
                } else {
                    ("404 Not Found", r#"{"message":"Not Found"}"#)
                };
                write!(
                    stream,
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .unwrap();
            }
        });

        Self { url }
    }
}

/// A real PNG of one flat colour, so the code under test can read its size.
pub fn png_of(width: u32, height: u32, rgba: [u8; 4]) -> Vec<u8> {
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(width, height, image::Rgba(rgba))
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    bytes.into_inner()
}

pub fn svg_of(width: u32, height: u32) -> String {
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\">\n<rect width=\"{width}\" height=\"{height}\" fill=\"red\"/>\n</svg>\n"
    )
}

/// Where `link` opens on GitHub, for a review with no IDE.
pub fn on_github(review: &Review, link: &CodeLink) -> String {
    match review.code_opener(Err(NoIde::NoTemplate)).destination(link) {
        Destination::GitHub { url, .. } => url,
        other => panic!("code opens on GitHub without an IDE, not {other:?}"),
    }
}
