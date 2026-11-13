use gpui::Keystroke;
use guided_review::config::Config;
use guided_review::viewer::{Chord, Command, KeyProblem, Keymap};

fn chord(text: &str) -> Chord {
    Chord::parse(text).unwrap_or_else(|| panic!("{text} is not a chord"))
}

fn keymap_of(config: &str) -> Result<Keymap, Vec<KeyProblem>> {
    Config::from_text(config).unwrap().keymap()
}

fn keys(keymap: &Keymap, command: Command) -> Vec<String> {
    keymap
        .chords(command)
        .iter()
        .copied()
        .map(Chord::keystroke)
        .collect()
}

#[test]
fn with_no_keys_table_every_command_has_its_default_keys() {
    let keymap = keymap_of("").unwrap();

    for command in Command::ALL {
        assert_eq!(keymap.chords(command), command.default_chords());
        assert!(!keymap.is_changed(command));
    }
}

#[test]
fn a_chord_is_read_in_any_modifier_order_and_written_as_gpui_reads_it() {
    assert_eq!(chord("shift-cmd-enter").keystroke(), "cmd-shift-enter");
    assert_eq!(chord("alt-ctrl-x").keystroke(), "ctrl-alt-x");
    assert_eq!(chord("W"), chord("w"));
    assert_eq!(chord("cmd--").keystroke(), "cmd--");
    assert_eq!(chord("[").keystroke(), "[");
    assert_eq!(chord("tab").keystroke(), "tab");
    for text in ["cmd-", "", "f13", "cmd-shift", "hyper-x", "ą"] {
        assert_eq!(Chord::parse(text), None, "{text} read as a chord");
    }
}

#[test]
fn every_chord_the_app_can_read_is_one_gpui_can_bind() {
    for text in [
        "a",
        "cmd-k",
        "ctrl-alt-shift-cmd-z",
        "?",
        ",",
        "[",
        "]",
        "-",
        "left",
        "right",
        "up",
        "down",
        "enter",
        "escape",
        "tab",
        "space",
        "backspace",
        "delete",
    ] {
        let keys = chord(text).keystroke();
        Keystroke::parse(&keys).unwrap_or_else(|_| panic!("{keys} is not a key"));
    }
}

#[test]
fn a_command_named_in_the_keys_table_takes_only_its_new_keys() {
    let keymap = keymap_of(
        "[keys]\nwrap_lines = \"l\"\nnext_screen = [\"down\", \"n\"]\nnext_unread_step = \"u\"",
    )
    .unwrap();

    assert_eq!(keys(&keymap, Command::WrapLines), ["l"]);
    assert_eq!(keys(&keymap, Command::NextScreen), ["down", "n"]);
    assert!(keymap.is_changed(Command::WrapLines));
    assert_eq!(keys(&keymap, Command::Jump), ["cmd-k"]);
    assert!(!keymap.is_changed(Command::Jump));
}

#[test]
fn two_commands_can_swap_their_keys() {
    let keymap = keymap_of("[keys]\nnext_screen = \"k\"\nprevious_screen = \"j\"").unwrap();

    assert_eq!(keys(&keymap, Command::NextScreen), ["k"]);
    assert_eq!(keys(&keymap, Command::PreviousScreen), ["j"]);
}

#[test]
fn an_empty_list_leaves_a_command_with_no_key() {
    let keymap = keymap_of("[keys]\nopen_on_github = []").unwrap();

    assert_eq!(keymap.chords(Command::OpenOnGitHub).len(), 0);
    assert_eq!(keymap.first_chord(Command::OpenOnGitHub), None);
}

#[test]
fn a_name_that_is_no_command_or_a_text_that_is_no_key_is_a_problem() {
    let problems = keymap_of("[keys]\nwrap = \"l\"\njump = \"cmd-\"\nquit = 7").unwrap_err();

    assert_eq!(
        problems,
        [
            KeyProblem::NotAKey {
                command: Command::Jump,
                written: "cmd-".to_owned()
            },
            KeyProblem::NotAKey {
                command: Command::Quit,
                written: "7".to_owned()
            },
            KeyProblem::UnknownCommand("wrap".to_owned()),
        ]
    );
}

#[test]
fn a_key_two_commands_would_both_answer_is_a_problem_naming_both() {
    let problems = keymap_of("[keys]\nwrap_lines = \"m\"").unwrap_err();

    assert_eq!(
        problems,
        [KeyProblem::Clash {
            chord: chord("m"),
            commands: [Command::ToggleRead, Command::WrapLines],
        }]
    );
}

