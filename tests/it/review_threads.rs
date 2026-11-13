use crate::support::app_path::AppPath;
use crate::support::fake_ports::{FakeGitHub, GitHubCall, SentLog};
use crate::support::line;
use crate::support::{FixtureRepo, file_path};
use guided_review::local_git::LocalGit;
use guided_review::pull_request::{PullRequestUrl, RepoName};
use guided_review::tour::Tour;
use guided_review::viewer::{
    CommentId, Review, ReviewStateFile, ReviewThread, SafetyNetSize, Screen, Side, ThreadComment,
    ThreadId, ThreadView,
};
use serde_json::{Value, json};

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

    fn source(&self) -> LocalGit {
        LocalGit::find(&RepoName::new("acme", "shop"), self.repo.path())
            .unwrap()
            .unwrap()
    }

    /// Threads arrive before the files at head are read, as in the app.
    fn open_with_threads_then_head_files(&self, threads: &[ReviewThread]) -> Review {
        let mut review = self.open(list_step());
        review.set_threads(threads);
        let files_at_head = review.files_at_head_request().read(&self.source());
        review.set_files_at_head(files_at_head);
        review.go_to(&"list".into());
        review
    }

    fn with_list(base_lines: &[&str], head_lines: &[&str]) -> Self {
        let repo = FixtureRepo::cloned_from("acme/shop");
        repo.write("list.txt", &format!("{}\n", base_lines.join("\n")));
        let base = repo.commit("base");
        repo.write("list.txt", &format!("{}\n", head_lines.join("\n")));
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
        let source = self.source();
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
        avatar_url: None,
        id: CommentId::new(&format!("C_{body}")),
        reactions: Vec::new(),
        author: author.into(),
        is_bot,
        by_viewer: false,
        body: body.into(),
        url: format!("https://github.com/acme/shop/pull/7#discussion_{body}"),
    }
}

