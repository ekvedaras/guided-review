//! The findings list: rows, ticking, keys, and what a batch does.

use crate::support::FixtureRepo;
use crate::support::fake_ports::{FakeGitHub, GitHubCall, SentLog};
use guided_review::findings::{Finding, FindingId};
use guided_review::open_review::open_review;
use guided_review::review_subject::ReviewSubject;
use guided_review::tour::Tour;
use guided_review::update::Build;
use guided_review::viewer::{
    Command, CommentRefusal, CommentResult, Effect, FindingRefusal, FindingsList, Intent, Overlay,
    PaletteAction, Review, Screen, SendMode, WindowMoment,
};
use serde_json::{Value, json};

fn id(text: &str) -> FindingId {
    FindingId::try_from(text.to_owned()).unwrap()
}

fn finding(id: &str, severity: &str, body: &str, anchor: Value) -> Finding {
    serde_json::from_value(json!({"id": id, "severity": severity, "body": body, "anchor": anchor}))
        .unwrap()
}

fn on_line(file: &str, line: u32) -> Value {
    json!({"kind": "lines", "file": file, "side": "head", "lines": [line, line]})
}

fn three() -> Vec<Finding> {
    vec![
        finding(
            "a",
            "nit",
            "Nit one.\n\nMore on it.",
            on_line("src/a.rs", 12),
        ),
        finding(
            "b",
            "must fix",
            "Breaks.",
            json!({"kind": "file", "file": "src/b.rs"}),
        ),
        finding("c", "should fix", "Whole.", json!({"kind": "change"})),
    ]
}

fn ids(list: &FindingsList) -> Vec<String> {
    list.rows().iter().map(|row| row.id.to_string()).collect()
}

#[test]
fn rows_are_most_severe_first_each_with_its_place_and_first_line() {
    let list = FindingsList::of(&three());

    assert_eq!(ids(&list), ["b", "c", "a"]);
    let places: Vec<String> = list.rows().iter().map(|row| row.place.clone()).collect();
    assert_eq!(places, ["src/b.rs", "the whole change", "src/a.rs:12"]);
    assert_eq!(list.rows()[2].first_line, "Nit one.");
}

#[test]
fn ticking_selects_in_the_lists_order_and_select_all_toggles_every_row() {
    let mut list = FindingsList::of(&three());

    list.toggle_selected(&id("a"));
    list.toggle_selected(&id("b"));
    assert_eq!(list.selected(), [id("b"), id("a")]);
    list.toggle_selected(&id("b"));
    assert_eq!(list.selected(), [id("a")]);

    list.toggle_all();
    assert!(list.all_selected());
    assert_eq!(list.selected().len(), 3);
    list.toggle_all();
    assert_eq!(list.selected().len(), 0);
}

#[test]
fn the_cursor_moves_over_rows_and_stops_at_the_ends_and_ticks_the_row_it_is_on() {
    let mut list = FindingsList::of(&three());
    assert_eq!(list.id_under_cursor(), Some(&id("b")));

    list.cursor_up();
    assert_eq!(list.id_under_cursor(), Some(&id("b")));
    list.cursor_down();
    list.cursor_down();
    list.cursor_down();
    assert_eq!(list.id_under_cursor(), Some(&id("a")));
    assert!(list.rows()[2].is_under_cursor);

    list.toggle_under_cursor();
    assert_eq!(list.selected(), [id("a")]);
}

#[test]
fn the_cursor_goes_to_the_row_that_was_clicked_and_stays_put_when_there_is_no_such_row() {
    let mut list = FindingsList::of(&three());

    list.move_cursor_to(&id("a"));
    assert_eq!(list.id_under_cursor(), Some(&id("a")));
    list.move_cursor_to(&id("gone"));
    assert_eq!(list.id_under_cursor(), Some(&id("a")));
}

#[test]
fn one_row_is_open_at_a_time_and_shows_the_whole_finding() {
    let mut list = FindingsList::of(&three());

    list.toggle_open_under_cursor();
    assert!(list.rows()[0].is_open);
    list.cursor_down();
    list.toggle_open_under_cursor();
    let open: Vec<bool> = list.rows().iter().map(|row| row.is_open).collect();
    assert_eq!(open, [false, true, false]);
    list.toggle_open_under_cursor();
    assert!(list.rows().iter().all(|row| !row.is_open));
    assert_eq!(list.rows()[2].body, "Nit one.\n\nMore on it.");
}

