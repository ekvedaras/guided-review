use crate::support::FixtureRepo;
use guided_review::config::{AskEffortChoice, AskModelChoice, AskSettings};
use guided_review::ide::{IdeChoice, IdeTemplate};
use guided_review::summary_instructions::{SkillName, SummaryInstructionsChoice};
use guided_review::viewer::{CloneRow, CloneRowProblem, SaveOutcome, SettingProblem, Settings};
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
    assert_eq!(settings.ask_model(), AskModelChoice::Sonnet);

    settings.set_ask_model(&AskModelChoice::Opus);
    let outcome = settings.save().unwrap();

    assert_eq!(outcome, SaveOutcome::Written);
    assert_eq!(
        config_text(shop.home()),
        config.replace(r#"ask_model = "sonnet""#, r#"ask_model = "opus""#)
    );
}

#[test]
fn a_cleared_ide_removes_only_its_line() {
    let (shop, config, mut settings) = loaded_with_commented_config();

    settings.set_ide(&IdeChoice::None);
    settings.save().unwrap();

    assert_eq!(
        config_text(shop.home()),
        config.replace("ide = \"zed {path}:{line}\"\n", "")
    );
}

#[test]
fn a_new_ask_effort_is_added_among_the_top_keys_and_nothing_else_moves() {
    let (shop, config, mut settings) = loaded_with_commented_config();
    assert_eq!(settings.ask_effort(), AskEffortChoice::AppDefault);

    settings.set_ask_effort(&AskEffortChoice::High);
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

    settings.set_ask_model(&AskModelChoice::Opus);
    assert!(settings.has_unsaved_changes());
    settings.set_ask_model(&AskModelChoice::Sonnet);
    assert!(!settings.has_unsaved_changes());

    settings.set_ask_model(&AskModelChoice::Haiku);
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

    assert_eq!(outcome, SaveOutcome::Written);
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
            clone: CloneRow::new("acme/blog", shop.home().join("gone")),
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
            clone: CloneRow::new("acme/wiki", blog.path().to_path_buf()),
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
                clone: CloneRow::new("shop", shop.path().to_path_buf()),
                problem: CloneRowProblem::NotOwnerSlashRepo
            },
            SettingProblem::Clone {
                row: twice,
                clone: CloneRow::new("acme/shop", shop.path().to_path_buf()),
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
    assert_eq!(
        SummaryInstructionsChoice::skill_named("acme:warm-up"),
        SummaryInstructionsChoice::Skill(skill("acme:warm-up"))
    );
    assert_eq!(
        SummaryInstructionsChoice::skill_named("two words"),
        SummaryInstructionsChoice::Unreadable("{ skill = \"two words\" }".to_owned())
    );
}

fn loaded_from(config: &str) -> (tempfile::TempDir, Settings) {
    let home = tempfile::TempDir::new().unwrap();
    write_config(home.path(), config);
    let settings = Settings::load(home.path()).unwrap();
    (home, settings)
}

#[test]
fn summary_instructions_the_config_holds_wrongly_load_and_are_a_problem_on_that_setting() {
    let bad_skill = "summary_instructions = { skill = \"two words\" }\n";
    let (_home, settings) = loaded_from(bad_skill);
    assert_eq!(
        settings.check(),
        [SettingProblem::SummaryInstructionsUnreadable(
            "{ skill = \"two words\" }".to_owned()
        )]
    );

    let (_home, settings) = loaded_from("summary_instructions = 7\n");
    assert_eq!(
        settings.check(),
        [SettingProblem::SummaryInstructionsUnreadable(
            "7".to_owned()
        )]
    );
}

#[test]
fn save_is_refused_on_an_unreadable_summary_instructions_until_another_choice_is_picked() {
    let config = "ask_model = \"sonnet\"\nsummary_instructions = { skill = \"two words\" }\n";
    let (home, mut settings) = loaded_from(config);
    assert_eq!(settings.save().unwrap(), SaveOutcome::NothingChanged);

    settings.set_ask_model(&AskModelChoice::Haiku);
    let outcome = settings.save().unwrap();
    assert!(matches!(outcome, SaveOutcome::Refused(problems) if problems.len() == 1));
    assert_eq!(config_text(home.path()), config);

    settings.set_summary_instructions(SummaryInstructionsChoice::Off);
    assert_eq!(settings.save().unwrap(), SaveOutcome::Written);
    assert_eq!(
        config_text(home.path()),
        "ask_model = \"haiku\"\nsummary_instructions = \"off\"\n"
    );
}

