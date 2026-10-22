#[allow(dead_code)]
mod support;

use guided_review::local_git::LocalGit;
use guided_review::pull_request::RepoName;
use guided_review::tour::Tour;
use guided_review::viewer::{Capture, DiffLine, Review, ReviewStateFile, Side};
use serde_json::json;
use support::FixtureRepo;

fn open_review(repo: &FixtureRepo, base: &str, head: &str) -> (Review, LocalGit) {
    let tour: Tour = serde_json::from_value(json!({
        "pr": "https://github.com/acme/shop/pull/7",
        "base": base,
        "head": head,
        "title": "t",
        "summary": [],
        "steps": [{"id": "all", "title": "All", "why": "w", "pieces": [{"rest": ""}], "children": []}],
    }))
    .unwrap();
    let source = LocalGit::find(&RepoName::new("acme", "shop"), repo.path())
        .unwrap()
        .unwrap();
    let state_file = ReviewStateFile::for_pr(
        repo.home(),
        &"https://github.com/acme/shop/pull/7".parse().unwrap(),
    );
    let mut review = Review::open(tour, &source, state_file).unwrap();
    review.go_forward();
    (review, source)
}

fn lines(review: &Review) -> Vec<DiffLine> {
    review
        .current_step()
        .unwrap()
        .pieces
        .into_iter()
        .flat_map(|piece| piece.lines)
        .collect()
}

fn highlighted(review: &mut Review, source: &LocalGit) {
    let highlights = review.highlight_request().compute(source);
    review.set_highlights(highlights);
}

fn captures_in(line: &DiffLine) -> Vec<(&str, Capture)> {
    line.spans
        .iter()
        .map(|span| (&line.text[span.range.clone()], span.capture))
        .collect()
}

fn line_with<'a>(lines: &'a [DiffLine], side: Side, needle: &str) -> &'a DiffLine {
    lines
        .iter()
        .find(|line| line.location.side == side && line.text.contains(needle))
        .unwrap_or_else(|| panic!("no {side:?} line with {needle}"))
}

#[test]
fn lines_have_no_colours_until_highlights_arrive() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    repo.write("a.php", "<?php\nreturn 1;\n");
    let base = repo.commit("base");
    repo.write("a.php", "<?php\nreturn 2;\n");
    let head = repo.commit("head");
    let (mut review, source) = open_review(&repo, &base, &head);

    assert!(lines(&review).iter().all(|line| line.spans.is_empty()));

    highlighted(&mut review, &source);

    assert!(lines(&review).iter().all(|line| !line.spans.is_empty()));
}

#[test]
fn php_lines_are_coloured_from_the_whole_file_on_their_own_side() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    repo.write("a.php", "<?php\nreturn 1;\n");
    let base = repo.commit("base");
    repo.write("a.php", "<?php\nreturn 2;\n");
    let head = repo.commit("head");
    let (mut review, source) = open_review(&repo, &base, &head);

    highlighted(&mut review, &source);

    let all = lines(&review);
    let removed = captures_in(line_with(&all, Side::Base, "return 1"));
    let added = captures_in(line_with(&all, Side::Head, "return 2"));
    assert!(
        removed.contains(&("return", Capture::Keyword)),
        "{removed:?}"
    );
    assert!(removed.contains(&("1", Capture::Number)), "{removed:?}");
    assert!(added.contains(&("2", Capture::Number)), "{added:?}");
}

#[test]
fn typescript_parameters_are_not_plain_variables() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    repo.write("a.ts", "export const a = 1;\n");
    let base = repo.commit("base");
    repo.write(
        "a.ts",
        "export const a = 1;\nexport function add(left: number) {\n  return left;\n}\n",
    );
    let head = repo.commit("head");
    let (mut review, source) = open_review(&repo, &base, &head);

    highlighted(&mut review, &source);

    let all = lines(&review);
    let signature = captures_in(line_with(&all, Side::Head, "function add"));
    assert!(
        signature.contains(&("left", Capture::VariableParameter)),
        "{signature:?}"
    );
}

#[test]
fn tsx_tags_are_coloured() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    repo.write("a.tsx", "export const a = 1;\n");
    let base = repo.commit("base");
    repo.write("a.tsx", "export const a = <Box />;\n");
    let head = repo.commit("head");
    let (mut review, source) = open_review(&repo, &base, &head);

    highlighted(&mut review, &source);

    let all = lines(&review);
    let element = captures_in(line_with(&all, Side::Head, "<Box"));
    assert!(
        element
            .iter()
            .any(|(text, capture)| *text == "Box" && *capture != Capture::Variable),
        "{element:?}"
    );
}

#[test]
fn removed_lines_of_a_renamed_file_are_coloured_from_its_old_path() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    repo.write("old.php", "<?php\nreturn 1;\nreturn 5;\n");
    let base = repo.commit("base");
    repo.git(&["mv", "old.php", "new.php"]);
    repo.write("new.php", "<?php\nreturn 1;\nreturn 6;\n");
    let head = repo.commit("head");
    let (mut review, source) = open_review(&repo, &base, &head);

    highlighted(&mut review, &source);

    let all = lines(&review);
    let removed = captures_in(line_with(&all, Side::Base, "return 5"));
    assert!(removed.contains(&("5", Capture::Number)), "{removed:?}");
}

#[test]
fn files_of_unknown_languages_stay_plain() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    repo.write("notes.txt", "one\n");
    let base = repo.commit("base");
    repo.write("notes.txt", "two\n");
    let head = repo.commit("head");
    let (mut review, source) = open_review(&repo, &base, &head);

    highlighted(&mut review, &source);

    assert!(lines(&review).iter().all(|line| line.spans.is_empty()));
}

#[test]
fn other_common_languages_are_coloured_too() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    repo.write("a.json", "{}\n");
    repo.write("a.rs", "fn a() {}\n");
    repo.write("a.css", "a { }\n");
    repo.write("a.sh", "echo a\n");
    repo.write("a.yml", "a: 1\n");
    repo.write("a.js", "let a = 1;\n");
    let base = repo.commit("base");
    repo.write("a.json", "{\"key\": \"value\"}\n");
    repo.write("a.rs", "fn a() { let x = 1; }\n");
    repo.write("a.css", "a { color: red; }\n");
    repo.write("a.sh", "echo \"b\"\n");
    repo.write("a.yml", "a: \"b\"\n");
    repo.write("a.js", "let a = \"b\";\n");
    let head = repo.commit("head");
    let (mut review, source) = open_review(&repo, &base, &head);

    highlighted(&mut review, &source);

    for line in lines(&review)
        .iter()
        .filter(|l| l.location.side == Side::Head)
    {
        assert!(!line.spans.is_empty(), "{} is plain", line.location.path);
    }
}
