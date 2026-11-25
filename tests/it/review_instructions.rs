use crate::support::folders_in;
use guided_review::config::Config;
use guided_review::instructions::{ReviewInstructions, Reviewer, TourWriter};
use std::path::Path;
use tempfile::TempDir;

fn guided_review_file(home: &Path, name: &str, text: &str) {
    let path = home.join(".config/guided-review").join(name);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn review_instructions(home: &TempDir) -> ReviewInstructions {
    Config::read_instructions::<Reviewer>(&folders_in(home.path()))
}

#[test]
fn without_a_key_or_a_review_file_the_built_in_guidance_stands_in() {
    let home = TempDir::new().unwrap();

    let instructions = review_instructions(&home);

    let text = instructions.text().expect("built-in guidance");
    for severity in ["must fix", "should fix", "nit"] {
        assert!(text.contains(severity), "{severity} is not in {text}");
    }
    assert_eq!(instructions.skill(), None);
    assert_eq!(instructions.warning(), None);
}

#[test]
fn review_md_in_the_config_folder_replaces_the_built_in_guidance() {
    let home = TempDir::new().unwrap();
    guided_review_file(home.path(), "review.md", "Look at security only.\n");

    let instructions = review_instructions(&home);

    assert_eq!(instructions.text(), Some("Look at security only.\n"));
}

#[test]
fn a_blank_review_md_is_no_guidance_at_all() {
    let home = TempDir::new().unwrap();
    guided_review_file(home.path(), "review.md", "\n  \n");

    assert!(!review_instructions(&home).are_set());
}

#[test]
fn another_file_is_read_in_place_of_review_md() {
    let home = TempDir::new().unwrap();
    guided_review_file(home.path(), "review.md", "default file");
    guided_review_file(home.path(), "mine.md", "my file");
    guided_review_file(
        home.path(),
        "config.toml",
        &format!(
            "review_instructions = \"{}\"\n",
            home.path().join(".config/guided-review/mine.md").display()
        ),
    );

    assert_eq!(review_instructions(&home).text(), Some("my file"));
}

#[test]
fn a_skill_is_run_and_the_built_in_guidance_is_not_added() {
    let home = TempDir::new().unwrap();
    guided_review_file(
        home.path(),
        "config.toml",
        "review_instructions = { skill = \"audit\" }\n",
    );

    let instructions = review_instructions(&home);

    assert_eq!(instructions.skill(), Some("audit"));
    assert_eq!(instructions.text(), None);
}

#[test]
fn off_is_no_guidance_even_when_review_md_exists() {
    let home = TempDir::new().unwrap();
    guided_review_file(home.path(), "review.md", "guidance");
    guided_review_file(
        home.path(),
        "config.toml",
        "review_instructions = \"off\"\n",
    );

    let instructions = review_instructions(&home);

    assert!(!instructions.are_set());
    assert_eq!(instructions.warning(), None);
}

#[test]
fn a_review_key_that_is_none_of_the_choices_is_a_warning_naming_the_key() {
    let home = TempDir::new().unwrap();
    guided_review_file(home.path(), "config.toml", "review_instructions = 5\n");

    let instructions = review_instructions(&home);

    assert!(!instructions.are_set());
    assert!(
        instructions
            .warning()
            .is_some_and(|warning| warning.starts_with("review_instructions 5")),
        "{:?}",
        instructions.warning()
    );
}

#[test]
fn the_two_keys_are_read_apart() {
    let home = TempDir::new().unwrap();
    guided_review_file(
        home.path(),
        "config.toml",
        "review_instructions = \"off\"\nsummary_instructions = { skill = \"recap\" }\n",
    );

    let summary = Config::read_instructions::<TourWriter>(&folders_in(home.path()));

    assert_eq!(summary.skill(), Some("recap"));
    assert!(!review_instructions(&home).are_set());
}

#[test]
fn the_summary_has_no_built_in_guidance() {
    let home = TempDir::new().unwrap();

    assert!(!Config::read_instructions::<TourWriter>(&folders_in(home.path())).are_set());
}

#[test]
fn instructions_are_printed_under_their_own_name() {
    let home = TempDir::new().unwrap();
    guided_review_file(home.path(), "review.md", "Be brief.\n");

    assert_eq!(
        review_instructions(&home).to_string(),
        "\nreview instructions:\nBe brief.\n"
    );
}
