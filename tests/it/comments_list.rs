//! The comments list: every thread of the review in reading order, and going to one.

use crate::support::FixtureRepo;
use crate::support::file_path;
use crate::support::line;
use guided_review::build::Build;
use guided_review::local_git::LocalGit;
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
        LocalGit::find(&crate::support::shop_on_github(), self.repo.path())
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
        let mut review = Review::open(
            tour,
            &source,
            state_file,
            crate::support::fake_ports::every_capability(),
        )
        .unwrap();
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
                host: None,
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
        let mut review = Review::open_basic(
            tour,
            &self.source(),
            state_file,
            crate::support::fake_ports::every_capability(),
        )
        .unwrap();
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

fn headings(list: &CommentsList) -> Vec<(String, Vec<String>)> {
    list.sections()
        .iter()
        .flat_map(|section| {
            section.groups.iter().map(|group| {
                (
                    format!("{} / {}", section.title, group.heading),
                    group.rows.iter().map(|row| row.id.to_string()).collect(),
                )
            })
        })
        .collect()
}

fn resolved(thread: ReviewThread) -> ReviewThread {
    ReviewThread {
        is_resolved: true,
        ..thread
    }
}

fn ids(list: &CommentsList) -> Vec<String> {
    list.rows().iter().map(|row| row.id.to_string()).collect()
}

#[test]
fn rows_follow_the_reading_order_each_with_its_place_author_and_first_line() {
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
    assert_eq!(order.author, "alice");
    assert_eq!(order.first_line, "Why two?");
    assert_eq!(order.replies, 1);
}

#[test]
fn rows_sit_under_the_heading_of_their_screen_with_the_step_number_the_page_shows() {
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
                vec![comment("alice", "Why two?")],
            ),
        ],
    );

    assert_eq!(
        headings(&review.comments_list()),
        [
            (
                "Open · 2 / Step 1 · Title of order".to_owned(),
                vec!["on-order".to_owned()]
            ),
            (
                "Open · 2 / Step 2 · Title of config".to_owned(),
                vec!["on-config".to_owned()]
            ),
        ]
    );
}

#[test]
fn open_threads_come_before_resolved_ones_each_in_reading_order() {
    let shop = Shop::with_pull();
    let review = shop.open_with(
        two_steps(),
        &[
            resolved(thread(
                "order-done",
                "app/Order.php",
                7,
                vec![comment("alice", "Why two?")],
            )),
            thread(
                "config-open",
                "config.yml",
                3,
                vec![comment("bob", "Why three?")],
            ),
            thread(
                "order-open",
                "app/Order.php",
                7,
                vec![comment("carol", "Why 2?")],
            ),
        ],
    );

    let list = review.comments_list();

    assert_eq!(ids(&list), ["order-open", "config-open"]);
    let sections = list.sections();
    let titles: Vec<(&str, bool)> = sections
        .iter()
        .map(|section| (section.title.as_str(), section.is_resolved))
        .collect();
    assert_eq!(titles, [("Open · 2", false), ("Resolved · 1", true)]);
    assert_eq!(
        headings(&list)[2],
        (
            "Resolved · 1 / Step 1 · Title of order".to_owned(),
            vec!["order-done".to_owned()]
        )
    );
}

#[test]
fn a_section_with_no_thread_is_left_out() {
    let shop = Shop::with_pull();
    let review = shop.open_with(
        two_steps(),
        &[resolved(thread(
            "only",
            "config.yml",
            3,
            vec![comment("bob", "Why three?")],
        ))],
    );

    let titles: Vec<String> = review
        .comments_list()
        .sections()
        .iter()
        .map(|section| section.title.clone())
        .collect();

    assert_eq!(titles, ["Resolved · 1"]);
}

