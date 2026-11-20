use guided_review::instructions::InstructionsKind;
use guided_review::viewer::{SettingProblem, SettingsPage};
use std::path::PathBuf;

#[test]
fn the_pages_are_listed_agents_first_and_about_last() {
    let titles: Vec<_> = SettingsPage::ALL.iter().map(|page| page.title()).collect();

    assert_eq!(titles, ["Agents", "Code", "Pull requests", "Keys", "About"]);
}

#[test]
fn each_problem_belongs_to_the_page_that_holds_its_setting() {
    let cases = [
        (
            SettingProblem::InstructionsFileGone(InstructionsKind::Review, PathBuf::from("/gone")),
            SettingsPage::Agents,
        ),
        (
            SettingProblem::AskEffortUnknown("huge".to_owned()),
            SettingsPage::Agents,
        ),
        (SettingProblem::AskModelBlank, SettingsPage::Agents),
        (SettingProblem::IdeTemplateBlank, SettingsPage::Code),
        (
            SettingProblem::MoreLinesNotACount("lots".to_owned()),
            SettingsPage::Code,
        ),
    ];

    for (problem, page) in cases {
        assert_eq!(SettingsPage::of(&problem), page, "{problem:?}");
    }
}

#[test]
fn a_refused_save_stays_on_the_page_when_it_has_a_problem() {
    let problems = [
        SettingProblem::IdeTemplateBlank,
        SettingProblem::AskModelBlank,
    ];

    assert_eq!(
        SettingsPage::Code.after_refusal(&problems),
        SettingsPage::Code
    );
}

#[test]
fn a_refused_save_moves_to_the_first_page_with_a_problem_when_the_shown_one_has_none() {
    let problems = [
        SettingProblem::IdeTemplateBlank,
        SettingProblem::AskModelBlank,
    ];

    assert_eq!(
        SettingsPage::About.after_refusal(&problems),
        SettingsPage::Agents
    );
}

#[test]
fn with_no_problems_the_page_stays() {
    assert_eq!(SettingsPage::Keys.after_refusal(&[]), SettingsPage::Keys);
}
