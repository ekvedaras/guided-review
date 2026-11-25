use crate::support::FixtureRepo;
use crate::support::folders_in;
use guided_review::codebase::Codebase;
use guided_review::config::{AskEffortChoice, AskModelChoice, AskSettings, Config};
use guided_review::ide::{IdeChoice, IdeTemplate};
use guided_review::instructions::{InstructionsChoice, InstructionsKind, SkillName};
use guided_review::navigation::ServerAnswer;
use guided_review::pull_request::RepoName;
use guided_review::review_subject::WorkTreeRoot;
use guided_review::viewer::{
    Chord, CloneRow, CloneRowProblem, Command, KeyProblem, LanguageServersRow, MergeMethod,
    SaveOutcome, SendAction, SettingProblem, Settings,
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
    let path = home.join(".config/guided-review/config.toml");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn config_text(home: &Path) -> String {
    std::fs::read_to_string(home.join(".config/guided-review/config.toml")).unwrap()
}

#[test]
fn saving_with_no_edits_leaves_the_config_byte_for_byte() {
    let shop = FixtureRepo::cloned_from("acme/shop");
    let config = commented_config(&shop);
    write_config(shop.home(), &config);
    let mut settings = Settings::load(&shop.folders()).unwrap();

    let outcome = settings.save().unwrap();

    assert_eq!(outcome, SaveOutcome::NothingChanged);
    assert_eq!(config_text(shop.home()), config);
}

fn loaded_with_commented_config() -> (FixtureRepo, String, Settings) {
    let shop = FixtureRepo::cloned_from("acme/shop");
    let config = commented_config(&shop);
    write_config(shop.home(), &config);
    let settings = Settings::load(&shop.folders()).unwrap();
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
fn a_cleared_ide_removes_only_its_line_and_keeps_its_template_as_ide_custom() {
    let (shop, config, mut settings) = loaded_with_commented_config();

    settings.set_ide(&IdeChoice::None);
    settings.save().unwrap();

    assert_eq!(
        config_text(shop.home()),
        config.replace("ide = \"zed {path}:{line}\"\n", "").replace(
            "# quick answers\n",
            "# quick answers\nide_custom = \"zed {path}:{line}\"\n"
        )
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

fn summary_instructions_from(config: &str) -> InstructionsChoice {
    let home = tempfile::TempDir::new().unwrap();
    write_config(home.path(), config);
    Settings::load(&folders_in(home.path()))
        .unwrap()
        .instructions(InstructionsKind::Summary)
        .clone()
}

#[test]
fn summary_instructions_are_read_as_the_default_file_another_file_a_skill_or_off() {
    assert_eq!(
        summary_instructions_from(""),
        InstructionsChoice::DefaultFile
    );
    assert_eq!(
        summary_instructions_from("summary_instructions = \"/notes/summary.md\"\n"),
        InstructionsChoice::OtherFile(PathBuf::from("/notes/summary.md"))
    );
    assert_eq!(
        summary_instructions_from("summary_instructions = { skill = \"warm-up\" }\n"),
        InstructionsChoice::Skill(skill("warm-up"))
    );
    assert_eq!(
        summary_instructions_from("summary_instructions = \"off\"\n"),
        InstructionsChoice::Off
    );
}

#[test]
fn a_skill_is_written_as_a_table_so_it_is_never_read_as_a_path() {
    let (shop, config, mut settings) = loaded_with_commented_config();

    settings.set_instructions(
        InstructionsKind::Summary,
        InstructionsChoice::Skill(skill("acme:warm-up")),
    );
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

    settings.set_instructions(InstructionsKind::Summary, InstructionsChoice::Off);
    settings.save().unwrap();
    assert!(
        config_text(shop.home()).contains("\nsummary_instructions = \"off\"\n"),
        "{}",
        config_text(shop.home())
    );

    settings.set_instructions(InstructionsKind::Summary, InstructionsChoice::DefaultFile);
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
    let mut settings = Settings::load(&shop.folders()).unwrap();

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

    settings.set_instructions(
        InstructionsKind::Summary,
        InstructionsChoice::OtherFile(gone.clone()),
    );

    assert_eq!(
        refused(&mut settings, &shop, &config),
        [SettingProblem::InstructionsFileGone(
            InstructionsKind::Summary,
            gone
        )]
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

    let settings = Settings::load(&folders_in(home.path())).unwrap();

    assert_eq!(settings.skills(), [skill("linked"), skill("real")]);
}

#[test]
fn the_default_file_choice_knows_whether_the_default_file_exists() {
    let (shop, _config, _settings) = loaded_with_commented_config();
    assert!(
        !Settings::load(&shop.folders())
            .unwrap()
            .default_file_exists(InstructionsKind::Summary)
    );

    std::fs::write(
        shop.home().join(".config/guided-review/summary.md"),
        "Warm me up",
    )
    .unwrap();

    assert!(
        Settings::load(&shop.folders())
            .unwrap()
            .default_file_exists(InstructionsKind::Summary)
    );
}

#[test]
fn a_skill_name_outline_would_reject_is_not_a_skill_name_here_either() {
    for name in ["two words", "a,b", "a(b)", ""] {
        assert!(SkillName::parse(name).is_err(), "{name:?}");
    }
    assert_eq!(
        InstructionsChoice::skill_named("acme:warm-up"),
        InstructionsChoice::Skill(skill("acme:warm-up"))
    );
    assert_eq!(
        InstructionsChoice::skill_named("two words"),
        InstructionsChoice::Unreadable("{ skill = \"two words\" }".to_owned())
    );
}

fn loaded_from(config: &str) -> (tempfile::TempDir, Settings) {
    let home = tempfile::TempDir::new().unwrap();
    write_config(home.path(), config);
    let settings = Settings::load(&folders_in(home.path())).unwrap();
    (home, settings)
}

#[test]
fn summary_instructions_the_config_holds_wrongly_load_and_are_a_problem_on_that_setting() {
    let bad_skill = "summary_instructions = { skill = \"two words\" }\n";
    let (_home, settings) = loaded_from(bad_skill);
    assert_eq!(
        settings.check(),
        [SettingProblem::InstructionsUnreadable(
            InstructionsKind::Summary,
            "{ skill = \"two words\" }".to_owned()
        )]
    );

    let (_home, settings) = loaded_from("summary_instructions = 7\n");
    assert_eq!(
        settings.check(),
        [SettingProblem::InstructionsUnreadable(
            InstructionsKind::Summary,
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

    settings.set_instructions(InstructionsKind::Summary, InstructionsChoice::Off);
    assert_eq!(settings.save().unwrap(), SaveOutcome::Written);
    assert_eq!(
        config_text(home.path()),
        "ask_model = \"haiku\"\nsummary_instructions = \"off\"\n"
    );
}

#[test]
fn a_typed_skill_name_is_a_choice_or_a_problem() {
    let (_home, mut settings) = loaded_from("");

    settings.set_instructions(
        InstructionsKind::Summary,
        InstructionsChoice::skill_named("plugin:skill"),
    );
    assert_eq!(settings.check(), []);

    settings.set_instructions(
        InstructionsKind::Summary,
        InstructionsChoice::skill_named("a,b"),
    );
    assert_eq!(
        settings.check(),
        [SettingProblem::InstructionsUnreadable(
            InstructionsKind::Summary,
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
        ide_of("ide = \"idea://open?file={path}&line={line}\"\n"),
        IdeChoice::JetBrains
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
    let (home, mut settings) = loaded_from("");

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
    assert_eq!(
        config_text(home.path()),
        "ide_custom = \"code --goto {path}:{line}\"\n"
    );
}

#[test]
fn the_last_custom_ide_comes_back_when_custom_is_picked_again() {
    let (_home, mut settings) = loaded_from("ide = \"zed {path}:{line}\"\n");
    assert_eq!(settings.ide_custom_text(), "zed {path}:{line}");

    settings.set_ide(&IdeChoice::Zed);

    assert_eq!(settings.ide_custom_text(), "zed {path}:{line}");
}

#[test]
fn a_custom_ide_survives_save_with_a_preset_in_ide_custom_beside_ide() {
    let (home, mut settings) =
        loaded_from("ide = \"zed {path}:{line}\" # mine\nask_effort = \"low\"\n");

    settings.set_ide(&IdeChoice::VsCode);
    settings.save().unwrap();

    let config = config_text(home.path());
    assert_eq!(
        config,
        "ide = \"vscode://file{path}:{line}\" # mine\nask_effort = \"low\"\nide_custom = \"zed {path}:{line}\"\n"
    );
    let reloaded = Settings::load(&folders_in(home.path())).unwrap();
    assert_eq!(reloaded.ide(), IdeChoice::VsCode);
    assert_eq!(reloaded.ide_custom_text(), "zed {path}:{line}");
}

#[test]
fn ide_custom_is_not_written_while_the_custom_ide_is_the_active_one_or_blank() {
    let (home, mut settings) =
        loaded_from("ide = \"zed://file{path}:{line}\"\nide_custom = \"zed {path}:{line}\"\n");

    settings.set_ide(&custom_ide("code {path}"));
    settings.save().unwrap();
    assert_eq!(config_text(home.path()), "ide = \"code {path}\"\n");

    settings.set_ide(&custom_ide("  "));
    settings.set_ide(&IdeChoice::Zed);
    settings.save().unwrap();
    assert_eq!(
        config_text(home.path()),
        "ide = \"zed://file{path}:{line}\"\n"
    );
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
    assert_eq!(
        config_text(home.path()),
        "ask_model_custom = \"claude-opus-4-1\"\n"
    );
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
        SettingProblem::InstructionsFileGone(
            InstructionsKind::Summary,
            PathBuf::from("/notes/summary.md")
        )
        .message(),
        "The summary file /notes/summary.md is gone"
    );
    assert_eq!(
        SettingProblem::InstructionsUnreadable(
            InstructionsKind::Summary,
            "{ skill = \"two words\" }".to_owned()
        )
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

    assert_eq!(
        Settings::load(&folders_in(home.path())).unwrap().skills(),
        Vec::new()
    );
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
    let file = home.path().join(".config/guided-review/summary.md");
    assert!(!settings.default_file_exists(InstructionsKind::Summary));

    assert_eq!(
        settings
            .create_default_file(InstructionsKind::Summary)
            .unwrap(),
        file
    );
    assert!(settings.default_file_exists(InstructionsKind::Summary));
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "");

    std::fs::write(&file, "Warm me up").unwrap();
    settings
        .create_default_file(InstructionsKind::Summary)
        .unwrap();
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
    assert_eq!(
        settings
            .instructions(InstructionsKind::Summary)
            .skill_text(),
        Some("recap")
    );

    let (_home, settings) = loaded_from("ask_model = \"opus\"\nsummary_instructions = \"off\"\n");
    assert_eq!(settings.ide().custom_text(), None);
    assert_eq!(settings.ask_model().custom_text(), None);
    assert_eq!(
        settings
            .instructions(InstructionsKind::Summary)
            .skill_text(),
        None
    );
}

#[test]
fn settings_opens_showing_only_the_problems_the_config_already_has_in_it() {
    let (_home, settings) =
        loaded_from("summary_instructions = 5\nask_effort = \"turbo\"\nide = \"zed\"\n");
    assert_eq!(
        settings.problems_on_open(),
        [
            SettingProblem::AskEffortUnknown("turbo".to_owned()),
            SettingProblem::InstructionsUnreadable(InstructionsKind::Summary, "5".to_owned()),
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

#[test]
fn the_last_custom_ask_model_comes_back_and_survives_save_in_ask_model_custom() {
    let (home, mut settings) = loaded_from("ask_model = \"claude-opus-4-1\" # pinned\n");
    assert_eq!(settings.ask_model_custom_text(), "claude-opus-4-1");

    settings.set_ask_model(&AskModelChoice::Haiku);
    assert_eq!(settings.ask_model_custom_text(), "claude-opus-4-1");
    settings.save().unwrap();

    assert_eq!(
        config_text(home.path()),
        "ask_model = \"haiku\" # pinned\nask_model_custom = \"claude-opus-4-1\"\n"
    );
    let reloaded = Settings::load(&folders_in(home.path())).unwrap();
    assert_eq!(reloaded.ask_model(), AskModelChoice::Haiku);
    assert_eq!(reloaded.ask_model_custom_text(), "claude-opus-4-1");
}

#[test]
fn the_config_the_app_runs_on_ignores_the_custom_keys() {
    let config = "ide = \"zed://file{path}:{line}\"\nide_custom = \"x {path}\"\nask_model = \"haiku\"\nask_model_custom = \"m\"\n";

    assert_eq!(
        Config::from_text(config).unwrap().ask_settings().model,
        "haiku"
    );
}

#[test]
fn a_preset_shows_the_template_it_uses() {
    assert_eq!(
        IdeChoice::JetBrains.shown_template(),
        Some("idea://open?file={path}&line={line}")
    );
    assert_eq!(IdeChoice::None.shown_template(), None);
    assert_eq!(custom_ide("code {path}").shown_template(), None);
    assert_eq!(
        AskModelChoice::Sonnet.shown_command(),
        Some("claude --model sonnet".to_owned())
    );
    assert_eq!(AskModelChoice::AppDefault.shown_command(), None);
}

fn chord(text: &str) -> Chord {
    Chord::parse(text).unwrap()
}

const KEYED_CONFIG: &str = "ask_model = \"sonnet\"\n\n# my keys\n[keys]\nnext_screen = [\"space\", \"j\"] # like a pager\n";

#[test]
fn keys_in_the_config_load_as_each_command_s_keys_and_save_untouched() {
    let (home, mut settings) = loaded_from(KEYED_CONFIG);

    assert_eq!(
        settings.key_chords(Command::NextScreen),
        [chord("space"), chord("j")]
    );
    assert!(settings.is_key_changed(Command::NextScreen));
    assert_eq!(settings.key_chords(Command::WrapLines), [chord("w")]);
    assert!(!settings.is_key_changed(Command::WrapLines));
    assert_eq!(settings.save().unwrap(), SaveOutcome::NothingChanged);
    assert_eq!(config_text(home.path()), KEYED_CONFIG);
}

#[test]
fn a_new_key_is_written_under_keys_as_its_one_text_and_the_rest_keeps_its_comments() {
    let (home, mut settings) = loaded_from(KEYED_CONFIG);

    settings.set_key(Command::WrapLines, chord("shift-w"));
    assert_eq!(settings.key_chords(Command::WrapLines), [chord("shift-w")]);
    assert_eq!(settings.save().unwrap(), SaveOutcome::Written);

    assert_eq!(
        config_text(home.path()),
        "ask_model = \"sonnet\"\n\n# my keys\n[keys]\nnext_screen = [\"space\", \"j\"] # like a pager\nwrap_lines = \"shift-w\"\n"
    );
}

#[test]
fn a_key_set_back_to_its_default_or_reset_leaves_no_line() {
    let (home, mut settings) = loaded_from(KEYED_CONFIG);

    settings.set_key(Command::Jump, chord("cmd-k"));
    assert!(!settings.has_unsaved_changes());
    settings.reset_key(Command::NextScreen);
    assert_eq!(
        settings.key_chords(Command::NextScreen),
        [chord("right"), chord("j")]
    );
    settings.save().unwrap();

    assert_eq!(
        config_text(home.path()),
        "ask_model = \"sonnet\"\n\n# my keys\n[keys]\n"
    );
}

#[test]
fn a_key_another_command_answers_blocks_the_save_and_names_both() {
    let shop = FixtureRepo::cloned_from("acme/shop");
    write_config(shop.home(), KEYED_CONFIG);
    let mut settings = Settings::load(&shop.folders()).unwrap();

    settings.set_key(Command::WrapLines, chord("m"));

    assert_eq!(
        refused(&mut settings, &shop, KEYED_CONFIG),
        [SettingProblem::Key(KeyProblem::Clash {
            chord: chord("m"),
            commands: [Command::ToggleRead, Command::WrapLines],
        })]
    );
}

#[test]
fn keys_the_config_holds_wrongly_are_problems_on_open_until_reset_or_removed() {
    let (home, mut settings) = loaded_from("[keys]\nwrap = \"l\"\njump = \"cmd-\"\n");

    assert_eq!(
        settings.problems_on_open(),
        [
            SettingProblem::Key(KeyProblem::NotAKey {
                command: Command::Jump,
                written: "cmd-".to_owned()
            }),
            SettingProblem::Key(KeyProblem::UnknownCommand("wrap".to_owned())),
        ]
    );
    assert_eq!(settings.key_chords(Command::Jump), [chord("cmd-k")]);
    assert_eq!(settings.unknown_key_names(), ["wrap"]);

    settings.reset_key(Command::Jump);
    settings.remove_unknown_key("wrap");
    assert_eq!(settings.check(), []);
    assert_eq!(settings.save().unwrap(), SaveOutcome::Written);
    assert_eq!(config_text(home.path()), "[keys]\n");
}

#[test]
fn the_keymap_of_settings_is_the_keys_as_edited() {
    let (_home, mut settings) = loaded_from(KEYED_CONFIG);

    settings.set_key(Command::WrapLines, chord("shift-w"));

    assert_eq!(
        settings.keymap().chords(Command::WrapLines),
        [chord("shift-w")]
    );
    assert_eq!(
        settings.keymap().chords(Command::NextScreen),
        [chord("space"), chord("j")]
    );
}

#[test]
fn key_problems_follow_each_edit_before_any_save() {
    let (_home, mut settings) = loaded_from(KEYED_CONFIG);
    assert_eq!(settings.key_problems(), []);

    settings.set_key(Command::WrapLines, chord("m"));
    assert_eq!(
        settings.key_problems(),
        [KeyProblem::Clash {
            chord: chord("m"),
            commands: [Command::ToggleRead, Command::WrapLines],
        }]
    );

    settings.reset_key(Command::WrapLines);
    assert_eq!(settings.key_problems(), []);
}

#[test]
fn with_no_more_lines_key_a_step_shows_twenty_lines() {
    let (_home, settings) = loaded_from("");

    assert_eq!(settings.more_lines_text(), "20");
    assert!(!settings.has_unsaved_changes());
    assert_eq!(Config::from_text("").unwrap().more_lines().get(), 20);
}

#[test]
fn a_set_number_is_written_as_a_plain_integer_beside_the_rest() {
    let (home, mut settings) = loaded_from("ask_model = \"sonnet\" # quick\n");

    settings.set_more_lines_text("35");
    let outcome = settings.save().unwrap();

    assert_eq!(outcome, SaveOutcome::Written);
    assert_eq!(
        config_text(home.path()),
        "ask_model = \"sonnet\" # quick\nmore_lines = 35\n"
    );
    let config = Config::from_text(&config_text(home.path())).unwrap();
    assert_eq!(config.more_lines().get(), 35);
}

#[test]
fn the_number_set_back_to_twenty_or_blank_leaves_no_line() {
    for text in ["20", "", "  "] {
        let (home, mut settings) = loaded_from("more_lines = 35 # mine\nide = \"zed\"\n");
        assert_eq!(settings.more_lines_text(), "35");

        settings.set_more_lines_text(text);
        settings.save().unwrap();

        assert_eq!(config_text(home.path()), "ide = \"zed\"\n");
    }
}

#[test]
fn a_number_outside_one_to_two_hundred_or_not_whole_blocks_the_save_and_writes_nothing() {
    for text in ["0", "201", "-5", "2.5", "many", "1e2"] {
        let (home, mut settings) = loaded_from("ide = \"zed\"\n");

        settings.set_more_lines_text(text);

        let problem = SettingProblem::MoreLinesNotACount(text.to_owned());
        assert_eq!(settings.check(), vec![problem.clone()], "{text}");
        assert_eq!(
            settings.save().unwrap(),
            SaveOutcome::Refused(vec![problem])
        );
        assert_eq!(config_text(home.path()), "ide = \"zed\"\n");
    }
}

#[test]
fn the_edges_of_the_range_are_fine() {
    for text in ["1", "200"] {
        let (_home, mut settings) = loaded_from("");

        settings.set_more_lines_text(text);

        assert_eq!(settings.check(), []);
        assert_eq!(settings.more_lines_text(), text);
    }
}

#[test]
fn a_more_lines_the_config_holds_wrongly_is_a_problem_on_open_and_the_app_still_uses_twenty() {
    for written in ["\"many\"", "0", "500", "2.5", "[3]"] {
        let text = format!("more_lines = {written}\n");
        let (_home, settings) = loaded_from(&text);

        assert_eq!(
            settings.problems_on_open(),
            [SettingProblem::MoreLinesNotACount(written.to_owned())],
            "{written}"
        );
        assert_eq!(Config::from_text(&text).unwrap().more_lines().get(), 20);
    }
}

#[test]
fn the_problem_says_which_numbers_are_allowed() {
    let message = SettingProblem::MoreLinesNotACount("many".to_owned()).message();

    assert_eq!(message, "many is not a whole number from 1 to 200");
}

#[test]
fn with_no_merge_method_key_a_merge_commit_is_used_for_every_repo() {
    let (_home, settings) = loaded_from("");
    let config = Config::from_text("").unwrap();

    assert_eq!(settings.merge_method(), MergeMethod::Merge);
    assert_eq!(
        config.merge_method(&"acme/shop".parse().unwrap()),
        MergeMethod::Merge
    );
}

#[test]
fn a_repo_in_merge_methods_gets_its_own_and_the_others_the_global_one() {
    let config = Config::from_text(
        "merge_method = \"rebase\"\n\n[merge_methods]\n\"acme/shop\" = \"squash\"\n\"acme/bad\" = \"zip\"\n",
    )
    .unwrap();

    assert_eq!(
        config.merge_method(&"acme/shop".parse().unwrap()),
        MergeMethod::Squash
    );
    assert_eq!(
        config.merge_method(&"acme/other".parse().unwrap()),
        MergeMethod::Rebase
    );
    assert_eq!(
        config.merge_method(&"acme/bad".parse().unwrap()),
        MergeMethod::Rebase
    );
}

#[test]
fn a_chosen_merge_method_is_written_and_a_merge_commit_leaves_no_line() {
    let (home, mut settings) = loaded_from("ask_model = \"sonnet\" # quick\n");

    settings.set_merge_method(MergeMethod::Squash);
    settings.save().unwrap();
    assert_eq!(
        config_text(home.path()),
        "ask_model = \"sonnet\" # quick\nmerge_method = \"squash\"\n"
    );

    settings.set_merge_method(MergeMethod::Merge);
    settings.save().unwrap();
    assert_eq!(config_text(home.path()), "ask_model = \"sonnet\" # quick\n");
}

#[test]
fn chosen_primary_and_secondary_actions_are_written_and_the_defaults_leave_no_line() {
    let (home, mut settings) = loaded_from("ask_model = \"sonnet\" # quick\n");
    assert_eq!(settings.primary_comment_action(), SendAction::AddToReview);
    assert_eq!(settings.secondary_comment_action(), SendAction::AskAgent);

    settings.set_primary_comment_action(SendAction::CommentNow);
    settings.set_secondary_comment_action(SendAction::SuggestChange);
    assert_eq!(
        settings.keymap().chords(Command::CommentNow),
        Command::AddToReview.default_chords()
    );
    settings.save().unwrap();
    assert_eq!(
        config_text(home.path()),
        "ask_model = \"sonnet\" # quick\nprimary_comment_action = \"comment_now\"\nsecondary_comment_action = \"suggest_change\"\n"
    );

    settings.set_primary_comment_action(SendAction::AddToReview);
    settings.set_secondary_comment_action(SendAction::AskAgent);
    settings.save().unwrap();
    assert_eq!(config_text(home.path()), "ask_model = \"sonnet\" # quick\n");
}

#[test]
fn picking_the_secondarys_action_as_primary_swaps_them_and_both_are_written() {
    let (home, mut settings) = loaded_from("");

    settings.set_primary_comment_action(SendAction::AskAgent);
    settings.save().unwrap();

    assert_eq!(
        config_text(home.path()),
        "primary_comment_action = \"ask_agent\"\nsecondary_comment_action = \"add_to_review\"\n"
    );
    let (_, reloaded) = loaded_from(&config_text(home.path()));
    assert_eq!(reloaded.primary_comment_action(), SendAction::AskAgent);
    assert_eq!(reloaded.secondary_comment_action(), SendAction::AddToReview);
}

#[test]
fn a_key_equal_to_what_the_primary_action_gives_leaves_no_keys_line() {
    let (home, mut settings) = loaded_from("primary_comment_action = \"comment_now\"\n");

    settings.set_key(Command::CommentNow, Chord::parse("primary-j").unwrap());
    settings.set_key(Command::CommentNow, Chord::parse("primary-enter").unwrap());
    settings.save().unwrap();

    assert!(!settings.is_key_changed(Command::CommentNow));
    assert_eq!(
        config_text(home.path()),
        "primary_comment_action = \"comment_now\"\n"
    );
}

#[test]
fn review_instructions_are_read_saved_and_checked_apart_from_the_summary_ones() {
    let (home, mut settings) = loaded_from(
        "summary_instructions = \"off\"\nreview_instructions = { skill = \"audit\" }\n",
    );
    assert_eq!(
        settings.instructions(InstructionsKind::Review).skill_text(),
        Some("audit")
    );
    assert_eq!(
        settings.instructions(InstructionsKind::Summary),
        &InstructionsChoice::Off
    );

    settings.set_instructions(InstructionsKind::Review, InstructionsChoice::Off);
    settings.save().unwrap();

    assert_eq!(
        config_text(home.path()),
        "summary_instructions = \"off\"\nreview_instructions = \"off\"\n"
    );
    settings.set_instructions(InstructionsKind::Review, InstructionsChoice::DefaultFile);
    settings.save().unwrap();
    assert_eq!(config_text(home.path()), "summary_instructions = \"off\"\n");
}

#[test]
fn the_review_default_file_is_review_md_and_a_gone_review_file_is_a_problem_of_the_review_setting()
{
    let (home, mut settings) = loaded_from("");
    assert_eq!(
        settings.default_file(InstructionsKind::Review),
        home.path().join(".config/guided-review/review.md")
    );
    assert!(!settings.default_file_exists(InstructionsKind::Review));

    let gone = PathBuf::from("/notes/review.md");
    settings.set_instructions(
        InstructionsKind::Review,
        InstructionsChoice::OtherFile(gone.clone()),
    );

    assert_eq!(
        settings.check(),
        [SettingProblem::InstructionsFileGone(
            InstructionsKind::Review,
            gone
        )]
    );
    assert_eq!(
        SettingProblem::InstructionsUnreadable(InstructionsKind::Review, "7".to_owned()).message(),
        "7 is not a file, \"off\" or a skill name"
    );
}

#[test]
fn creating_the_review_default_file_leaves_the_summary_one_alone() {
    let (_home, settings) = loaded_from("");

    let file = settings
        .create_default_file(InstructionsKind::Review)
        .unwrap();

    assert!(file.ends_with("review.md"));
    assert!(settings.default_file_exists(InstructionsKind::Review));
    assert!(!settings.default_file_exists(InstructionsKind::Summary));
}

#[test]
fn an_unreadable_review_instructions_is_a_problem_on_open_and_refuses_save() {
    let (_home, mut settings) = loaded_from("review_instructions = 7\n");

    assert_eq!(
        settings.problems_on_open(),
        [SettingProblem::InstructionsUnreadable(
            InstructionsKind::Review,
            "7".to_owned()
        )]
    );
    settings.set_instructions(InstructionsKind::Review, InstructionsChoice::Off);
    assert_eq!(settings.save().unwrap(), SaveOutcome::Written);
}

fn github(owner: &str, name: &str) -> Codebase {
    Codebase::Hosted(guided_review::pull_request::HostedRepo {
        host: guided_review::pull_request::HostSite::GitHub,
        repo: RepoName::new(owner, name),
    })
}

#[test]
fn a_folder_with_no_github_origin_keeps_its_answer_under_its_path_beside_the_repos() {
    let home = tempfile::TempDir::new().unwrap();
    write_config(home.path(), "[language_servers]\n\"acme/shop\" = false\n");
    let folder = Codebase::Folder(WorkTreeRoot::new("/work/shop".into()).unwrap());

    Settings::remember_language_servers(&folders_in(home.path()), &folder, ServerAnswer::Yes)
        .unwrap();

    assert_eq!(
        config_text(home.path()),
        "[language_servers]\n\"acme/shop\" = false\n\"/work/shop\" = true\n"
    );
    let config = Config::load_from(&folders_in(home.path())).unwrap();
    assert_eq!(config.language_servers(&folder), Some(ServerAnswer::Yes));
    assert_eq!(
        config.language_servers(&github("acme", "shop")),
        Some(ServerAnswer::No)
    );
    let settings = Settings::load(&folders_in(home.path())).unwrap();
    assert_eq!(
        settings
            .language_servers()
            .iter()
            .map(|row| row.codebase.clone())
            .collect::<Vec<_>>(),
        [github("acme", "shop"), folder]
    );
}

#[test]
fn a_repo_answers_language_servers_with_what_the_config_holds_and_is_not_asked_without_it() {
    let config = Config::from_text(
        "[language_servers]\n\"acme/shop\" = true\n\"acme/old\" = false\n\"acme/bad\" = \"yes\"\n",
    )
    .unwrap();

    assert_eq!(
        config.language_servers(&github("acme", "shop")),
        Some(ServerAnswer::Yes)
    );
    assert_eq!(
        config.language_servers(&github("acme", "old")),
        Some(ServerAnswer::No)
    );
    assert_eq!(config.language_servers(&github("acme", "bad")), None);
    assert_eq!(config.language_servers(&github("acme", "new")), None);
}

#[test]
fn settings_list_the_repos_with_an_answer_and_the_others_are_not_listed() {
    let (_home, settings) = loaded_from(
        "[language_servers]\n\"acme/shop\" = true\n\"acme/old\" = false\n\"acme/bad\" = \"yes\"\n",
    );

    assert_eq!(
        settings.language_servers(),
        [
            LanguageServersRow {
                codebase: github("acme", "shop"),
                answer: ServerAnswer::Yes
            },
            LanguageServersRow {
                codebase: github("acme", "old"),
                answer: ServerAnswer::No
            },
        ]
    );
}

#[test]
fn changing_a_repos_answer_writes_that_entry_only_and_keeps_the_comments() {
    let (home, mut settings) = loaded_from(
        "ask_model = \"sonnet\" # quick\n\n# my servers\n[language_servers]\n\"acme/shop\" = true # trusted\n\"acme/old\" = false\n",
    );

    settings.set_language_servers(&github("acme", "shop"), ServerAnswer::No);
    assert!(settings.has_unsaved_changes());
    settings.save().unwrap();

    assert_eq!(
        config_text(home.path()),
        "ask_model = \"sonnet\" # quick\n\n# my servers\n[language_servers]\n\"acme/shop\" = false # trusted\n\"acme/old\" = false\n"
    );
}

#[test]
fn a_repo_answered_for_the_first_time_is_added_to_the_config() {
    let (home, mut settings) = loaded_from("ask_model = \"sonnet\" # quick\n");

    settings.set_language_servers(&github("acme", "shop"), ServerAnswer::Yes);
    settings.save().unwrap();

    assert_eq!(
        config_text(home.path()),
        "ask_model = \"sonnet\" # quick\n\n[language_servers]\n\"acme/shop\" = true\n"
    );
    assert_eq!(
        settings.language_servers(),
        [LanguageServersRow {
            codebase: github("acme", "shop"),
            answer: ServerAnswer::Yes
        }]
    );
}

#[test]
fn an_answer_given_on_the_card_is_saved_through_settings_with_the_comments_kept() {
    let home = tempfile::TempDir::new().unwrap();
    write_config(home.path(), "# mine\nide = \"zed {path}:{line}\" # fast\n");

    Settings::remember_language_servers(
        &folders_in(home.path()),
        &github("acme", "shop"),
        ServerAnswer::No,
    )
    .unwrap();

    assert_eq!(
        config_text(home.path()),
        "# mine\nide = \"zed {path}:{line}\" # fast\n\n[language_servers]\n\"acme/shop\" = false\n"
    );
}

#[test]
fn a_listed_repos_answer_is_changed_by_its_row_and_saved() {
    let (home, mut settings) =
        loaded_from("[language_servers]\n\"acme/shop\" = true\n\"acme/old\" = false\n");

    settings.change_language_servers(1, ServerAnswer::Yes);
    settings.change_language_servers(7, ServerAnswer::No);
    settings.save().unwrap();

    assert_eq!(
        config_text(home.path()),
        "[language_servers]\n\"acme/shop\" = true\n\"acme/old\" = true\n"
    );
}