#[test]
fn a_comment_that_is_only_a_suggestion_reads_suggested_change() {
    let shop = Shop::with_pull();
    let review = shop.open_with(
        two_steps(),
        &[thread(
            "suggesting",
            "app/Order.php",
            7,
            vec![comment("alice", "```suggestion\nreturn 3;\n```")],
        )],
    );

    let list = review.comments_list();
    let row = &list.rows()[0];

    assert_eq!(row.first_line, "Suggested change");
    assert!(row.is_suggestion);
}

#[test]
fn the_first_line_is_the_first_line_of_prose_not_a_fence() {
    let shop = Shop::with_pull();
    let review = shop.open_with(
        two_steps(),
        &[
            thread(
                "explained",
                "app/Order.php",
                7,
                vec![comment(
                    "alice",
                    "Use this instead:\n```suggestion\nreturn 3;\n```",
                )],
            ),
            thread(
                "fenced",
                "config.yml",
                3,
                vec![comment("bob", "```php\n\necho 1;\n```")],
            ),
        ],
    );

    let list = review.comments_list();
    let rows = list.rows();
    let first_lines: Vec<&str> = rows.iter().map(|row| row.first_line.as_str()).collect();

    assert_eq!(first_lines, ["Use this instead:", "echo 1;"]);
    assert!(rows.iter().all(|row| !row.is_suggestion));
}

#[test]
fn a_row_carries_the_picture_of_its_author_and_whether_an_agent_wrote_it() {
    let shop = Shop::with_pull();
    let with_picture = ThreadComment {
        avatar_url: Some("https://avatars.example/alice".to_owned()),
        ..comment("alice", "Why two?")
    };
    let by_agent = comment("bob", "🤖 Why three?");
    let review = shop.open_with(
        two_steps(),
        &[
            thread("on-order", "app/Order.php", 7, vec![with_picture]),
            thread("on-config", "config.yml", 3, vec![by_agent]),
        ],
    );

    let list = review.comments_list();
    let rows = list.rows();

    assert_eq!(
        rows[0].avatar_url.as_deref(),
        Some("https://avatars.example/alice")
    );
    assert!(!rows[0].is_by_agent);
    assert!(rows[1].is_by_agent);
    assert_eq!(rows[1].author, "bob's agent");
}

#[test]
fn a_thread_no_screen_shows_comes_last_under_not_in_this_tour() {
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
        headings(&list),
        [
            (
                "Open · 2 / app/Order.php".to_owned(),
                vec!["on-order".to_owned()]
            ),
            (
                "Open · 2 / Not in this tour".to_owned(),
                vec!["on-gone".to_owned()]
            ),
        ]
    );
}

#[test]
fn the_cursor_cycles_over_the_rows_from_the_last_to_the_first_and_back() {
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

    list.cursor_down();
    assert_eq!(
        list.thread_under_cursor(),
        Some(&ThreadId::new("on-config"))
    );
    let under: Vec<bool> = list.rows().iter().map(|row| row.is_under_cursor).collect();
    assert_eq!(under, [false, true]);

    list.cursor_down();
    assert_eq!(list.thread_under_cursor(), Some(&ThreadId::new("on-order")));
    list.cursor_up();
    assert_eq!(
        list.thread_under_cursor(),
        Some(&ThreadId::new("on-config"))
    );
}

/// One open thread and two resolved ones, as the Resolved section's tests need.
fn open_and_two_resolved(shop: &Shop) -> Review {
    shop.open_with(
        two_steps(),
        &[
            thread(
                "open",
                "app/Order.php",
                7,
                vec![comment("alice", "Why two?")],
            ),
            resolved(thread(
                "done-a",
                "app/Order.php",
                7,
                vec![comment("bob", "Fine")],
            )),
            resolved(thread(
                "done-b",
                "config.yml",
                3,
                vec![comment("carol", "Ok")],
            )),
        ],
    )
}

fn cursor_on(list: &mut CommentsList, id: &str) {
    for _ in 0..8 {
        if list.thread_under_cursor() == Some(&ThreadId::new(id)) {
            return;
        }
        list.cursor_down();
    }
    panic!("the cursor never reached {id}");
}

