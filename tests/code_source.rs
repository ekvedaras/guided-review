mod support;

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::thread;

use support::{FakeGitHub, FixtureRepo, GuidedReview};
use tempfile::TempDir;

const PR: &str = "https://github.com/acme/shop/pull/7";

const ORDER_V1: &str = "<?php

class Order
{
    public function total(): int
    {
        return 1;
    }
}
";

const ORDER_V2: &str = "<?php

class Order
{
    public function total(): int
    {
        return 2;
    }
}
";

const ORDER_OUTLINE: &str = "\
pr: https://github.com/acme/shop/pull/7
base: <base>
head: <head>
piece id: <file>#<symbol>, or <file> for a whole-file piece
1 file, 1 piece, +1 −1

app/Order.php  modified  +1 −1
  #Order::total  method  modified  +1 −1  public function total(): int
";

#[test]
fn commits_missing_from_the_clone_are_fetched_without_touching_branches_or_files() {
    let author = FixtureRepo::cloned_from("acme/shop");
    author.write("app/Order.php", ORDER_V1);
    let base = author.commit("base");
    author.write("app/Order.php", ORDER_V2);
    let head = author.commit("head");
    author.branch_from("main", &base);
    author.write("README.md", "newer base branch tip");
    let base_branch_tip = author.commit("base branch moved on");
    let hosting = TempDir::new().unwrap();
    let remote = hosting.path().join("github.com/acme/shop.git");
    author.git(&["clone", "-q", "--bare", ".", remote.to_str().unwrap()]);
    let remote_git =
        |args: &[&str]| author.git(&[&["-C", remote.to_str().unwrap()], args].concat());
    remote_git(&["update-ref", "refs/pull/7/head", &head]);

    let clone = FixtureRepo::cloned_from("acme/shop");
    clone.git(&["remote", "set-url", "origin", remote.to_str().unwrap()]);
    let github = FakeGitHub::serving_pull("acme/shop", 7, &base_branch_tip, &head);

    let run = GuidedReview::new(&clone, &github).run(&["outline", PR]);

    assert_eq!(
        run.stdout_with(&[(&base, "<base>"), (&head, "<head>")]),
        ORDER_OUTLINE
    );
    assert_eq!(
        clone.git(&["for-each-ref", "--format=%(refname) %(objectname)"]),
        format!("refs/guided-review/7 {head}")
    );
    assert_eq!(clone.git(&["branch", "--list"]), "");
    assert_eq!(clone.git(&["status", "--porcelain"]), "");
    assert!(!clone.path().join("app").exists());
}

#[test]
fn without_a_clone_the_tour_reads_code_from_github() {
    let author = FixtureRepo::cloned_from("acme/shop");
    author.write("app/Order.php", ORDER_V1);
    let base = author.commit("base");
    author.write("app/Order.php", ORDER_V2);
    let head = author.commit("head");
    let other_repo = FixtureRepo::cloned_from("acme/blog");
    let github = FakeCodeHost::new("acme/shop", 7)
        .pull(&base, &head)
        .compare_diff(&base, &head, &author.git(&["diff", "-M", &base, &head]))
        .merge_base(&base, &head, &base)
        .file(&base, "app/Order.php", ORDER_V1)
        .file(&head, "app/Order.php", ORDER_V2)
        .serve();

    let run = GuidedReview::new(&author, &github)
        .in_dir(other_repo.path())
        .run(&["outline", PR]);

    assert_eq!(
        run.stdout_with(&[(&base, "<base>"), (&head, "<head>")]),
        ORDER_OUTLINE
    );
}

/// Serves only what the GitHub adapter asks for, and only with the test token.
struct FakeCodeHost {
    repo: String,
    number: u32,
    routes: HashMap<(String, bool), String>,
}

impl FakeCodeHost {
    fn new(repo: &str, number: u32) -> Self {
        Self {
            repo: repo.to_string(),
            number,
            routes: HashMap::new(),
        }
    }

    fn route(mut self, path: String, as_diff: bool, body: &str) -> Self {
        self.routes.insert((path, as_diff), body.to_string());
        self
    }

    fn pull(self, base: &str, head: &str) -> Self {
        let path = format!("/repos/{}/pulls/{}", self.repo, self.number);
        let body = format!(r#"{{"base":{{"sha":"{base}"}},"head":{{"sha":"{head}"}}}}"#);
        self.route(path, false, &body)
    }

    fn compare_diff(self, base: &str, head: &str, diff: &str) -> Self {
        let path = format!("/repos/{}/compare/{base}...{head}", self.repo);
        self.route(path, true, diff)
    }

    fn merge_base(self, a: &str, b: &str, merge_base: &str) -> Self {
        let path = format!("/repos/{}/compare/{a}...{b}", self.repo);
        let body = format!(r#"{{"merge_base_commit":{{"sha":"{merge_base}"}}}}"#);
        self.route(path, false, &body)
    }

    fn file(self, sha: &str, path: &str, content: &str) -> Self {
        let path = format!("/repos/{}/contents/{path}?ref={sha}", self.repo);
        self.route(path, false, content)
    }

    fn serve(self) -> FakeGitHub {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        thread::spawn(move || {
            for stream in listener.incoming() {
                let mut stream = stream.unwrap();
                let mut request_line = String::new();
                let mut reader = BufReader::new(&stream);
                reader.read_line(&mut request_line).unwrap();
                let (mut as_diff, mut authorized) = (false, false);
                loop {
                    let mut header = String::new();
                    reader.read_line(&mut header).unwrap();
                    let header = header.trim().to_lowercase();
                    if header.is_empty() {
                        break;
                    }
                    as_diff |= header.starts_with("accept:") && header.contains("diff");
                    authorized |= header == "authorization: bearer test-token";
                }
                let path = request_line.split_whitespace().nth(1).unwrap_or_default();
                let route = self.routes.get(&(path.to_string(), as_diff));
                let (status, body) = match route {
                    Some(body) if authorized => ("200 OK", body.as_str()),
                    Some(_) => ("401 Unauthorized", "{}"),
                    None => ("404 Not Found", r#"{"message":"Not Found"}"#),
                };
                write!(
                    stream,
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .unwrap();
            }
        });
        FakeGitHub { url }
    }
}
