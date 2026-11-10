use std::collections::HashSet;

use gpui::Keystroke;
use guided_review::viewer::{Command, KeyScope, ShortcutGroup, in_group, keystroke};

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
        assert!(!command.chords().is_empty(), "{command:?} has no keys");
        assert_eq!(command.first_chord(), command.chords()[0]);
        for chord in command.chords() {
            let keys = keystroke(chord);
            Keystroke::parse(&keys).unwrap_or_else(|_| panic!("{command:?}: {keys} is not a key"));
        }
    }
}

#[test]
fn no_two_commands_of_one_scope_share_a_key() {
    let mut taken = HashSet::new();
    for command in Command::ALL {
        for chord in command.chords() {
            let scope = match command.scope() {
                KeyScope::Page => "page",
                KeyScope::Window => "window",
                KeyScope::TextBox => "text box",
            };
            assert!(
                taken.insert((scope, keystroke(chord))),
                "{} is bound twice on the {scope}",
                keystroke(chord)
            );
        }
    }
}

#[test]
fn a_chord_is_written_as_gpui_reads_it() {
    assert_eq!(keystroke(Command::Jump.first_chord()), "cmd-k");
    assert_eq!(
        keystroke(Command::AskAgent.first_chord()),
        "cmd-shift-enter"
    );
    assert_eq!(keystroke(Command::NextScreen.first_chord()), "right");
    assert_eq!(keystroke(Command::Shortcuts.first_chord()), "?");
}

#[test]
fn the_keys_the_app_had_before_the_sheet_still_work() {
    let keys = |command: Command| -> Vec<String> {
        command
            .chords()
            .iter()
            .map(|chord| keystroke(chord))
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
