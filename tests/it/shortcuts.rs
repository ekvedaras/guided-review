use std::collections::HashSet;

use gpui::Keystroke;
use guided_review::viewer::{Chord, Command, KeyScope, ShortcutGroup, in_group};

#[test]
fn every_command_is_listed_once_under_one_heading_of_the_sheet() {
    let listed: Vec<Command> = ShortcutGroup::ALL.into_iter().flat_map(in_group).collect();

    assert_eq!(listed.len(), Command::ALL.len());
    for command in Command::ALL {
        assert_eq!(
            listed.iter().filter(|shown| **shown == command).count(),
            1,
            "{command:?} is not listed exactly once"
        );
        assert!(!command.label().is_empty(), "{command:?} has no label");
    }
}

#[test]
fn every_heading_lists_something() {
    for group in ShortcutGroup::ALL {
        assert!(in_group(group).next().is_some(), "{group:?} is empty");
    }
}

#[test]
fn every_command_has_keys_gpui_can_bind() {
    for command in Command::ALL {
        assert!(
            !command.default_chords().is_empty(),
            "{command:?} has no keys"
        );
        for chord in command.default_chords() {
            let keys = chord.keystroke();
            Keystroke::parse(&keys).unwrap_or_else(|_| panic!("{command:?}: {keys} is not a key"));
        }
    }
}

#[test]
fn no_two_commands_of_one_scope_share_a_key() {
    let mut taken = HashSet::new();
    for command in Command::ALL {
        for chord in command.default_chords() {
            let scope = match command.scope() {
                KeyScope::Page => "page",
                KeyScope::Window => "window",
                KeyScope::TextBox => "text box",
                KeyScope::NotTyping => "outside text boxes",
                KeyScope::App => "app",
            };
            assert!(
                taken.insert((scope, chord.keystroke())),
                "{} is bound twice on the {scope}",
                chord.keystroke()
            );
        }
    }
}

#[test]
fn a_chord_is_written_as_gpui_reads_it() {
    assert_eq!(Command::Jump.default_chords()[0].keystroke(), "cmd-k");
    assert_eq!(
        Command::AskAgent.default_chords()[0].keystroke(),
        "cmd-shift-enter"
    );
    assert_eq!(Command::NextScreen.default_chords()[0].keystroke(), "right");
    assert_eq!(Command::Shortcuts.default_chords()[0].keystroke(), "?");
}

#[test]
fn the_keys_the_app_had_before_the_sheet_still_work() {
    let keys = |command: Command| -> Vec<String> {
        command
            .default_chords()
            .iter()
            .copied()
            .map(Chord::keystroke)
            .collect()
    };

    assert_eq!(keys(Command::NextScreen), ["right", "j"]);
    assert_eq!(keys(Command::PreviousScreen), ["left", "k"]);
    assert_eq!(keys(Command::WrapLines), ["w"]);
    assert_eq!(keys(Command::Jump), ["cmd-k"]);
    assert_eq!(keys(Command::Settings), ["cmd-,"]);
    assert_eq!(keys(Command::CopyCode), ["cmd-c"]);
    assert_eq!(keys(Command::AddToReview), ["cmd-enter"]);
    assert_eq!(keys(Command::CloseBox), ["escape"]);
}

#[test]
fn copying_code_works_with_a_comment_box_open_but_not_while_typing_in_it() {
    assert_eq!(Command::CopyCode.scope(), KeyScope::NotTyping);
}

#[test]
fn cmd_w_closes_the_window_from_anywhere_in_it() {
    assert_eq!(
        Command::CloseWindow.default_chords()[0].keystroke(),
        "cmd-w"
    );
    assert_eq!(Command::CloseWindow.scope(), KeyScope::Window);
}

#[test]
fn g_opens_the_pull_request_on_github_wherever_the_page_takes_keys() {
    assert_eq!(Command::OpenOnGitHub.default_chords()[0].keystroke(), "g");
    assert_eq!(Command::OpenOnGitHub.scope(), KeyScope::Page);
    assert_eq!(Command::OpenOnGitHub.group(), ShortcutGroup::App);
}

#[test]
fn cmd_q_quits_from_any_window_of_the_app() {
    assert_eq!(Command::Quit.default_chords()[0].keystroke(), "cmd-q");
    assert_eq!(Command::Quit.scope(), KeyScope::App);
    assert_eq!(Command::Quit.group(), ShortcutGroup::App);
}

#[test]
fn a_key_of_the_app_scope_is_taken_in_no_other_scope() {
    let app_keys: Vec<String> = Command::ALL
        .into_iter()
        .filter(|command| command.scope() == KeyScope::App)
        .flat_map(|command| {
            command
                .default_chords()
                .iter()
                .copied()
                .map(Chord::keystroke)
        })
        .collect();

    for command in Command::ALL {
        if command.scope() == KeyScope::App {
            continue;
        }
        for chord in command.default_chords() {
            assert!(
                !app_keys.contains(&chord.keystroke()),
                "{} is also a key of the whole app",
                chord.keystroke()
            );
        }
    }
}

#[test]
fn the_page_keys_are_all_single_keys_or_arrows_so_g_fits_them() {
    for command in Command::ALL {
        if command.scope() == KeyScope::Page {
            for chord in command.default_chords() {
                let shifted = chord.keystroke().starts_with("shift-");
                assert_eq!(
                    chord.caps().count(),
                    if shifted { 2 } else { 1 },
                    "{command:?} is a chord on the page"
                );
            }
        }
    }
}

#[test]
fn the_keys_of_the_focused_change_are_on_the_page_with_defaults() {
    let keys = |command: Command| -> Vec<String> {
        command
            .default_chords()
            .iter()
            .copied()
            .map(Chord::keystroke)
            .collect()
    };

    assert_eq!(keys(Command::NextChange), ["tab"]);
    assert_eq!(keys(Command::PreviousChange), ["shift-tab"]);
    assert_eq!(keys(Command::ReadDown), ["down"]);
    assert_eq!(keys(Command::ReadUp), ["up"]);
    assert_eq!(keys(Command::MoreAbove), ["["]);
    assert_eq!(keys(Command::MoreBelow), ["]"]);
    for command in [
        Command::NextChange,
        Command::PreviousChange,
        Command::ReadDown,
        Command::ReadUp,
        Command::MoreAbove,
        Command::MoreBelow,
    ] {
        assert_eq!(command.scope(), KeyScope::Page, "{command:?}");
    }
}
