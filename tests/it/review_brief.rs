use crate::support::{FakeGitHub, FixtureRepo, GuidedReview};

const PR: &str = "https://github.com/acme/shop/pull/7";

fn brief_with_config(config: Option<&str>) -> String {
    let repo = FixtureRepo::cloned_from("acme/shop");
    if let Some(config) = config {
        let path = repo.home().join(".config/guided-review/config.toml");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, config).unwrap();
    }
    let github = FakeGitHub::serving_pull("acme/shop", 7, "base", "head");
    GuidedReview::new(&repo, &github)
        .run(&["review-brief", PR])
        .stdout_with(&[])
}

#[test]
fn the_brief_gives_the_protocol_with_the_findings_path_to_write() {
    let brief = brief_with_config(None);

    assert!(
        brief.contains(&format!("guided-review check-findings {PR}")),
        "{brief}"
    );
    assert!(
        brief.contains(&format!("guided-review findings-done {PR}")),
        "{brief}"
    );
    assert!(
        brief.contains("/guided-review/tours/acme/shop/7.findings.json"),
        "{brief}"
    );
    assert!(brief.contains("Never change code"), "{brief}");
    assert!(brief.contains("never post on GitHub"), "{brief}");
    assert!(brief.contains("one command per Bash call"), "{brief}");
}

#[test]
fn the_brief_carries_the_built_in_guidance_when_nothing_is_set() {
    let brief = brief_with_config(None);

    assert!(brief.contains("review instructions:"), "{brief}");
    assert!(brief.contains("must fix"), "{brief}");
}

#[test]
fn a_review_skill_is_named_instead_of_guidance_text() {
    let brief = brief_with_config(Some("review_instructions = { skill = \"audit\" }\n"));

    assert!(brief.contains("run the skill audit"), "{brief}");
    assert!(!brief.contains("review instructions:\n"), "{brief}");
}

#[test]
fn off_leaves_only_the_protocol() {
    let brief = brief_with_config(Some("review_instructions = \"off\"\n"));

    assert!(!brief.contains("review instructions"), "{brief}");
    assert!(brief.contains("findings-done"), "{brief}");
}

#[test]
fn the_brief_has_a_slot_for_the_findings_the_reviewer_dismissed_that_says_none_yet() {
    let brief = brief_with_config(None);

    assert!(brief.contains("dismissed findings: none"), "{brief}");
}

#[test]
fn a_config_that_cannot_be_read_warns_on_stderr_and_the_brief_still_prints() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    let path = repo.home().join(".config/guided-review/config.toml");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, "review_instructions = [").unwrap();
    let github = FakeGitHub::serving_pull("acme/shop", 7, "base", "head");

    let run = GuidedReview::new(&repo, &github).run(&["review-brief", PR]);

    assert!(run.success, "{}", run.stderr);
    assert!(
        run.stderr.contains("warning: review instructions not read"),
        "{}",
        run.stderr
    );
    assert!(run.stdout.contains("findings-done"), "{}", run.stdout);
}