#[test]
fn resolved_threads_are_folded_by_default_and_the_cursor_walk_stops_on_their_heading() {
    let shop = Shop::with_pull();
    let mut list = open_and_two_resolved(&shop).comments_list();

    assert!(!list.resolved_are_shown());
    assert_eq!(ids(&list), ["open"]);
    let sections = list.sections();
    assert_eq!(sections[1].title, "Resolved · 2");
    assert!(sections[0].is_shown);
    assert!(!sections[1].is_shown);
    assert!(!list.is_on_resolved_header());

    list.cursor_down();

    assert!(list.is_on_resolved_header());
    assert_eq!(list.thread_under_cursor(), None);
    assert!(list.sections()[1].is_header_under_cursor);
    assert!(!list.sections()[0].is_header_under_cursor);
    list.cursor_down();
    assert_eq!(list.thread_under_cursor(), Some(&ThreadId::new("open")));
}

#[test]
fn the_resolved_heading_unfolds_and_folds_by_toggle_right_and_left() {
    let shop = Shop::with_pull();
    let mut list = open_and_two_resolved(&shop).comments_list();
    list.cursor_down();

    list.toggle_resolved_shown();
    assert!(list.resolved_are_shown());
    assert_eq!(ids(&list), ["open", "done-a", "done-b"]);
    assert!(list.is_on_resolved_header());

    list.fold_under_cursor();
    assert!(!list.resolved_are_shown());
    assert_eq!(ids(&list), ["open"]);

    list.unfold_under_cursor();
    assert!(list.resolved_are_shown());
    list.unfold_under_cursor();
    assert!(list.resolved_are_shown());

    list.toggle_resolved_shown();
    assert!(!list.resolved_are_shown());
}

#[test]
fn left_on_a_resolved_row_goes_up_to_the_heading_and_the_walk_includes_the_rows_while_shown() {
    let shop = Shop::with_pull();
    let mut list = open_and_two_resolved(&shop).comments_list();
    list.cursor_down();
    list.unfold_under_cursor();

    list.cursor_down();
    assert_eq!(list.thread_under_cursor(), Some(&ThreadId::new("done-a")));
    cursor_on(&mut list, "done-b");
    list.cursor_down();
    assert_eq!(list.thread_under_cursor(), Some(&ThreadId::new("open")));

    cursor_on(&mut list, "done-b");
    list.fold_under_cursor();

    assert!(list.is_on_resolved_header());
    assert!(list.resolved_are_shown());
    let under: Vec<bool> = list.rows().iter().map(|row| row.is_under_cursor).collect();
    assert_eq!(under, [false, false, false]);
}

#[test]
fn folding_with_the_cursor_on_a_resolved_row_leaves_the_cursor_on_the_heading() {
    let shop = Shop::with_pull();
    let mut list = open_and_two_resolved(&shop).comments_list();
    list.cursor_down();
    list.unfold_under_cursor();
    cursor_on(&mut list, "done-b");

    list.toggle_resolved_shown();

    assert!(list.is_on_resolved_header());
    assert!(!list.resolved_are_shown());
}

#[test]
fn resolved_threads_are_shown_when_no_thread_is_open() {
    let shop = Shop::with_pull();
    let review = shop.open_with(
        two_steps(),
        &[resolved(thread(
            "only",
            "config.yml",
            3,
            vec![comment("bob", "Ok")],
        ))],
    );
    let mut list = review.comments_list();

    assert!(list.resolved_are_shown());
    assert_eq!(ids(&list), ["only"]);
    assert!(list.is_on_resolved_header());
    list.cursor_down();
    assert_eq!(list.thread_under_cursor(), Some(&ThreadId::new("only")));
    list.cursor_down();
    assert!(list.is_on_resolved_header());
}

