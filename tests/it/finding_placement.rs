//! Where the findings of an AI review show: on their lines in the step, on the safety net, on the summary.

use crate::support::FixtureRepo;
use guided_review::local_git::LocalGit;
use guided_review::open_review::open_review;
use guided_review::pull_request::{RepoName, Sha};
use guided_review::review_subject::ReviewSubject;
use guided_review::tour::Tour;
use guided_review::tour::{BasicTour, TourPin};
use guided_review::update::Build;
use guided_review::viewer::{
    Effect, Intent, LineKind, Page, PageRow, PaletteAction, Review, ReviewStateFile, Screen,
    WindowMoment,
};
use serde_json::{Value, json};

const PR: &str = "https://github.com/acme/shop/pull/7";

fn pr() -> ReviewSubject {
    PR.parse().unwrap()
}

/// `README.md` changes on line 2 (`bye` becomes `world`) and is the only step; `other.txt` is in no step.
fn shop(findings: &[Value]) -> Review {
    let repo = FixtureRepo::cloned_from("acme/shop");
    repo.write("README.md", "hello\nbye\nend\n");
    repo.write("other.txt", "x\n");
    let base = repo.commit("base");
    repo.write("README.md", "hello\nworld\nend\n");
    repo.write("other.txt", "y\n");
    let head = repo.commit("head");
    let tour = json!({
        "pr": PR, "base": base, "head": head, "title": "Readme", "summary": [],
        "steps": [{"id": "readme", "title": "Readme", "why": "w", "pieces": [{"file": "README.md"}]}],
    });
    let path = Tour::path_for(&repo.folders(), &pr());
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, tour.to_string()).unwrap();
    let findings_path = Tour::findings_path_for(&repo.folders(), &pr());
    std::fs::write(
        findings_path,
        json!({"pr": PR, "base": base, "head": head, "findings": findings}).to_string(),
    )
    .unwrap();
    open_review(&pr(), repo.path(), &repo.folders())
        .unwrap()
        .review
}

fn finding(id: &str, severity: &str, anchor: Value) -> Value {
    json!({"id": id, "severity": severity, "body": "Why.", "anchor": anchor})
}

fn on_lines(side: &str, from: u32, to: u32) -> Value {
    json!({"kind": "lines", "file": "README.md", "side": side, "lines": [from, to]})
}

/// One word per row that matters: `+2` an added line 2, `-2` a removed one, `finding a UnderLine`.
fn shape(row: &PageRow) -> Option<String> {
    match row {
        PageRow::Line(line) => {
            let sign = match line.kind {
                LineKind::Removed => '-',
                LineKind::Added => '+',
                LineKind::Context { .. } => ' ',
            };
            Some(format!("{sign}{}", line.location.line))
        }
        PageRow::Finding(finding) => Some(format!("finding {} {:?}", finding.id, finding.place)),
        _ => None,
    }
}

fn shapes(page: &Page) -> Vec<String> {
    page.rows().iter().filter_map(shape).collect()
}

fn on_step(mut review: Review) -> Review {
    review.go_forward();
    review
}

#[test]
fn a_finding_on_added_lines_hangs_under_the_last_of_them() {
    let review = on_step(shop(&[finding("a", "must fix", on_lines("head", 2, 2))]));

    assert_eq!(shapes(review.page()), ["-2", "+2", "finding a UnderLine"]);
}

#[test]
fn a_finding_on_removed_lines_hangs_under_the_last_of_them() {
    let review = on_step(shop(&[finding("a", "nit", on_lines("base", 2, 2))]));

    assert_eq!(shapes(review.page()), ["-2", "finding a UnderLine", "+2"]);
}

#[test]
fn a_finding_on_a_whole_file_follows_the_code_of_the_step_that_shows_it() {
    let review = on_step(shop(&[finding(
        "a",
        "should fix",
        json!({"kind": "file", "file": "README.md"}),
    )]));

    assert_eq!(shapes(review.page()), ["-2", "+2", "finding a AfterCode"]);
}

#[test]
fn a_finding_on_lines_no_step_shows_goes_to_the_safety_net() {
    let mut review = shop(&[finding(
        "a",
        "must fix",
        json!({"kind": "lines", "file": "other.txt", "side": "head", "lines": [1, 1]}),
    )]);
    review.go_to_safety_net();

    assert_eq!(shapes(review.page()), ["-1", "+1", "finding a UnderLine"]);
}

#[test]
fn a_finding_about_the_whole_change_shows_on_the_summary() {
    let mut review = shop(&[finding("a", "must fix", json!({"kind": "change"}))]);
    review.go_to_summary();

    assert_eq!(shapes(review.page()), ["finding a AfterCode"]);
}

