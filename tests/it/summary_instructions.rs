use crate::support::{FakeGitHub, FixtureRepo, GuidedReview, Run};
use guided_review::config::Config;
use guided_review::tour::Tour;
use serde_json::{Value, json};
use std::path::Path;

const PR: &str = "https://github.com/acme/shop/pull/7";

struct Shop {
    repo: FixtureRepo,
    github: FakeGitHub,
}

impl Shop {
    fn with_pull() -> Self {
        let repo = FixtureRepo::cloned_from("acme/shop");
        repo.write("README.md", "hello\n");
        let base = repo.commit("base");
        repo.write("README.md", "hello\nworld\n");
        let head = repo.commit("head");
        let github = FakeGitHub::serving_pull("acme/shop", 7, &base, &head);
        Self { repo, github }
    }

    fn guided_review_file(&self, name: &str, text: &str) {
        let path = self.repo.home().join(".guided-review").join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn config(&self, text: &str) {
        self.guided_review_file("config.toml", text);
    }

    fn tour_path(&self) -> std::path::PathBuf {
        self.repo
            .home()
            .join(".guided-review/tours/acme/shop/7.json")
    }

    fn outline(&self) -> Run {
        GuidedReview::new(&self.repo, &self.github).run(&["outline", PR])
    }

    fn tour_with(&self, extra: &Value) {
        let mut tour = json!({
            "title": "t",
            "summary": ["Adds world"],
            "steps": [{"id": "readme", "title": "Readme", "why": "w", "pieces": [{"file": "README.md"}]}],
        });
        tour.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        self.guided_review_file("tours/acme/shop/7.json", &tour.to_string());
    }

    fn check(&self) -> Run {
        GuidedReview::new(&self.repo, &self.github).run(&["check", PR])
    }
}

fn instructions_block(run: &Run) -> Option<String> {
    run.stdout
        .split_once("summary instructions:\n")
        .map(|(_, rest)| rest.to_owned())
}

#[test]
fn outline_says_nothing_about_instructions_when_there_are_none() {
    let shop = Shop::with_pull();

    let run = shop.outline();

    assert!(run.success, "{}", run.stderr);
    assert!(
        !run.stdout.contains("summary instructions"),
        "{}",
        run.stdout
    );
}

#[test]
fn outline_ends_with_the_text_of_the_default_summary_file() {
    let shop = Shop::with_pull();
    shop.guided_review_file(
        "summary.md",
        "Open with the risk.\nThen the touched parts.\n",
    );

    let run = shop.outline();

    assert!(run.success, "{}", run.stderr);
    assert!(
        run.stdout
            .ends_with("\nsummary instructions:\nOpen with the risk.\nThen the touched parts.\n"),
        "{}",
        run.stdout
    );
}

#[test]
fn a_blank_default_file_gives_no_instructions() {
    let shop = Shop::with_pull();
    shop.guided_review_file("summary.md", "  \n");

    let run = shop.outline();

    assert!(
        !run.stdout.contains("summary instructions"),
        "{}",
        run.stdout
    );
}

#[test]
fn outline_reads_the_file_the_config_names_over_the_default_one() {
    let shop = Shop::with_pull();
    shop.guided_review_file("summary.md", "default text\n");
    let notes = shop.repo.home().join("notes/mine.md");
    std::fs::create_dir_all(notes.parent().unwrap()).unwrap();
    std::fs::write(&notes, "my own way\n").unwrap();
    shop.config(&format!("summary_instructions = \"{}\"\n", notes.display()));

    let run = shop.outline();

    assert_eq!(instructions_block(&run).as_deref(), Some("my own way\n"));
}

#[test]
fn a_configured_path_may_start_with_the_home_folder_mark() {
    let shop = Shop::with_pull();
    shop.guided_review_file("elsewhere.md", "from home\n");
    shop.config("summary_instructions = \"~/.guided-review/elsewhere.md\"\n");

    let run = shop.outline();

    assert_eq!(instructions_block(&run).as_deref(), Some("from home\n"));
}

#[test]
fn off_ignores_the_default_file() {
    let shop = Shop::with_pull();
    shop.guided_review_file("summary.md", "default text\n");
    shop.config("summary_instructions = \"off\"\n");

    let run = shop.outline();

    assert!(run.success, "{}", run.stderr);
    assert!(
        !run.stdout.contains("summary instructions"),
        "{}",
        run.stdout
    );
}

#[test]
fn the_eval_variable_turns_instructions_off_whatever_the_config_says() {
    let shop = Shop::with_pull();
    shop.guided_review_file("summary.md", "default text\n");

    let run = GuidedReview::new(&shop.repo, &shop.github)
        .with_env(Config::SUMMARY_INSTRUCTIONS_OFF_ENV, Path::new("1"))
        .run(&["outline", PR]);

    assert!(run.success, "{}", run.stderr);
    assert!(
        !run.stdout.contains("summary instructions"),
        "{}",
        run.stdout
    );
}

#[test]
fn a_missing_configured_file_is_one_warning_naming_the_path_and_outline_goes_on() {
    let shop = Shop::with_pull();
    shop.config("summary_instructions = \"/nowhere/summary.md\"\n");

    let run = shop.outline();

    assert!(run.success, "{}", run.stderr);
    let warnings = run
        .stdout
        .lines()
        .filter(|line| line.starts_with("warning: ") && line.contains("/nowhere/summary.md"))
        .count();
    assert_eq!(warnings, 1, "{}", run.stdout);
    assert!(
        !run.stdout.contains("summary instructions:"),
        "{}",
        run.stdout
    );
}

#[test]
fn check_warns_when_instructions_exist_but_the_tour_has_no_reviewer_summary() {
    let shop = Shop::with_pull();
    shop.guided_review_file("summary.md", "Open with the risk.\n");
    shop.tour_with(&json!({}));

    let run = shop.check();

    assert!(run.success, "{}", run.stderr);
    assert!(
        run.stdout
            .contains("warning: summary instructions are set but the tour has no reviewer summary"),
        "{}",
        run.stdout
    );
}

#[test]
fn check_is_quiet_when_the_tour_has_a_reviewer_summary() {
    let shop = Shop::with_pull();
    shop.guided_review_file("summary.md", "Open with the risk.\n");
    shop.tour_with(
        &json!({"reviewer_summary": "**Risk**: low.\n\n- a long, free list\n- of things\n- no limit"}),
    );

    let run = shop.check();

    assert!(run.success, "{}", run.stderr);
    assert!(
        run.stdout.contains("0 errors, 0 warnings"),
        "{}",
        run.stdout
    );
}

#[test]
fn check_is_quiet_when_there_are_no_instructions() {
    let shop = Shop::with_pull();
    shop.tour_with(&json!({}));

    let run = shop.check();

    assert!(
        run.stdout.contains("0 errors, 0 warnings"),
        "{}",
        run.stdout
    );
}

#[test]
fn check_is_quiet_when_the_instructions_file_is_broken() {
    let shop = Shop::with_pull();
    shop.config("summary_instructions = \"/nowhere/summary.md\"\n");
    shop.tour_with(&json!({}));

    let run = shop.check();

    assert!(
        run.stdout.contains("0 errors, 0 warnings"),
        "{}",
        run.stdout
    );
}

#[test]
fn a_tour_without_a_reviewer_summary_is_written_back_unchanged() {
    let shop = Shop::with_pull();
    shop.tour_with(&json!({}));

    shop.check();

    let text = std::fs::read_to_string(shop.tour_path()).unwrap();
    assert!(!text.contains("reviewer_summary"), "{text}");
}

#[test]
fn the_reviewer_summary_survives_a_read_and_write_of_the_tour_file() {
    let shop = Shop::with_pull();
    shop.tour_with(&json!({"reviewer_summary": "warm-up"}));
    let path = shop.tour_path();

    let tour = Tour::load(&path).unwrap();
    tour.save(&path).unwrap();

    assert_eq!(tour.reviewer_summary, "warm-up");
    assert!(
        std::fs::read_to_string(path)
            .unwrap()
            .contains("\"reviewer_summary\": \"warm-up\"")
    );
}