fn thread(id: &str, path: &str, side: Side, number: usize) -> ReviewThread {
    let line = line(number);
    ReviewThread {
        id: ThreadId::new(id),
        is_resolved: false,
        is_outdated: false,
        path: file_path(path),
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
    assert_eq!((anchor.side, anchor.line), (Side::Head, line(7)));

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
fn the_safety_net_size_counts_what_its_page_shows() {
    let mut review =
        Shop::with_pull().open(vec![step("order", json!([{"file": "app/Order.php"}]))]);
    review.set_threads(&[thread("t1", "config.yml", Side::Head, 3)]);

    let net = review.safety_net().unwrap();
    assert_eq!(
        review.safety_net_size(),
        Some(SafetyNetSize {
            pieces: net.pieces.len(),
            threads: 1,
        })
    );
}

#[test]
fn there_is_no_safety_net_size_without_a_safety_net() {
    let review = Shop::with_pull().open(steps());

    assert_eq!(review.safety_net_size(), None);
}

#[test]
fn a_thread_on_a_line_only_the_safety_net_shows_hangs_under_that_line_there() {
    let mut review =
        Shop::with_pull().open(vec![step("order", json!([{"file": "app/Order.php"}]))]);
    review.set_threads(&[thread("t1", "config.yml", Side::Head, 3)]);

    let net = review.safety_net().unwrap();
    assert_eq!(ids(&net.threads), ["t1"]);
    assert_eq!(net.threads[0].anchor.as_ref().unwrap().line, line(3));
    review.go_to(&"order".into());
    assert!(review.current_step().unwrap().threads.is_empty());
}

#[test]
fn an_outdated_thread_stays_open_at_the_end_of_the_step_holding_its_file() {
    let mut review = Shop::with_pull().open(steps());
    review.set_threads(&[outdated("old", "config.yml")]);

    review.go_to(&"config".into());
    let page = review.current_step().unwrap();
    assert_eq!(ids(&page.threads), ["old"]);
    assert!(page.threads[0].anchor.is_none());
    assert!(!page.threads[0].is_folded, "only resolving folds a thread");
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
    review.toggle_thread_fold(&ThreadId::new("done"));
    assert_eq!(folded(&review), [false, true]);
    review.toggle_thread_fold(&ThreadId::new("done"));
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
        sent.github_calls(),
        [GitHubCall::ThreadsOf(
            "https://github.com/acme/shop/pull/7".parse().unwrap()
        )]
    );
    review.go_to(&"order".into());
    assert_eq!(ids(&review.current_step().unwrap().threads), ["t1"]);
}

fn list_step() -> Vec<Value> {
    vec![step("list", json!([{"file": "list.txt"}]))]
}

fn hangs_in_step(shop: &Shop, thread: ReviewThread) -> Option<(Side, u32)> {
    let review = shop.open_with_threads_then_head_files(&[thread]);
    let page = review.current_step().unwrap();
    page.threads
        .first()
        .and_then(|view| view.anchor.as_ref())
        .map(|anchor| (anchor.side, anchor.line.get()))
}

fn is_in_safety_net_only(shop: &Shop, thread: ReviewThread) -> bool {
    let review = shop.open_with_threads_then_head_files(&[thread]);
    review.current_step().unwrap().threads.is_empty()
        && ids(&review.safety_net().unwrap().threads) == ["t1"]
}

#[test]
fn a_left_comment_on_an_unchanged_line_hangs_under_the_row_the_step_shows_for_it() {
    let base: Vec<_> = (1..=10).map(|n| format!("l{n}")).collect();
    let mut head = base.clone();
    head.splice(2..2, ["new1".to_owned(), "new2".to_owned()]);
    let shop = Shop::with_list(
        &base.iter().map(String::as_str).collect::<Vec<_>>(),
        &head.iter().map(String::as_str).collect::<Vec<_>>(),
    );

    let on_base = |line| thread("t1", "list.txt", Side::Base, line);
    // base line 4 is head line 6, three lines below the two added ones
    assert_eq!(hangs_in_step(&shop, on_base(4)), Some((Side::Base, 4)));
    assert_eq!(hangs_in_step(&shop, on_base(1)), Some((Side::Base, 1)));
    // base line 6 is head line 8, outside what the step shows
    assert!(is_in_safety_net_only(&shop, on_base(6)));
}

#[test]
fn threads_around_a_deletion_only_piece_hang_under_the_rows_the_step_shows() {
    let base: Vec<_> = (1..=10).map(|n| format!("l{n}")).collect();
    let mut head = base.clone();
    head.drain(4..6);
    let shop = Shop::with_list(
        &base.iter().map(String::as_str).collect::<Vec<_>>(),
        &head.iter().map(String::as_str).collect::<Vec<_>>(),
    );
    let on = |side, line| thread("t1", "list.txt", side, line);

    // head lines 2..=7 show: three above the cut, three below
    assert_eq!(
        hangs_in_step(&shop, on(Side::Head, 4)),
        Some((Side::Head, 4))
    );
    assert_eq!(
        hangs_in_step(&shop, on(Side::Head, 5)),
        Some((Side::Head, 5))
    );
    assert_eq!(
        hangs_in_step(&shop, on(Side::Base, 5)),
        Some((Side::Base, 5))
    );
    // base line 8 is head line 6
    assert_eq!(
        hangs_in_step(&shop, on(Side::Base, 8)),
        Some((Side::Base, 8))
    );
    assert!(is_in_safety_net_only(&shop, on(Side::Head, 8)));
}

#[test]
fn a_comment_starting_with_the_robot_is_shown_as_its_authors_agent_without_the_robot() {
    let by_agent = comment("ekvedaras", false, "🤖 One variadic method: not as-is.");

    assert!(by_agent.is_by_agent());
    assert_eq!(by_agent.shown_author(), "ekvedaras's agent");
    assert_eq!(by_agent.shown_body(), "One variadic method: not as-is.");
}

#[test]
fn a_comment_with_the_robot_elsewhere_is_the_persons_own_and_shown_as_written() {
    let by_person = comment("alice", false, "The 🤖 said so");

    assert!(!by_person.is_by_agent());
    assert_eq!(by_person.shown_author(), "alice");
    assert_eq!(by_person.shown_body(), "The 🤖 said so");
}