#[test]
fn a_typed_skill_name_is_a_choice_or_a_problem() {
    let (_home, mut settings) = loaded_from("");

    settings.set_summary_instructions(SummaryInstructionsChoice::skill_named("plugin:skill"));
    assert_eq!(settings.check(), []);

    settings.set_summary_instructions(SummaryInstructionsChoice::skill_named("a,b"));
    assert_eq!(
        settings.check(),
        [SettingProblem::SummaryInstructionsUnreadable(
            "{ skill = \"a,b\" }".to_owned()
        )]
    );
}

fn custom_ide(template: &str) -> IdeChoice {
    IdeChoice::Custom(IdeTemplate::new(template.to_owned()))
}

#[test]
fn the_ide_reads_as_a_preset_none_or_custom() {
    let ide_of = |config: &str| loaded_from(config).1.ide();

    assert_eq!(ide_of(""), IdeChoice::None);
    assert_eq!(
        ide_of("ide = \"phpstorm://open?file={path}&line={line}\"\n"),
        IdeChoice::PhpStorm
    );
    assert_eq!(
        ide_of("ide = \"vscode://file{path}:{line}\"\n"),
        IdeChoice::VsCode
    );
    assert_eq!(
        ide_of("ide = \"zed://file{path}:{line}\"\n"),
        IdeChoice::Zed
    );
    assert_eq!(
        ide_of("ide = \"zed {path}:{line}\"\n"),
        custom_ide("zed {path}:{line}")
    );
}

#[test]
fn picking_an_ide_writes_its_template_and_none_removes_the_key() {
    let (home, mut settings) = loaded_from("ide = \"zed {path}:{line}\"\n");

    settings.set_ide(&IdeChoice::VsCode);
    settings.save().unwrap();
    assert_eq!(
        config_text(home.path()),
        "ide = \"vscode://file{path}:{line}\"\n"
    );

    settings.set_ide(&custom_ide("code --goto {path}:{line}"));
    settings.save().unwrap();
    assert_eq!(
        config_text(home.path()),
        "ide = \"code --goto {path}:{line}\"\n"
    );

    settings.set_ide(&IdeChoice::None);
    settings.save().unwrap();
    assert_eq!(config_text(home.path()), "");
}

#[test]
fn a_custom_ide_equal_to_a_preset_reads_back_as_that_preset() {
    let (_home, mut settings) = loaded_from("");

    settings.set_ide(&custom_ide("zed://file{path}:{line}"));

    assert_eq!(settings.ide(), IdeChoice::Zed);
}

#[test]
fn the_ask_model_reads_as_the_app_default_an_alias_or_custom() {
    let model_of = |config: &str| loaded_from(config).1.ask_model();

    assert_eq!(model_of(""), AskModelChoice::AppDefault);
    assert_eq!(model_of("ask_model = \"opus\"\n"), AskModelChoice::Opus);
    assert_eq!(model_of("ask_model = \"haiku\"\n"), AskModelChoice::Haiku);
    assert_eq!(
        model_of("ask_model = \"claude-opus-4-1\"\n"),
        AskModelChoice::Custom("claude-opus-4-1".to_owned())
    );
    assert_eq!(AskSettings::default().model, "opus");
}

#[test]
fn picking_the_app_default_model_removes_the_key_and_a_custom_one_is_written_as_typed() {
    let (home, mut settings) = loaded_from("ask_model = \"sonnet\"\n");

    settings.set_ask_model(&AskModelChoice::Custom("claude-opus-4-1".to_owned()));
    settings.save().unwrap();
    assert_eq!(
        config_text(home.path()),
        "ask_model = \"claude-opus-4-1\"\n"
    );

    settings.set_ask_model(&AskModelChoice::AppDefault);
    settings.save().unwrap();
    assert_eq!(config_text(home.path()), "");
}

