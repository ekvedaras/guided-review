use crate::support::FixtureRepo;
use guided_review::open_review::open_review;
use guided_review::review_subject::ReviewSubject;
use guided_review::tour::Tour;
use guided_review::viewer::AiReview;
use serde_json::{Value, json};

const PR: &str = "https://github.com/acme/shop/pull/7";

fn pr() -> ReviewSubject {
    PR.parse().unwrap()
}

fn shop_with_a_tour() -> (FixtureRepo, String, String) {
    let repo = FixtureRepo::cloned_from("acme/shop");
    repo.write("README.md", "hello\n");
    let base = repo.commit("base");
    repo.write("README.md", "hello\nworld\n");
    let head = repo.commit("head");
    let tour = json!({
        "pr": PR, "base": base, "head": head, "title": "Readme", "summary": [],
        "steps": [{"id": "readme", "title": "Readme", "why": "w", "pieces": [{"file": "README.md"}]}],
    });
    let path = Tour::path_for(&repo.folders(), &pr());
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, tour.to_string()).unwrap();
    (repo, base, head)
}

fn write_findings(repo: &FixtureRepo, text: &str) {
    let path = Tour::findings_path_for(&repo.folders(), &pr());
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn finding(id: &str, severity: &str) -> Value {
    json!({"id": id, "severity": severity, "body": "Why.", "anchor": {"kind": "change"}})
}

fn findings_text(base: &str, head: &str, findings: &[Value]) -> String {
    json!({"pr": PR, "base": base, "head": head, "findings": findings}).to_string()
}

#[test]
fn a_review_with_no_findings_file_has_had_no_ai_review() {
    let (repo, _, _) = shop_with_a_tour();

    let opened = open_review(&pr(), repo.path(), &repo.folders()).unwrap();

    assert!(matches!(opened.review.ai_review(), AiReview::NotRun));
    assert_eq!(opened.review.ai_review().line(), "Not run");
}

#[test]
fn the_findings_beside_the_tour_are_loaded_when_the_review_opens() {
    let (repo, base, head) = shop_with_a_tour();
    write_findings(
        &repo,
        &findings_text(
            &base,
            &head,
            &[
                finding("a", "nit"),
                finding("b", "must fix"),
                finding("c", "nit"),
            ],
        ),
    );

    let opened = open_review(&pr(), repo.path(), &repo.folders()).unwrap();

    assert_eq!(opened.review.ai_review().counts().total(), 3);
    assert_eq!(
        opened.review.ai_review().line(),
        "3 findings: 1 must fix, 2 nit"
    );
}

#[test]
fn one_finding_reads_in_the_singular() {
    let (repo, base, head) = shop_with_a_tour();
    write_findings(
        &repo,
        &findings_text(&base, &head, &[finding("a", "should fix")]),
    );

    let opened = open_review(&pr(), repo.path(), &repo.folders()).unwrap();

    assert_eq!(opened.review.ai_review().line(), "1 finding: 1 should fix");
}

#[test]
fn a_review_agent_that_found_nothing_says_so() {
    let (repo, base, head) = shop_with_a_tour();
    write_findings(&repo, &findings_text(&base, &head, &[]));

    let opened = open_review(&pr(), repo.path(), &repo.folders()).unwrap();

    assert_eq!(opened.review.ai_review().line(), "No findings");
}

#[test]
fn a_findings_file_that_cannot_be_read_does_not_stop_the_review_opening() {
    let (repo, _, _) = shop_with_a_tour();
    write_findings(&repo, "not json");

    let opened = open_review(&pr(), repo.path(), &repo.folders()).unwrap();

    assert!(matches!(opened.review.ai_review(), AiReview::Unreadable(_)));
    assert!(opened.review.ai_review().line().starts_with("Not read: "));
}

#[test]
fn findings_done_while_the_review_is_open_reads_the_file_again() {
    let (repo, base, head) = shop_with_a_tour();
    let mut opened = open_review(&pr(), repo.path(), &repo.folders()).unwrap();
    write_findings(&repo, &findings_text(&base, &head, &[finding("a", "nit")]));

    opened.review.read_findings(&repo.folders());

    assert_eq!(opened.review.ai_review().counts().nit, 1);
}
