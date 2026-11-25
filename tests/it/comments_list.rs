//! The comments list: every thread of the review in reading order, and going to one.

use crate::support::FixtureRepo;
use crate::support::file_path;
use crate::support::line;
use guided_review::build::Build;
use guided_review::local_git::LocalGit;
use guided_review::pull_request::RepoName;
use guided_review::pull_request::Sha;
use guided_review::review_subject::ReviewSubject;
use guided_review::tour::{BasicTour, GuidedTour, TourPin};
use guided_review::viewer::{
    Command, CommentId, CommentsList, Effect, Intent, Overlay, PageRow, PaletteAction, Review,
    ReviewStateFile, ReviewThread, Screen, Side, ThreadComment, ThreadId, ThreadVisit,
    WindowMoment,
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
    fn open_with(&self, steps: Vec<Value>, threads: &[ReviewThread]) -> Review {
        let tour: GuidedTour = serde_json::from_value(json!({
            "pr": "https://github.com/acme/shop/pull/7",
            "base": self.base,
            "head": self.head,
            "title": "Orders cost two",
            "summary": ["Order total goes up"],
            "steps": steps,
        }))
        .unwrap();
        let source = self.source();
        let state_file = ReviewStateFile::for_subject(
            &self.repo.folders(),
            &"https://github.com/acme/shop/pull/7"
                .parse::<ReviewSubject>()
                .unwrap(),
        );
        let mut review = Review::open(tour, &source, state_file).unwrap();
        review.set_threads(threads);
        let files_at_head = review.files_at_head_request().read(&source);
        review.set_files_at_head(files_at_head);
        review
    }

    fn open_basic_with(&self, threads: &[ReviewThread]) -> Review {
        let tour = BasicTour::new(
            TourPin {
                version: 1,
                pr: "https://github.com/acme/shop/pull/7".to_owned(),
                local: None,
                repo: None,
                base: Sha::new(&self.base),
                head: Sha::new(&self.head),
                previous_head: None,
            },
            "Orders cost two".to_owned(),
        );
        let state_file = ReviewStateFile::for_subject(
            &self.repo.folders(),
            &"https://github.com/acme/shop/pull/7"
                .parse::<ReviewSubject>()
                .unwrap(),
        );
        let mut review = Review::open_basic(tour, &self.source(), state_file).unwrap();
        review.set_threads(threads);
        review
    }
}

fn step(id: &str, pieces: Value) -> Value {
    json!({"id": id, "title": format!("Title of {id}"), "why": "because", "pieces": pieces})
}

fn two_steps() -> Vec<Value> {
    vec![
        step("order", json!([{"file": "app/Order.php"}])),
        step("config", json!([{"file": "config.yml"}])),
    ]
}

fn comment(author: &str, body: &str) -> ThreadComment {
    ThreadComment {
        avatar_url: None,
        id: CommentId::new(&format!("C_{body}")),
        reactions: Vec::new(),
        author: author.into(),
        is_bot: false,
        by_agent: None,
        by_viewer: false,
        body: body.into(),
        url: format!("https://github.com/acme/shop/pull/7#discussion_{author}"),
    }
}

fn thread(id: &str, path: &str, number: usize, comments: Vec<ThreadComment>) -> ReviewThread {
    let line = line(number);
    ReviewThread {
        id: ThreadId::new(id),
        is_resolved: false,
        is_outdated: false,
        path: file_path(path),
        line: Some(line),
        start_line: None,
        side: Side::Head,
        is_file_comment: false,
        original_line: Some(line),
        original_start_line: None,
        original_commit: Some("abc".into()),
        comments,
    }
}

fn ids(list: &CommentsList) -> Vec<String> {
    list.rows().iter().map(|row| row.id.to_string()).collect()
}

#[test]
fn rows_follow_the_reading_order_each_with_its_place_step_author_and_first_line() {
    let shop = Shop::with_pull();
    let review = shop.open_with(
        two_steps(),
        &[
            thread(
                "on-config",
                "config.yml",
                3,
                vec![comment("bob", "Why three?")],
            ),
            thread(
                "on-order",
                "app/Order.php",
                7,
                vec![
                    comment("alice", "Why two?\n\nIt was one."),
                    comment("carol", "Prices went up."),
                ],
            ),
        ],
    );

    let list = review.comments_list();

    assert_eq!(ids(&list), ["on-order", "on-config"]);
    let order = &list.rows()[0];
    assert_eq!(order.place, "app/Order.php:7");
    assert_eq!(order.screen_title.as_deref(), Some("Title of order"));
    assert_eq!(order.author, "alice");
    assert_eq!(order.first_line, "Why two?");
    assert_eq!(order.replies, 1);
    assert_eq!(
        list.rows()[1].screen_title.as_deref(),
        Some("Title of config")
    );
}

