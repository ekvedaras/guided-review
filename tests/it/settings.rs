use crate::support::FixtureRepo;
use guided_review::summary_instructions::SkillName;
use guided_review::viewer::{
    CloneRow, CloneRowProblem, SaveOutcome, SettingProblem, Settings, SummaryInstructionsChoice,
};
use std::path::{Path, PathBuf};

fn commented_config(shop: &FixtureRepo) -> String {
    format!(
        r#"# my settings, by hand
ask_model = "sonnet" # quick answers
ide = "zed {{path}}:{{line}}"

# where my clones live
[clones]
"acme/shop" = "{}" # work laptop
"#,
        shop.path().display()
    )
}

fn skill(name: &str) -> SkillName {
    SkillName::parse(name).unwrap()
}

fn write_config(home: &Path, text: &str) {
    let path = home.join(".guided-review/config.toml");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn config_text(home: &Path) -> String {
    std::fs::read_to_string(home.join(".guided-review/config.toml")).unwrap()
}

#[test]
fn saving_with_no_edits_leaves_the_config_byte_for_byte() {
    let shop = FixtureRepo::cloned_from("acme/shop");
    let config = commented_config(&shop);
    write_config(shop.home(), &config);
    let mut settings = Settings::load(shop.home()).unwrap();

    let outcome = settings.save().unwrap();

    assert_eq!(outcome, SaveOutcome::NothingChanged);
    assert_eq!(config_text(shop.home()), config);
}

fn loaded_with_commented_config() -> (FixtureRepo, String, Settings) {
    let shop = FixtureRepo::cloned_from("acme/shop");
    let config = commented_config(&shop);
    write_config(shop.home(), &config);
    let settings = Settings::load(shop.home()).unwrap();
    (shop, config, settings)
}

#[test]
fn a_changed_ask_model_rewrites_only_its_value_and_keeps_the_comment_after_it() {
    let (shop, config, mut settings) = loaded_with_commented_config();
    assert_eq!(settings.ask_model(), Some("sonnet"));

    settings.set_ask_model(Some("opus".to_owned()));
    let outcome = settings.save().unwrap();

    assert_eq!(outcome, SaveOutcome::ReopenOpenPr);
    assert_eq!(
        config_text(shop.home()),
        config.replace(r#"ask_model = "sonnet""#, r#"ask_model = "opus""#)
    );
}

#[test]
fn a_cleared_ide_removes_only_its_line() {
    let (shop, config, mut settings) = loaded_with_commented_config();

    settings.set_ide(None);
    settings.save().unwrap();

    assert_eq!(
        config_text(shop.home()),
        config.replace("ide = \"zed {path}:{line}\"\n", "")
    );
}

#[test]
fn a_new_ask_effort_is_added_among_the_top_keys_and_nothing_else_moves() {
    let (shop, config, mut settings) = loaded_with_commented_config();
    assert_eq!(settings.ask_effort(), None);

    settings.set_ask_effort(Some("high".to_owned()));
    settings.save().unwrap();

    assert_eq!(
        config_text(shop.home()),
        config.replace(
            "ide = \"zed {path}:{line}\"\n",
            "ide = \"zed {path}:{line}\"\nask_effort = \"high\"\n"
        )
    );
}

#[test]
fn an_edit_is_unsaved_until_save_and_an_edit_back_to_the_saved_value_is_not() {
    let (_shop, _config, mut settings) = loaded_with_commented_config();
    assert!(!settings.has_unsaved_changes());

    settings.set_ask_model(Some("opus".to_owned()));
    assert!(settings.has_unsaved_changes());
    settings.set_ask_model(Some("sonnet".to_owned()));
    assert!(!settings.has_unsaved_changes());

    settings.set_ask_model(Some("haiku".to_owned()));
    settings.save().unwrap();
    assert!(!settings.has_unsaved_changes());
}

fn summary_instructions_from(config: &str) -> SummaryInstructionsChoice {
    let home = tempfile::TempDir::new().unwrap();
    write_config(home.path(), config);
    Settings::load(home.path())
        .unwrap()
        .summary_instructions()
        .clone()
}

#[test]
fn summary_instructions_are_read_as_the_default_file_another_file_a_skill_or_off() {
    assert_eq!(
        summary_instructions_from(""),
        SummaryInstructionsChoice::DefaultFile
    );
    assert_eq!(
        summary_instructions_from("summary_instructions = \"/notes/summary.md\"\n"),
        SummaryInstructionsChoice::OtherFile(PathBuf::from("/notes/summary.md"))
    );
    assert_eq!(
        summary_instructions_from("summary_instructions = { skill = \"warm-up\" }\n"),
        SummaryInstructionsChoice::Skill(skill("warm-up"))
    );
    assert_eq!(
        summary_instructions_from("summary_instructions = \"off\"\n"),
        SummaryInstructionsChoice::Off
    );
}

#[test]
fn a_skill_is_written_as_a_table_so_it_is_never_read_as_a_path() {
    let (shop, config, mut settings) = loaded_with_commented_config();

    settings.set_summary_instructions(SummaryInstructionsChoice::Skill(skill("acme:warm-up")));
    settings.save().unwrap();

    assert_eq!(
        config_text(shop.home()),
        config.replace(
            "ide = \"zed {path}:{line}\"\n",
            "ide = \"zed {path}:{line}\"\nsummary_instructions = { skill = \"acme:warm-up\" }\n"
        )
    );
}

#[test]
fn off_is_written_as_off_and_the_default_file_as_no_key() {
    let (shop, config, mut settings) = loaded_with_commented_config();

    settings.set_summary_instructions(SummaryInstructionsChoice::Off);
    settings.save().unwrap();
    assert!(
        config_text(shop.home()).contains("\nsummary_instructions = \"off\"\n"),
        "{}",
        config_text(shop.home())
    );

    settings.set_summary_instructions(SummaryInstructionsChoice::DefaultFile);
    settings.save().unwrap();
    assert_eq!(config_text(shop.home()), config);
}

#[test]
fn clones_are_rows_of_repo_and_folder() {
    let (shop, _config, settings) = loaded_with_commented_config();

    assert_eq!(
        settings.clones(),
        [CloneRow::new("acme/shop", shop.path().to_path_buf())]
    );
}

#[test]
fn an_added_clone_is_written_as_its_own_key_and_the_other_rows_keep_their_comments() {
    let (shop, config, mut settings) = loaded_with_commented_config();
    let blog = FixtureRepo::cloned_from("acme/blog");

    let row = settings.add_clone();
    settings.set_clone_repo(row, "acme/blog".to_owned());
    settings.set_clone_folder(row, blog.path().to_path_buf());
    let outcome = settings.save().unwrap();

    assert_eq!(outcome, SaveOutcome::ReopenOpenPr);
    assert_eq!(
        config_text(shop.home()),
        format!("{config}\"acme/blog\" = \"{}\"\n", blog.path().display())
    );
}

#[test]
fn a_removed_clone_takes_only_its_own_line() {
    let (shop, config, _) = loaded_with_commented_config();
    let blog = FixtureRepo::cloned_from("acme/blog");
    let with_blog = format!("{config}\"acme/blog\" = \"{}\"\n", blog.path().display());
    write_config(shop.home(), &with_blog);
    let mut settings = Settings::load(shop.home()).unwrap();

    settings.remove_clone(1);
    settings.save().unwrap();

    assert_eq!(config_text(shop.home()), config);
}

#[test]
fn picking_a_clone_folder_fills_the_repo_from_its_origin() {
    let (_shop, _config, mut settings) = loaded_with_commented_config();
    let blog = FixtureRepo::cloned_from("acme/blog");

    let row = settings.add_clone();
    settings.pick_clone_folder(row, blog.path().to_path_buf());

    assert_eq!(
        settings.clones().get(row),
        Some(&CloneRow::new("acme/blog", blog.path().to_path_buf()))
    );
}

fn refused(settings: &mut Settings, shop: &FixtureRepo, config: &str) -> Vec<SettingProblem> {
    let outcome = settings.save().unwrap();
    assert_eq!(config_text(shop.home()), config, "nothing is written");
    assert!(settings.has_unsaved_changes());
    let SaveOutcome::Refused(problems) = outcome else {
        panic!("save went through: {outcome:?}");
    };
    problems
}

#[test]
fn a_clone_folder_that_is_gone_blocks_the_save_and_names_its_row() {
    let (shop, config, mut settings) = loaded_with_commented_config();
    let gone = shop.home().join("gone");

    let row = settings.add_clone();
    settings.set_clone_repo(row, "acme/blog".to_owned());
    settings.set_clone_folder(row, gone);

    assert_eq!(
        refused(&mut settings, &shop, &config),
        [SettingProblem::Clone {
            row,
            problem: CloneRowProblem::FolderGone
        }]
    );
}

#[test]
fn a_clone_with_no_remote_pointing_at_its_repo_blocks_the_save_and_names_its_row() {
    let (shop, config, mut settings) = loaded_with_commented_config();
    let blog = FixtureRepo::cloned_from("acme/blog");

    let row = settings.add_clone();
    settings.set_clone_repo(row, "acme/wiki".to_owned());
    settings.set_clone_folder(row, blog.path().to_path_buf());

    assert_eq!(
        refused(&mut settings, &shop, &config),
        [SettingProblem::Clone {
            row,
            problem: CloneRowProblem::NoRemoteOfRepo
        }]
    );
}

#[test]
fn a_repo_that_is_not_owner_slash_repo_or_set_twice_blocks_the_save_and_names_its_row() {
    let (shop, config, mut settings) = loaded_with_commented_config();

    let unreadable = settings.add_clone();
    settings.set_clone_repo(unreadable, "shop".to_owned());
    settings.set_clone_folder(unreadable, shop.path().to_path_buf());
    let twice = settings.add_clone();
    settings.set_clone_repo(twice, "acme/shop".to_owned());
    settings.set_clone_folder(twice, shop.path().to_path_buf());

    assert_eq!(
        refused(&mut settings, &shop, &config),
        [
            SettingProblem::Clone {
                row: unreadable,
                problem: CloneRowProblem::NotOwnerSlashRepo
            },
            SettingProblem::Clone {
                row: twice,
                problem: CloneRowProblem::SameRepoTwice
            }
        ]
    );
}

#[test]
fn a_summary_file_that_is_gone_blocks_the_save() {
    let (shop, config, mut settings) = loaded_with_commented_config();
    let gone = shop.home().join("notes/summary.md");

    settings.set_summary_instructions(SummaryInstructionsChoice::OtherFile(gone.clone()));

    assert_eq!(
        refused(&mut settings, &shop, &config),
        [SettingProblem::SummaryFileGone(gone)]
    );
}

#[test]
fn the_skill_list_holds_the_skills_in_claude_skills_and_follows_symlinks() {
    let home = tempfile::TempDir::new().unwrap();
    let skills = home.path().join(".claude/skills");
    let elsewhere = home.path().join("dotfiles/linked");
    for skill in [skills.join("real"), elsewhere.clone()] {
        std::fs::create_dir_all(&skill).unwrap();
        std::fs::write(skill.join("SKILL.md"), "---\nname: x\n---\n").unwrap();
    }
    std::os::unix::fs::symlink(&elsewhere, skills.join("linked")).unwrap();
    std::fs::create_dir_all(skills.join("not-a-skill")).unwrap();
    std::fs::write(skills.join("notes.md"), "").unwrap();

    let settings = Settings::load(home.path()).unwrap();

    assert_eq!(settings.skills(), [skill("linked"), skill("real")]);
}

#[test]
fn the_default_file_choice_knows_whether_the_default_file_exists() {
    let (shop, _config, _settings) = loaded_with_commented_config();
    assert!(!Settings::load(shop.home()).unwrap().default_file_exists());

    std::fs::write(shop.home().join(".guided-review/summary.md"), "Warm me up").unwrap();

    assert!(Settings::load(shop.home()).unwrap().default_file_exists());
}

#[test]
fn a_skill_name_outline_would_reject_is_not_a_skill_name_here_either() {
    for name in ["two words", "a,b", "a(b)", ""] {
        assert!(SkillName::parse(name).is_err(), "{name:?}");
    }
    let home = tempfile::TempDir::new().unwrap();
    write_config(
        home.path(),
        "summary_instructions = { skill = \"two words\" }\n",
    );
    assert!(Settings::load(home.path()).is_err());
}

#[test]
fn a_skill_folder_whose_name_is_not_a_skill_name_is_not_listed() {
    let home = tempfile::tempdir().unwrap();
    let folder = home.path().join(".claude/skills/two words");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("SKILL.md"), "").unwrap();

    assert_eq!(Settings::load(home.path()).unwrap().skills(), Vec::new());
}