#[test]
fn the_ask_effort_is_the_app_default_or_one_of_the_list_and_is_written_as_its_word() {
    let (home, mut settings) = loaded_from("ask_effort = \"xhigh\"\n");
    assert_eq!(settings.ask_effort(), AskEffortChoice::XHigh);
    assert_eq!(AskSettings::default().effort, "medium");

    settings.set_ask_effort(&AskEffortChoice::Max);
    settings.save().unwrap();
    assert_eq!(config_text(home.path()), "ask_effort = \"max\"\n");

    settings.set_ask_effort(&AskEffortChoice::AppDefault);
    settings.save().unwrap();
    assert_eq!(config_text(home.path()), "");
}

#[test]
fn an_ask_effort_outside_the_list_loads_and_blocks_the_save_until_another_is_picked() {
    let config = "ask_effort = \"turbo\"\n";
    let (home, mut settings) = loaded_from(config);
    assert_eq!(
        settings.ask_effort(),
        AskEffortChoice::Unknown("turbo".to_owned())
    );
    assert_eq!(
        settings.check(),
        [SettingProblem::AskEffortUnknown("turbo".to_owned())]
    );

    settings.set_ask_model(&AskModelChoice::Haiku);
    assert!(matches!(settings.save().unwrap(), SaveOutcome::Refused(_)));
    assert_eq!(config_text(home.path()), config);

    settings.set_ask_effort(&AskEffortChoice::Low);
    assert_eq!(settings.save().unwrap(), SaveOutcome::Written);
    assert_eq!(
        config_text(home.path()),
        "ask_effort = \"low\"\nask_model = \"haiku\"\n"
    );
}

#[test]
fn each_problem_words_the_red_line_under_its_setting() {
    let clone = |repo: &str| CloneRow::new(repo, PathBuf::from("/code/shop"));
    let clone_problem = |repo: &str, problem| SettingProblem::Clone {
        row: 0,
        clone: clone(repo),
        problem,
    };

    assert_eq!(
        clone_problem("acme/shop", CloneRowProblem::FolderGone).message(),
        "This folder is gone"
    );
    assert_eq!(
        clone_problem("acme/shop", CloneRowProblem::NoRemoteOfRepo).message(),
        "No remote of this clone points at acme/shop"
    );
    assert_eq!(
        clone_problem("shop", CloneRowProblem::NotOwnerSlashRepo).message(),
        "Write the repo as owner/repo"
    );
    assert_eq!(
        clone_problem("acme/shop", CloneRowProblem::SameRepoTwice).message(),
        "acme/shop is set twice"
    );
    assert_eq!(
        CloneRowProblem::FolderGone.message(&clone("acme/shop")),
        "This folder is gone"
    );
    assert_eq!(
        SettingProblem::SummaryFileGone(PathBuf::from("/notes/summary.md")).message(),
        "The summary file /notes/summary.md is gone"
    );
    assert_eq!(
        SettingProblem::SummaryInstructionsUnreadable("{ skill = \"two words\" }".to_owned())
            .message(),
        "{ skill = \"two words\" } is not a file, \"off\" or a skill name"
    );
    assert_eq!(
        SettingProblem::AskEffortUnknown("turbo".to_owned()).message(),
        "\"turbo\" is not an effort Claude accepts: low, medium, high, xhigh or max"
    );
}

#[test]
fn a_skill_folder_whose_name_is_not_a_skill_name_is_not_listed() {
    let home = tempfile::tempdir().unwrap();
    let folder = home.path().join(".claude/skills/two words");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("SKILL.md"), "").unwrap();

    assert_eq!(Settings::load(home.path()).unwrap().skills(), Vec::new());
}