#[test]
fn a_key_of_the_window_or_the_app_clashes_with_any_other_place() {
    assert!(keymap_of("[keys]\njump = \"w\"").is_err());
    assert!(keymap_of("[keys]\nquit = \"escape\"").is_err());
}

#[test]
fn the_same_key_may_do_one_thing_on_the_page_and_another_while_typing() {
    let keymap = keymap_of("[keys]\nclose_box = \"w\"").unwrap();

    assert_eq!(keys(&keymap, Command::CloseBox), ["w"]);
    assert_eq!(keys(&keymap, Command::WrapLines), ["w"]);
}

#[test]
fn each_problem_says_what_to_fix() {
    let messages: Vec<String> =
        keymap_of("[keys]\nwrap = \"l\"\njump = \"cmd-\"\nwrap_lines = \"m\"")
            .unwrap_err()
            .iter()
            .map(KeyProblem::message)
            .collect();

    assert_eq!(
        messages,
        [
            "cmd- is not a key",
            "m is taken by both “Mark the step as read, or unread” and “Wrap long lines”",
            "wrap is not a command",
        ]
    );
}

#[test]
fn every_command_has_a_config_name_that_reads_back_as_itself() {
    for command in Command::ALL {
        assert_eq!(
            Command::of_config_name(command.config_name()),
            Some(command)
        );
    }
    assert_eq!(Command::WrapLines.config_name(), "wrap_lines");
    assert_eq!(Command::OpenOnGitHub.config_name(), "open_on_github");
}

fn resolved(config: &str) -> (Keymap, Vec<KeyProblem>) {
    let resolved = Config::from_text(config).unwrap().resolved_keymap();
    (resolved.keymap, resolved.problems)
}

#[test]
fn one_bad_line_leaves_the_good_lines_in_force() {
    let (keymap, problems) =
        resolved("[keys]\nwrap = \"l\"\njump = \"cmd-\"\nnext_unread_step = \"u\"\nquit = 7");

    assert_eq!(keys(&keymap, Command::NextUnreadStep), ["u"]);
    assert_eq!(keys(&keymap, Command::Jump), ["cmd-k"]);
    assert_eq!(keys(&keymap, Command::Quit), ["cmd-q"]);
    assert_eq!(problems.len(), 3);
}

#[test]
fn two_commands_on_one_key_both_go_back_to_their_defaults() {
    let (keymap, problems) = resolved("[keys]\nwrap_lines = \"m\"\ntoggle_read = \"m\"");

    assert_eq!(keys(&keymap, Command::WrapLines), ["w"]);
    assert_eq!(keys(&keymap, Command::ToggleRead), ["m"]);
    assert!(!keymap.is_changed(Command::WrapLines));
    assert!(!keymap.is_changed(Command::ToggleRead));
    assert_ne!(problems.len(), 0);
}

#[test]
fn going_back_to_a_default_cannot_leave_a_new_clash() {
    let (keymap, problems) =
        resolved("[keys]\nwrap_lines = \"n\"\nnext_unread_step = \"n\"\ntoggle_read = \"w\"");

    for (at, first) in Command::ALL.iter().enumerate() {
        for second in Command::ALL.iter().skip(at + 1) {
            if first.scope().overlaps(second.scope()) {
                for chord in keymap.chords(*first) {
                    assert!(
                        !keymap.chords(*second).contains(chord),
                        "{first:?} and {second:?}"
                    );
                }
            }
        }
    }
    assert!(problems.len() >= 2);
}

#[test]
fn a_config_with_no_problem_resolves_to_its_keymap() {
    let (keymap, problems) = resolved("[keys]\nwrap_lines = \"l\"");

    assert_eq!(keys(&keymap, Command::WrapLines), ["l"]);
    assert_eq!(problems.len(), 0);
}

#[test]
fn the_notice_at_start_counts_the_problems_and_points_to_settings() {
    let (_, none) = resolved("[keys]\nwrap_lines = \"l\"");
    assert_eq!(none.len(), 0);
    let one = Config::from_text("[keys]\njump = \"cmd-\"")
        .unwrap()
        .resolved_keymap();
    let two = Config::from_text("[keys]\njump = \"cmd-\"\nwrap = \"l\"")
        .unwrap()
        .resolved_keymap();

    assert_eq!(
        one.notice(),
        Some(
            "1 key in config.toml cannot be used and keeps its default. Settings shows it."
                .to_owned()
        )
    );
    assert_eq!(
        two.notice(),
        Some(
            "2 keys in config.toml cannot be used and keep their defaults. Settings shows them."
                .to_owned()
        )
    );
    assert_eq!(
        Config::from_text("").unwrap().resolved_keymap().notice(),
        None
    );
}
