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

#[allow(
    dead_code,
    reason = "each test binary uses only part of the shared test support"
)]
mod support;

use support::{FakeGitHub, FixtureRepo, GuidedReview, png_of, svg_of};

const PR: &str = "https://github.com/acme/shop/pull/7";

fn outline_files(repo: &FixtureRepo, base: &str, head: &str) -> String {
    let github = FakeGitHub::serving_pull("acme/shop", 7, base, head);
    let run = GuidedReview::new(repo, &github).run(&["outline", PR]);
    let outline = run.stdout_with(&[(base, "<base>"), (head, "<head>")]);
    let (_, files) = outline
        .split_once("\n\n")
        .expect("a blank line before files");
    files.to_owned()
}

#[test]
fn a_changed_image_is_an_image_piece_with_its_size_before_and_after() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    repo.write_bytes("assets/logo.png", &png_of(64, 64, [255, 0, 0, 255]));
    let base = repo.commit("base");
    repo.write_bytes("assets/logo.png", &png_of(128, 96, [0, 255, 0, 255]));
    let head = repo.commit("head");

    assert_eq!(
        outline_files(&repo, &base, &head),
        "\
assets/logo.png  modified  +0 −0
  assets/logo.png  image  modified  +0 −0  64×64 → 128×96
"
    );
}

#[test]
fn an_added_image_shows_its_head_size_and_a_deleted_one_its_base_size() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    repo.write_bytes("old.png", &png_of(8, 4, [0, 0, 0, 255]));
    let base = repo.commit("base");
    std::fs::remove_file(repo.path().join("old.png")).unwrap();
    repo.write_bytes("new.png", &png_of(32, 16, [0, 0, 0, 255]));
    let head = repo.commit("head");

    assert_eq!(
        outline_files(&repo, &base, &head),
        "\
new.png  added  +0 −0
  new.png  image  added  +0 −0  32×16

old.png  deleted  +0 −0
  old.png  image  deleted  +0 −0  8×4
"
    );
}

#[test]
fn an_svg_is_an_image_piece_whose_text_lines_stay_countable() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    repo.write("icon.svg", &svg_of(24, 24));
    let base = repo.commit("base");
    repo.write("icon.svg", &svg_of(48, 32));
    let head = repo.commit("head");

    assert_eq!(
        outline_files(&repo, &base, &head),
        "\
icon.svg  modified  +2 −2
  icon.svg  image  modified  +2 −2  24×24 → 48×32
"
    );
}

#[test]
fn an_image_that_cannot_be_read_is_still_an_image_piece_without_a_size() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    repo.write_bytes("broken.png", &[0, 159, 146, 150, 0, 1]);
    let base = repo.commit("base");
    repo.write_bytes("broken.png", &[0, 159, 146, 150, 0, 2]);
    let head = repo.commit("head");

    assert_eq!(
        outline_files(&repo, &base, &head),
        "\
broken.png  modified  +0 −0
  broken.png  image  modified  +0 −0
"
    );
}