#[test]
fn a_finding_on_a_line_the_step_of_its_file_does_not_show_goes_to_the_safety_net_after_its_code() {
    let mut review = shop(&[finding("a", "nit", on_lines("head", 3, 3))]);
    review.go_forward();
    assert_eq!(shapes(review.page()), ["-2", "+2"]);

    review.go_to_safety_net();

    assert_eq!(shapes(review.page()), ["-1", "+1", "finding a AfterCode"]);
}

/// A basic tour of the same change: an overview, then a screen for each file.
fn basic_shop(findings: &[Value]) -> Review {
    let repo = FixtureRepo::cloned_from("acme/shop");
    repo.write("README.md", "hello\nbye\nend\n");
    let base = repo.commit("base");
    repo.write("README.md", "hello\nworld\nend\n");
    let head = repo.commit("head");
    let tour = BasicTour::new(
        TourPin {
            version: 1,
            pr: PR.to_owned(),
            local: None,
            repo: None,
            base: Sha::new(&base),
            head: Sha::new(&head),
            previous_head: None,
        },
        "Readme".to_owned(),
    );
    let findings_path = Tour::findings_path_for(&repo.folders(), &pr());
    std::fs::create_dir_all(findings_path.parent().unwrap()).unwrap();
    std::fs::write(
        findings_path,
        json!({"pr": PR, "base": base, "head": head, "findings": findings}).to_string(),
    )
    .unwrap();
    let source = LocalGit::find(&RepoName::new("acme", "shop"), repo.path())
        .unwrap()
        .unwrap();
    let state_file = ReviewStateFile::for_subject(&repo.folders(), &pr());
    let mut review = Review::open_basic(tour, &source, state_file).unwrap();
    review.read_findings(&repo.folders());
    review
}

#[test]
fn a_basic_tour_shows_a_finding_on_the_screen_of_its_file_and_one_of_the_change_on_the_overview() {
    let mut review = basic_shop(&[
        finding("a", "must fix", on_lines("head", 2, 2)),
        finding("b", "nit", json!({"kind": "change"})),
    ]);
    assert_eq!(shapes(review.page()), ["finding b AfterCode"]);

    review.go_forward();

    assert_eq!(shapes(review.page()), ["-2", "+2", "finding a UnderLine"]);
}

fn on_other_file() -> Value {
    json!({"kind": "lines", "file": "other.txt", "side": "head", "lines": [1, 1]})
}

#[test]
fn next_finding_walks_the_findings_most_severe_first_and_goes_round() {
    let mut review = shop(&[
        finding("nit", "nit", on_other_file()),
        finding("must", "must fix", json!({"kind": "change"})),
        finding("should", "should fix", on_lines("head", 2, 2)),
    ]);

    let walked: Vec<_> = (0..4)
        .map(|_| {
            let id = review.go_to_next_finding().map(|id| id.to_string());
            (id, review.screen().clone())
        })
        .collect();

    assert_eq!(
        walked,
        [
            (Some("must".to_owned()), Screen::Summary),
            (Some("should".to_owned()), Screen::Step("readme".into())),
            (Some("nit".to_owned()), Screen::SafetyNet),
            (Some("must".to_owned()), Screen::Summary),
        ]
    );
}

#[test]
fn next_finding_lands_on_the_row_of_the_finding() {
    let mut review = shop(&[finding("a", "must fix", on_lines("head", 2, 2))]);

    let id = review.go_to_next_finding().unwrap();

    assert!(matches!(
        review
            .page()
            .row(review.page().row_of_finding(&id).unwrap()),
        Some(PageRow::Finding(_))
    ));
}

#[test]
fn next_finding_with_no_findings_goes_nowhere() {
    let mut review = shop(&[]);

    assert!(review.go_to_next_finding().is_none());
    assert_eq!(review.screen(), &Screen::Summary);
}

#[test]
fn next_finding_is_offered_in_the_sidebar_and_the_palette_only_while_there_are_findings() {
    let with = shop(&[finding("a", "nit", json!({"kind": "change"}))]);
    let without = shop(&[]);

    assert!(with.ai_review_stage().can_go_to_next_finding);
    assert!(
        with.palette_actions(Build::Local)
            .contains(&PaletteAction::NextFinding)
    );
    assert_eq!(
        with.decide(Intent::NextFinding, WindowMoment::default()),
        Effect::NextFinding
    );
    assert!(!without.ai_review_stage().can_go_to_next_finding);
    assert!(
        !without
            .palette_actions(Build::Local)
            .contains(&PaletteAction::NextFinding)
    );
    assert_eq!(
        without.decide(Intent::NextFinding, WindowMoment::default()),
        Effect::Nothing
    );
    assert_eq!(
        Intent::from(PaletteAction::NextFinding),
        Intent::NextFinding
    );
    assert_eq!(PaletteAction::NextFinding.label(), "Next finding");
}
