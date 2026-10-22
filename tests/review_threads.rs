#[allow(dead_code)]
mod support;

use guided_review::local_git::LocalGit;
use guided_review::pull_request::{PullRequestUrl, RepoName};
use guided_review::tour::Tour;
use guided_review::viewer::{
    Review, ReviewStateFile, ReviewThread, Screen, Side, ThreadComment, ThreadView,
};
use serde_json::{Value, json};
use support::FixtureRepo;
use support::fake_ports::{FakeGitHub, SentLog};

struct Shop {
    repo: FixtureRepo,
    base: String,
    head: String,
}

impl Shop {
    fn with_pull() -> Self {
        let repo = FixtureRepo::cloned_from("acme/shop");
        repo.write(
            "app/Order.php",
            "<?php\n\nclass Order\n{\n    public function total(): int\n    {\n        return 1;\n    }\n}\n",
        );
        repo.write("config.yml", "a: 1\nb: 2\nc: 3\n");
        let base = repo.commit("base");
        repo.write(
            "app/Order.php",
            "<?php\n\nclass Order\n{\n    public function total(): int\n    {\n        return 2;\n    }\n}\n",
        );
        repo.write("config.yml", "a: 1\nb: 2\nc: three\n");
        let head = repo.commit("head");
        Self { repo, base, head }
    }

    fn open(&self, steps: Vec<Value>) -> Review {
        let tour: Tour = serde_json::from_value(json!({
            "pr": "https://github.com/acme/shop/pull/7",
            "base": self.base,
            "head": self.head,
            "title": "Orders cost two",
            "summary": ["Order total goes up"],
            "steps": steps,
        }))
        .unwrap();
        let source = LocalGit::find(&RepoName::new("acme", "shop"), self.repo.path())
            .unwrap()
            .unwrap();
        let state_file = ReviewStateFile::for_pr(
            self.repo.home(),
            &"https://github.com/acme/shop/pull/7"
                .parse::<PullRequestUrl>()
                .unwrap(),
        );
        Review::open(tour, &source, state_file).unwrap()
    }
}

fn step(id: &str, pieces: Value) -> Value {
    json!({"id": id, "title": format!("Title of {id}"), "why": "because", "pieces": pieces})
}

fn comment(author: &str, is_bot: bool, body: &str) -> ThreadComment {
    ThreadComment {
        author: author.into(),
        is_bot,
        body: body.into(),
        url: format!("https://github.com/acme/shop/pull/7#discussion_{body}"),
    }
}

fn thread(id: &str, path: &str, side: Side, line: usize) -> ReviewThread {
    ReviewThread {
        id: id.into(),
        is_resolved: false,
        is_outdated: false,
        path: path.into(),
        line: Some(line),
        start_line: None,
        side,
        is_file_comment: false,
        original_line: Some(line),
        original_start_line: None,
        original_commit: Some("abc".into()),
        comments: vec![comment("alice", false, "why two?")],
    }
}

fn outdated(id: &str, path: &str) -> ReviewThread {
    ReviewThread {
        is_outdated: true,
        line: None,
        ..thread(id, path, Side::Head, 4)
    }
}

fn ids(threads: &[ThreadView]) -> Vec<&str> {
    threads.iter().map(|view| view.thread.id.as_str()).collect()
}

fn steps() -> Vec<Value> {
    vec![
        step("order", json!([{"file": "app/Order.php"}])),
        step("config", json!([{"file": "config.yml"}])),
    ]
}

#[test]
fn a_thread_hangs_under_its_line_in_the_step_that_holds_the_line() {
    let mut review = Shop::with_pull().open(steps());
    review.set_threads(&[thread("t1", "app/Order.php", Side::Head, 7)]);

    review.go_to(&"order".into());
    let page = review.current_step().unwrap();
    assert_eq!(ids(&page.threads), ["t1"]);
    let anchor = page.threads[0].anchor.as_ref().unwrap();
    assert_eq!((anchor.side, anchor.line), (Side::Head, 7));

    review.go_to(&"config".into());
    assert!(review.current_step().unwrap().threads.is_empty());
}

#[test]
fn a_thread_on_a_removed_line_hangs_on_the_base_side() {
    let mut review = Shop::with_pull().open(steps());
    review.set_threads(&[thread("t1", "app/Order.php", Side::Base, 7)]);

    review.go_to(&"order".into());
    assert_eq!(ids(&review.current_step().unwrap().threads), ["t1"]);
}

