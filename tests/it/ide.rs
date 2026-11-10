use crate::support::line;
use std::path::PathBuf;

use crate::support::{FixtureRepo, file_path};
use guided_review::config::Config;
use guided_review::ide::{Ide, IdeChoice, IdeLaunch, NoIde};
use guided_review::local_git::LocalGit;
use guided_review::pull_request::RepoName;

fn ide(template: &str, clone: &str) -> Ide {
    let config = Config::from_text(&format!("ide = {template:?}")).unwrap();
    Ide::for_clone(Some(PathBuf::from(clone)), config.ide_template()).unwrap()
}

#[test]
fn a_url_template_gets_the_clone_path_and_line() {
    let ide = ide("idea://open?file={path}&line={line}", "/code/shop");

    assert_eq!(
        ide.launch(&file_path("app/Order.php"), line(7)),
        IdeLaunch::Url("idea://open?file=/code/shop/app/Order.php&line=7".into())
    );
}

#[test]
fn a_url_path_is_percent_encoded() {
    let ide = ide("idea://open?file={path}&line={line}", "/code/my shop");

    assert_eq!(
        ide.launch(&file_path("app/Order.php"), line(1)),
        IdeLaunch::Url("idea://open?file=/code/my%20shop/app/Order.php&line=1".into())
    );
}

#[test]
fn a_command_template_gets_a_shell_quoted_path() {
    let ide = ide("code --goto {path}:{line}", "/code/it's");

    assert_eq!(
        ide.launch(&file_path("a b.php"), line(3)),
        IdeLaunch::Shell("code --goto '/code/it'\\''s/a b.php':3".into())
    );
}

#[test]
fn without_a_template_or_a_local_clone_there_is_no_open_in_ide() {
    let no_template = Config::from_text("").unwrap();
    let with_template = Config::from_text("ide = \"idea://open?file={path}\"").unwrap();

    assert_eq!(
        Ide::for_clone(Some("/code/shop".into()), no_template.ide_template()).err(),
        Some(NoIde::NoTemplate)
    );
    assert_eq!(
        Ide::for_clone(None, with_template.ide_template()).err(),
        Some(NoIde::NoClone)
    );
}

#[test]
fn a_local_clone_says_where_it_is_and_the_github_source_says_it_has_none() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    let clone = LocalGit::find(&RepoName::new("acme", "shop"), repo.path())
        .unwrap()
        .unwrap();

    assert_eq!(
        guided_review::code_source::CodeSource::local_clone(&clone),
        Some(repo.path())
    );
    assert_eq!(
        guided_review::code_source::CodeSource::local_clone(
            &guided_review::github::GitHub::from_env().code_source(&RepoName::new("acme", "shop"))
        ),
        None
    );
}

#[test]
fn running_a_url_launch_hands_the_url_to_the_opener() {
    let ide = ide("idea://open?file={path}&line={line}", "/code/shop");
    let mut opened = Vec::new();

    ide.launch(&file_path("a.php"), line(2))
        .run(|url| opened.push(url.to_owned()))
        .unwrap();

    assert_eq!(opened, ["idea://open?file=/code/shop/a.php&line=2"]);
}

#[test]
fn each_ide_preset_opens_the_file_at_the_line_through_its_url_scheme() {
    let launch = |choice: IdeChoice| {
        Ide::for_clone(Some(PathBuf::from("/code/shop")), choice.template())
            .unwrap()
            .launch(&file_path("app/Order.php"), line(7))
    };

    assert_eq!(
        launch(IdeChoice::PhpStorm),
        IdeLaunch::Url("phpstorm://open?file=/code/shop/app/Order.php&line=7".into())
    );
    assert_eq!(
        launch(IdeChoice::VsCode),
        IdeLaunch::Url("vscode://file/code/shop/app/Order.php:7".into())
    );
    assert_eq!(
        launch(IdeChoice::Zed),
        IdeLaunch::Url("zed://file/code/shop/app/Order.php:7".into())
    );
}

#[test]
fn running_a_shell_launch_does_not_use_the_url_opener() {
    let ide = ide("true {path}:{line}", "/code/shop");

    ide.launch(&file_path("a.php"), line(2))
        .run(|_| panic!("a shell command is not a URL"))
        .unwrap();
}