#[test]
fn refreshing_over_fewer_findings_keeps_the_ticks_of_those_left() {
    let mut list = FindingsList::of(&three());
    list.toggle_all();

    let left: Vec<Finding> = three().into_iter().filter(|f| f.id != id("c")).collect();
    let refreshed = list.refreshed(&left);

    assert_eq!(ids(&refreshed), ["b", "a"]);
    assert_eq!(refreshed.selected(), [id("b"), id("a")]);
}

const PR: &str = "https://github.com/acme/shop/pull/7";

fn pr() -> ReviewSubject {
    PR.parse().unwrap()
}

/// `README.md` changes on line 2 and is the only step.
struct Shop {
    repo: FixtureRepo,
    head: String,
}

impl Shop {
    fn with_findings(findings: &[Finding]) -> Self {
        let repo = FixtureRepo::cloned_from("acme/shop");
        repo.write("README.md", "hello\nbye\nend\n");
        let base = repo.commit("base");
        repo.write("README.md", "hello\nworld\nend\n");
        let head = repo.commit("head");
        let tour = json!({
            "pr": PR, "base": base, "head": head, "title": "Readme", "summary": [],
            "steps": [{"id": "readme", "title": "Readme", "why": "w", "pieces": [{"file": "README.md"}]}],
        });
        let path = Tour::path_for(&repo.folders(), &pr());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, tour.to_string()).unwrap();
        std::fs::write(
            Tour::findings_path_for(&repo.folders(), &pr()),
            json!({"pr": PR, "base": base, "head": head, "findings": findings}).to_string(),
        )
        .unwrap();
        Self { repo, head }
    }

    /// The review as the app opens it, on its only step.
    fn open(&self) -> Review {
        let mut review = open_review(&pr(), self.repo.path(), &self.repo.folders())
            .unwrap()
            .review;
        review.go_forward();
        review
    }
}

fn on_line_2(id: &str, body: &str) -> Finding {
    finding(
        id,
        "should fix",
        body,
        json!({"kind": "lines", "file": "README.md", "side": "head", "lines": [2, 2]}),
    )
}

fn shown_ids(review: &Review) -> Vec<String> {
    review
        .findings_list()
        .rows()
        .iter()
        .map(|row| row.id.to_string())
        .collect()
}

#[test]
fn the_review_lists_its_open_findings_and_not_the_dismissed() {
    let shop = Shop::with_findings(&[on_line_2("a", "One."), on_line_2("b", "Two.")]);
    let mut review = shop.open();
    review.dismiss_finding(&id("a")).unwrap();

    assert_eq!(shown_ids(&review), ["b"]);
}

#[test]
fn a_batch_dismiss_dismisses_each_selected_finding_and_reports_one_that_is_not_open() {
    let shop = Shop::with_findings(&[
        on_line_2("a", "One."),
        on_line_2("b", "Two."),
        on_line_2("c", "Three."),
    ]);
    let mut review = shop.open();

    let batch = review.dismiss_findings(&[id("a"), id("c"), id("gone")]);

    assert_eq!(batch.done, [id("a"), id("c")]);
    assert_eq!(batch.refused, [(id("gone"), FindingRefusal::NotThere)]);
    assert_eq!(shown_ids(&review), ["b"]);
    assert_eq!(shown_ids(&shop.open()), ["b"]);
}

fn file_finding(id: &str, body: &str) -> Finding {
    finding(
        id,
        "nit",
        body,
        json!({"kind": "file", "file": "README.md"}),
    )
}

#[test]
fn an_open_row_holds_the_code_lines_of_its_finding_and_none_for_a_finding_on_no_lines() {
    let shop = Shop::with_findings(&[
        on_line_2("a", "On the line."),
        file_finding("f", "On the file."),
    ]);
    let review = shop.open();

    let rows = review.findings_list().rows();

    let lines_of = |name: &str| {
        let row = rows.iter().find(|row| row.id == id(name)).unwrap();
        row.lines
            .iter()
            .map(|line| (line.number, line.text.clone()))
            .collect::<Vec<_>>()
    };
    assert_eq!(lines_of("a"), [(2, "world".to_owned())]);
    assert_eq!(lines_of("f"), []);
}