#[test]
fn the_sidebar_counts_the_threads_of_each_step() {
    let mut review = Shop::with_pull().open(steps());
    review.set_threads(&[
        thread("t1", "app/Order.php", Side::Head, 7),
        thread("t2", "app/Order.php", Side::Base, 7),
        thread("t3", "config.yml", Side::Head, 3),
    ]);

    let counts: Vec<_> = review
        .sidebar()
        .iter()
        .map(|entry| entry.thread_count)
        .collect();
    assert_eq!(counts, [2, 1]);
}

#[test]
fn a_thread_on_a_line_no_step_shows_goes_to_the_safety_net_even_when_every_change_is_in_a_step() {
    let mut review = Shop::with_pull().open(steps());
    assert!(review.safety_net().is_none());

    review.set_threads(&[thread("t1", "app/Order.php", Side::Head, 3)]);

    let net = review.safety_net().unwrap();
    assert_eq!(ids(&net.threads), ["t1"]);
    assert!(net.threads[0].anchor.is_none());
    review.go_to(&"config".into());
    assert!(review.go_forward());
    assert_eq!(review.screen(), &Screen::SafetyNet);
}

#[test]
fn a_thread_on_a_line_only_the_safety_net_shows_hangs_under_that_line_there() {
    let mut review =
        Shop::with_pull().open(vec![step("order", json!([{"file": "app/Order.php"}]))]);
    review.set_threads(&[thread("t1", "config.yml", Side::Head, 3)]);

    let net = review.safety_net().unwrap();
    assert_eq!(ids(&net.threads), ["t1"]);
    assert_eq!(net.threads[0].anchor.as_ref().unwrap().line, 3);
    review.go_to(&"order".into());
    assert!(review.current_step().unwrap().threads.is_empty());
}

#[test]
fn an_outdated_thread_is_folded_at_the_end_of_the_step_holding_its_file() {
    let mut review = Shop::with_pull().open(steps());
    review.set_threads(&[outdated("old", "config.yml")]);

    review.go_to(&"config".into());
    let page = review.current_step().unwrap();
    assert_eq!(ids(&page.threads), ["old"]);
    assert!(page.threads[0].anchor.is_none());
    assert!(page.threads[0].is_folded);
}

#[test]
fn an_outdated_thread_on_a_file_no_step_holds_goes_to_the_safety_net() {
    let mut review =
        Shop::with_pull().open(vec![step("order", json!([{"file": "app/Order.php"}]))]);
    review.set_threads(&[outdated("old", "config.yml")]);

    assert_eq!(ids(&review.safety_net().unwrap().threads), ["old"]);
}

#[test]
fn resolved_and_bot_threads_start_folded_and_a_toggle_flips_them() {
    let mut review = Shop::with_pull().open(steps());
    let resolved = ReviewThread {
        is_resolved: true,
        ..thread("done", "app/Order.php", Side::Head, 7)
    };
    let by_bot = ReviewThread {
        comments: vec![comment("copilot", true, "nit")],
        ..thread("bot", "app/Order.php", Side::Base, 7)
    };
    review.set_threads(&[
        resolved,
        by_bot,
        thread("open", "config.yml", Side::Head, 3),
    ]);

    review.go_to(&"order".into());
    let folded = |review: &Review| {
        review
            .current_step()
            .unwrap()
            .threads
            .iter()
            .map(|view| view.is_folded)
            .collect::<Vec<_>>()
    };
    assert_eq!(folded(&review), [true, true]);
    review.toggle_thread_fold("done");
    assert_eq!(folded(&review), [false, true]);
    review.toggle_thread_fold("done");
    assert_eq!(folded(&review), [true, true]);

    review.go_to(&"config".into());
    assert_eq!(folded(&review), [false]);
}

#[test]
fn threads_are_pulled_from_github_for_the_tours_pr() {
    let shop = Shop::with_pull();
    let mut review = shop.open(steps());
    let sent = SentLog::default();
    let mut github = FakeGitHub::with_pull("b", "h", &sent);
    github.threads = vec![thread("t1", "app/Order.php", Side::Head, 7)];

    review.pull_threads(&github).unwrap();

    assert_eq!(
        sent.entries(),
        ["github: threads of https://github.com/acme/shop/pull/7"]
    );
    review.go_to(&"order".into());
    assert_eq!(ids(&review.current_step().unwrap().threads), ["t1"]);
}