#[test]
fn a_thread_no_screen_shows_comes_last_with_no_screen() {
    let shop = Shop::with_pull();
    let review = shop.open_basic_with(&[
        thread(
            "on-gone",
            "docs/gone.md",
            2,
            vec![comment("bob", "Still needed?")],
        ),
        thread(
            "on-order",
            "app/Order.php",
            7,
            vec![comment("alice", "Why two?")],
        ),
    ]);

    let list = review.comments_list();

    assert_eq!(ids(&list), ["on-order", "on-gone"]);
    assert_eq!(
        list.rows()[0].screen_title.as_deref(),
        Some("app/Order.php")
    );
    assert_eq!(list.rows()[1].screen_title, None);
}

#[test]
fn the_cursor_moves_over_the_rows_stops_at_the_ends_and_goes_to_a_clicked_row() {
    let shop = Shop::with_pull();
    let review = shop.open_with(
        two_steps(),
        &[
            thread(
                "on-order",
                "app/Order.php",
                7,
                vec![comment("alice", "Why two?")],
            ),
            thread(
                "on-config",
                "config.yml",
                3,
                vec![comment("bob", "Why three?")],
            ),
        ],
    );
    let mut list = review.comments_list();
    assert_eq!(list.thread_under_cursor(), Some(&ThreadId::new("on-order")));

    list.cursor_up();
    assert_eq!(list.thread_under_cursor(), Some(&ThreadId::new("on-order")));
    list.cursor_down();
    list.cursor_down();
    assert_eq!(
        list.thread_under_cursor(),
        Some(&ThreadId::new("on-config"))
    );
    let under: Vec<bool> = list.rows().iter().map(|row| row.is_under_cursor).collect();
    assert_eq!(under, [false, true]);

    list.move_cursor_to(&ThreadId::new("on-order"));
    assert_eq!(list.thread_under_cursor(), Some(&ThreadId::new("on-order")));
    list.move_cursor_to(&ThreadId::new("gone"));
    assert_eq!(list.thread_under_cursor(), Some(&ThreadId::new("on-order")));
}

#[test]
fn going_to_a_thread_opens_its_folded_step_and_unfolds_the_thread_on_its_page_row() {
    let shop = Shop::with_pull();
    let mut steps = two_steps();
    steps[1]["skim"] = json!(true);
    let resolved = ReviewThread {
        is_resolved: true,
        ..thread(
            "on-config",
            "config.yml",
            3,
            vec![comment("bob", "Why three?")],
        )
    };
    let mut review = shop.open_with(steps, &[resolved]);

    let visit = review.go_to_thread(&ThreadId::new("on-config"));

    assert_eq!(visit, ThreadVisit::OnScreen);
    assert_eq!(review.screen(), &Screen::Step("config".into()));
    assert!(!review.is_folded(&"config".into()));
    let page = review.page();
    let row = page
        .row_of_thread(&ThreadId::new("on-config"))
        .expect("the page shows the thread");
    let PageRow::Thread { view, .. } = &page.rows()[row] else {
        panic!("row {row} is not the thread");
    };
    assert!(!view.is_folded);
}

#[test]
fn a_thread_no_screen_shows_opens_on_github_and_the_screen_stays() {
    let shop = Shop::with_pull();
    let mut review = shop.open_basic_with(&[thread(
        "on-gone",
        "docs/gone.md",
        2,
        vec![comment("bob", "Still needed?")],
    )]);
    let before = review.screen().clone();

    let visit = review.go_to_thread(&ThreadId::new("on-gone"));

    assert_eq!(
        visit,
        ThreadVisit::OnGitHub("https://github.com/acme/shop/pull/7#discussion_bob".to_owned())
    );
    assert_eq!(review.screen(), &before);
}

#[test]
fn c_and_the_palette_open_the_comments_list_only_when_the_review_has_threads() {
    let shop = Shop::with_pull();
    let with = shop.open_with(
        two_steps(),
        &[thread(
            "on-order",
            "app/Order.php",
            7,
            vec![comment("alice", "Why two?")],
        )],
    );
    let without = shop.open_with(two_steps(), &[]);

    assert!(
        with.palette_actions(Build::Local)
            .contains(&PaletteAction::CommentsList)
    );
    assert_eq!(
        with.decide(Intent::ToggleCommentsList, WindowMoment::default()),
        Effect::Toggle(Overlay::Comments)
    );
    assert!(
        !without
            .palette_actions(Build::Local)
            .contains(&PaletteAction::CommentsList)
    );
    assert_eq!(
        without.decide(Intent::ToggleCommentsList, WindowMoment::default()),
        Effect::Nothing
    );
    assert_eq!(
        Intent::from(PaletteAction::CommentsList),
        Intent::ToggleCommentsList
    );
    assert_eq!(PaletteAction::CommentsList.label(), "Comments list");
    assert_eq!(
        PaletteAction::CommentsList.command(),
        Some(Command::CommentsList)
    );
    assert_eq!(Command::CommentsList.default_chords()[0].keystroke(), "c");
}