#[test]
fn a_blank_custom_ask_model_is_refused_with_a_message_and_nothing_is_written() {
    let (home, mut settings) = loaded_from("ask_model = \"sonnet\"\n");

    settings.set_ask_model(&AskModelChoice::Custom("  ".to_owned()));
    let outcome = settings.save().unwrap();

    let SaveOutcome::Refused(problems) = outcome else {
        panic!("expected a refusal, got {outcome:?}");
    };
    assert_eq!(problems, vec![SettingProblem::AskModelBlank]);
    assert_eq!(
        problems[0].message(),
        "Type the model name, or pick another choice"
    );
    assert_eq!(config_text(home.path()), "ask_model = \"sonnet\"\n");
}

#[test]
fn a_blank_custom_ide_template_is_refused_with_a_message_and_nothing_is_written() {
    let (home, mut settings) = loaded_from("ide = \"zed://file{path}:{line}\"\n");

    settings.set_ide(&IdeChoice::Custom(IdeTemplate::new(String::new())));
    let outcome = settings.save().unwrap();

    let SaveOutcome::Refused(problems) = outcome else {
        panic!("expected a refusal, got {outcome:?}");
    };
    assert_eq!(problems, vec![SettingProblem::IdeTemplateBlank]);
    assert_eq!(
        problems[0].message(),
        "Type the command or link, with {path} and {line}, or pick another choice"
    );
    assert_eq!(
        config_text(home.path()),
        "ide = \"zed://file{path}:{line}\"\n"
    );
}

#[test]
fn creating_the_default_file_makes_an_empty_one_and_keeps_one_that_is_there() {
    let (home, settings) = loaded_from("");
    let file = home.path().join(".guided-review/summary.md");
    assert!(!settings.default_file_exists());

    assert_eq!(settings.create_default_file().unwrap(), file);
    assert!(settings.default_file_exists());
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "");

    std::fs::write(&file, "Warm me up").unwrap();
    settings.create_default_file().unwrap();
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "Warm me up");
}

#[test]
fn the_ask_model_choices_to_pick_are_the_aliases_and_each_reads_back_as_itself() {
    let words: Vec<_> = AskModelChoice::PICKABLE
        .iter()
        .map(|choice| choice.config_text().unwrap())
        .collect();

    assert_eq!(words, ["opus", "sonnet", "haiku"]);
    for choice in AskModelChoice::PICKABLE {
        assert_eq!(AskModelChoice::of(choice.config_text()), choice);
    }
}

#[test]
fn a_custom_text_box_opens_on_the_text_of_a_custom_choice_and_only_then() {
    let (_home, settings) = loaded_from(
        "ide = \"zed {path}:{line}\"\nask_model = \"claude-opus-4-1\"\nsummary_instructions = { skill = \"recap\" }\n",
    );
    assert_eq!(settings.ide().custom_text(), Some("zed {path}:{line}"));
    assert_eq!(settings.ask_model().custom_text(), Some("claude-opus-4-1"));
    assert_eq!(settings.summary_instructions().skill_text(), Some("recap"));

    let (_home, settings) = loaded_from("ask_model = \"opus\"\nsummary_instructions = \"off\"\n");
    assert_eq!(settings.ide().custom_text(), None);
    assert_eq!(settings.ask_model().custom_text(), None);
    assert_eq!(settings.summary_instructions().skill_text(), None);
}

#[test]
fn settings_opens_showing_only_the_problems_the_config_already_has_in_it() {
    let (_home, settings) =
        loaded_from("summary_instructions = 5\nask_effort = \"turbo\"\nide = \"zed\"\n");
    assert_eq!(
        settings.problems_on_open(),
        [
            SettingProblem::AskEffortUnknown("turbo".to_owned()),
            SettingProblem::SummaryInstructionsUnreadable("5".to_owned()),
        ]
    );

    let (_home, settings) = loaded_from(
        "summary_instructions = \"/gone/summary.md\"\n[clones]\n\"acme\" = \"/gone\"\n",
    );
    assert_eq!(settings.problems_on_open(), []);
}

#[test]
fn a_clone_row_with_no_folder_says_so() {
    assert_eq!(
        CloneRow::new("acme/shop", PathBuf::new()).folder_label(),
        "No folder yet"
    );
    assert_eq!(
        CloneRow::new("acme/shop", PathBuf::from("~/Code/shop")).folder_label(),
        "~/Code/shop"
    );
}