#[test]
fn a_batch_comment_adds_each_finding_as_is_marked_as_ai_and_leaves_open_those_with_no_lines() {
    let shop = Shop::with_findings(&[
        on_line_2("a", "On the line."),
        file_finding("f", "On the file."),
        finding("c", "nit", "On the change.", json!({"kind": "change"})),
    ]);
    let mut review = shop.open();
    let sent = SentLog::default();

    let batch = review.comment_findings(&[id("a"), id("f"), id("c")], SendMode::AddToReview);

    assert_eq!(
        batch.refused,
        [(id("c"), FindingRefusal::Comment(CommentRefusal::NoLines))]
    );
    assert_eq!(batch.done.len(), 2);
    let github = FakeGitHub::with_pull("main-tip", &shop.head, &sent);
    for request in batch.done {
        assert_eq!(
            review.apply_comment(request.run(&github)).unwrap(),
            CommentResult::Sent
        );
    }
    let added: Vec<String> = sent
        .github_calls()
        .into_iter()
        .filter_map(|call| match call {
            GitHubCall::AddThread { body, .. } => Some(body),
            _ => None,
        })
        .collect();
    assert_eq!(added, ["🤖 On the line.", "🤖 On the file."]);
    assert_eq!(shown_ids(&review), ["c"]);
}

#[test]
fn a_batch_comment_of_findings_on_the_same_lines_sends_them_all_as_one_comment() {
    let shop = Shop::with_findings(&[on_line_2("a", "First."), on_line_2("b", "Second.")]);
    let mut review = shop.open();
    let sent = SentLog::default();

    let batch = review.comment_findings(&[id("a"), id("b")], SendMode::AddToReview);

    assert_eq!(batch.refused, []);
    assert_eq!(batch.done.len(), 1);
    assert_eq!(shown_ids(&review), ["a", "b"]);
    let github = FakeGitHub::with_pull("main-tip", &shop.head, &sent);
    for request in batch.done {
        assert_eq!(
            review.apply_comment(request.run(&github)).unwrap(),
            CommentResult::Sent
        );
    }
    let added: Vec<String> = sent
        .github_calls()
        .into_iter()
        .filter_map(|call| match call {
            GitHubCall::AddThread { body, .. } => Some(body),
            _ => None,
        })
        .collect();
    assert_eq!(added, ["🤖 First.\n\n🤖 Second."]);
    assert_eq!(shown_ids(&review), Vec::<String>::new());
}

#[test]
fn a_batch_comment_keeps_open_every_finding_whose_lines_have_a_box_with_text_in_it() {
    let shop = Shop::with_findings(&[
        on_line_2("a", "First."),
        on_line_2("b", "Second."),
        on_line_2("c", "Third."),
    ]);
    let mut review = shop.open();
    review.use_finding(&id("c")).unwrap();

    let batch = review.comment_findings(&[id("a"), id("b")], SendMode::AddToReview);

    assert!(batch.done.is_empty());
    assert_eq!(
        batch.refused,
        [
            (id("a"), FindingRefusal::DraftInTheWay),
            (id("b"), FindingRefusal::DraftInTheWay)
        ]
    );
}

#[test]
fn the_findings_list_is_offered_in_the_sidebar_the_palette_and_by_key_only_while_there_are_findings()
 {
    let with = Shop::with_findings(&[on_line_2("a", "One.")]).open();
    let without = Shop::with_findings(&[]).open();

    assert!(with.ai_review_stage().has_findings);
    assert!(
        with.palette_actions(Build::Local)
            .contains(&PaletteAction::FindingsList)
    );
    assert_eq!(
        with.decide(Intent::ToggleFindingsList, WindowMoment::default()),
        Effect::Toggle(Overlay::Findings)
    );
    assert!(!without.ai_review_stage().has_findings);
    assert!(
        !without
            .palette_actions(Build::Local)
            .contains(&PaletteAction::FindingsList)
    );
    assert_eq!(
        without.decide(Intent::ToggleFindingsList, WindowMoment::default()),
        Effect::Nothing
    );
    assert_eq!(
        Intent::from(PaletteAction::FindingsList),
        Intent::ToggleFindingsList
    );
    assert_eq!(PaletteAction::FindingsList.label(), "Findings list");
    assert_eq!(
        PaletteAction::FindingsList.command(),
        Some(Command::FindingsList)
    );
    assert_eq!(Command::FindingsList.default_chords()[0].keystroke(), "f");
}

#[test]
fn going_to_a_finding_opens_the_screen_that_shows_it() {
    let shop = Shop::with_findings(&[
        finding("c", "nit", "Change.", json!({"kind": "change"})),
        on_line_2("a", "One."),
    ]);
    let mut review = shop.open();
    assert!(matches!(review.screen(), Screen::Step(_)));

    assert!(review.go_to_finding(&id("c")));
    assert_eq!(review.screen(), &Screen::Summary);
    assert!(review.page().row_of_finding(&id("c")).is_some());
    assert!(!review.go_to_finding(&id("gone")));
}