#[test]
fn a_list_with_no_resolved_thread_has_no_heading_to_stop_on() {
    let shop = Shop::with_pull();
    let review = shop.open_with(
        two_steps(),
        &[thread(
            "open",
            "app/Order.php",
            7,
            vec![comment("alice", "Why two?")],
        )],
    );
    let mut list = review.comments_list();

    assert!(!list.is_on_resolved_header());
    list.cursor_down();
    assert_eq!(list.thread_under_cursor(), Some(&ThreadId::new("open")));
    assert!(!list.is_on_resolved_header());
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
        ThreadVisit::OnHost("https://github.com/acme/shop/pull/7#discussion_bob".to_owned())
    );
    assert_eq!(review.screen(), &before);
}

#[test]
fn c_and_the_palette_open_the_comments_list_with_threads_or_with_none_yet() {
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
        without
            .palette_actions(Build::Local)
            .contains(&PaletteAction::CommentsList)
    );
    assert_eq!(
        without.decide(Intent::ToggleCommentsList, WindowMoment::default()),
        Effect::Toggle(Overlay::Comments)
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

#[test]
fn a_list_open_while_threads_change_takes_the_new_threads_and_keeps_the_cursor_on_its_thread() {
    let shop = Shop::with_pull();
    let on_order = || {
        thread(
            "on-order",
            "app/Order.php",
            7,
            vec![comment("alice", "Why two?")],
        )
    };
    let on_config = || {
        thread(
            "on-config",
            "config.yml",
            3,
            vec![comment("bob", "Why three?")],
        )
    };
    let before = shop.open_with(two_steps(), &[on_order(), on_config()]);
    let resolved_order = ReviewThread {
        is_resolved: true,
        ..on_order()
    };
    let after = shop.open_with(two_steps(), &[resolved_order, on_config()]);
    let mut list = before.comments_list();
    list.cursor_down();
    assert_eq!(
        list.thread_under_cursor(),
        Some(&ThreadId::new("on-config"))
    );

    let list = list.refreshed_to(&after.comments_list());

    assert_eq!(ids(&list), ["on-config"]);
    assert_eq!(
        list.thread_under_cursor(),
        Some(&ThreadId::new("on-config"))
    );
}

#[test]
fn a_list_open_while_threads_change_keeps_the_cursor_on_the_resolved_heading() {
    let shop = Shop::with_pull();
    let on_order = thread(
        "on-order",
        "app/Order.php",
        7,
        vec![comment("alice", "Why two?")],
    );
    let on_config = thread(
        "on-config",
        "config.yml",
        3,
        vec![comment("bob", "Why three?")],
    );
    let resolved = ReviewThread {
        is_resolved: true,
        ..thread("done", "config.yml", 3, vec![comment("bob", "Fixed.")])
    };
    let before = shop.open_with(two_steps(), &[on_order.clone(), resolved.clone()]);
    let after = shop.open_with(two_steps(), &[on_order, on_config, resolved]);
    let mut list = before.comments_list();
    list.cursor_down();
    assert!(list.is_on_resolved_header());

    let list = list.refreshed_to(&after.comments_list());

    assert!(list.is_on_resolved_header());
}

#[test]
fn a_search_keeps_the_threads_whose_place_author_or_words_hold_every_word_resolved_ones_too() {
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
            resolved(thread(
                "on-order",
                "app/Order.php",
                7,
                vec![
                    comment("alice", "Why two?"),
                    comment("carol", "Prices went up."),
                ],
            )),
        ],
    );
    let mut list = review.comments_list();

    list.set_query("PRICES");
    assert_eq!(ids(&list), ["on-order"]);
    list.set_query("why bob");
    assert_eq!(ids(&list), ["on-config"]);
    list.set_query("order.php alice");
    assert_eq!(ids(&list), ["on-order"]);
    list.set_query("");
    assert_eq!(ids(&list), ["on-config"]);
    assert_eq!(list.thread_count(), 2);
}
