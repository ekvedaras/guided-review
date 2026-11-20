use gpui::Keystroke;
use guided_review::app::keymap::chord_on;
use guided_review::config::Config;
use guided_review::platform::{Os, TextEdit};
use guided_review::viewer::{
    Chord, Command, KeyProblem, Keymap, Modifier, PrimaryCommentAction, SendButton, SendMode,
    Sending,
};

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
        .map(Chord::written)
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
fn a_chord_is_read_in_any_modifier_order_and_written_in_one() {
    assert_eq!(
        chord("shift-primary-enter").written(),
        "primary-shift-enter"
    );
    assert_eq!(chord("shift-alt-x").written(), "alt-shift-x");
    assert_eq!(chord("W"), chord("w"));
    assert_eq!(chord("primary--").written(), "primary--");
    assert_eq!(chord("[").written(), "[");
    assert_eq!(chord("tab").written(), "tab");
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
        "[keys]\nwrap_lines = \"y\"\nnext_screen = [\"space\", \"n\"]\nnext_unread_step = \"u\"",
    )
    .unwrap();

    assert_eq!(keys(&keymap, Command::WrapLines), ["y"]);
    assert_eq!(keys(&keymap, Command::NextScreen), ["space", "n"]);
    assert!(keymap.is_changed(Command::WrapLines));
    assert_eq!(keys(&keymap, Command::Jump), ["primary-k"]);
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
    assert_eq!(keys(&keymap, Command::Jump), ["primary-k"]);
    assert_eq!(keys(&keymap, Command::Quit), ["primary-q"]);
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
    let (keymap, problems) = resolved("[keys]\nwrap_lines = \"y\"");

    assert_eq!(keys(&keymap, Command::WrapLines), ["y"]);
    assert_eq!(problems.len(), 0);
}

#[test]
fn the_notice_at_start_counts_the_problems_and_points_to_settings() {
    let (_, none) = resolved("[keys]\nwrap_lines = \"y\"");
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

#[test]
fn a_chord_is_spelled_on_macos_with_the_symbols_its_caps_use() {
    let spelled = |text: &str| {
        Chord::parse_on(Os::MacOs, text)
            .unwrap()
            .spelled_on(Os::MacOs)
    };

    assert_eq!(spelled("w"), "W");
    assert_eq!(spelled("primary-shift-enter"), "⌘⇧↵");
    assert_eq!(spelled("alt-left"), "⌥←");
    assert_eq!(spelled("ctrl-x"), "⌃X");
    assert_eq!(spelled("escape"), "esc");
}

#[test]
fn a_chord_is_spelled_on_linux_with_named_modifiers_joined_by_a_plus() {
    let spelled = |text: &str| {
        Chord::parse_on(Os::Linux, text)
            .unwrap()
            .spelled_on(Os::Linux)
    };

    assert_eq!(spelled("w"), "W");
    assert_eq!(spelled("primary-shift-enter"), "Ctrl+Shift+↵");
    assert_eq!(spelled("alt-left"), "Alt+←");
    assert_eq!(spelled("escape"), "esc");
}

#[test]
fn a_pressed_key_is_primary_with_cmd_on_macos_and_ctrl_on_linux() {
    let pressed = |os: Os, keys: &str| chord_on(os, &Keystroke::parse(keys).unwrap());

    assert_eq!(pressed(Os::MacOs, "cmd-k"), Some(chord("primary-k")));
    assert_eq!(
        pressed(Os::MacOs, "ctrl-k"),
        Chord::parse_on(Os::MacOs, "ctrl-k")
    );
    assert_eq!(pressed(Os::Linux, "ctrl-k"), Some(chord("primary-k")));
    assert_eq!(
        pressed(Os::Linux, "super-k"),
        None,
        "Super belongs to the desktop"
    );
}

#[test]
fn a_saved_cmd_key_is_primary_and_a_written_ctrl_is_primary_only_where_ctrl_is() {
    let primary_k = Chord::parse_on(Os::Linux, "primary-k");

    assert_eq!(Chord::parse_on(Os::Linux, "cmd-k"), primary_k);
    assert_eq!(Chord::parse_on(Os::Linux, "ctrl-k"), primary_k);
    assert_eq!(Chord::parse_on(Os::MacOs, "cmd-k"), primary_k);
    assert_ne!(Chord::parse_on(Os::MacOs, "ctrl-k"), primary_k);
    assert_eq!(
        keys(
            &keymap_of("[keys]\njump = \"cmd-j\"").unwrap(),
            Command::Jump
        ),
        ["primary-j"]
    );
}

#[test]
fn primary_is_written_as_primary_and_bound_as_gpui_secondary() {
    assert_eq!(chord("cmd-k").written(), "primary-k");
    assert_eq!(chord("cmd-k").keystroke(), "secondary-k");
}

#[test]
fn a_modifier_cap_shows_the_face_its_spelling_uses() {
    assert_eq!(Modifier::Primary.face(Os::MacOs), "⌘");
    assert_eq!(Modifier::Primary.face(Os::Linux), "Ctrl");
    assert_eq!(Modifier::Alt.face(Os::Linux), "Alt");
}

#[test]
fn the_new_simple_keys_change_and_clear_through_the_keys_table() {
    let keymap = keymap_of("[keys]\nhide_whitespace = \"x\"\nask_about_step = []").unwrap();

    assert_eq!(keys(&keymap, Command::HideWhitespace), ["x"]);
    assert!(keymap.first_chord(Command::AskAboutStep).is_none());
}

#[test]
fn s_on_the_page_clashes_with_a_command_of_the_window() {
    let problems = keymap_of("[keys]\nsettings = \"s\"").unwrap_err();

    assert_eq!(
        problems,
        [KeyProblem::Clash {
            chord: chord("s"),
            commands: [Command::HideWhitespace, Command::Settings],
        }]
    );
}

fn text_key(os: Os, edit: TextEdit) -> Vec<&'static str> {
    os.text_keys()
        .iter()
        .filter(|(_, bound)| *bound == edit)
        .map(|(keys, _)| *keys)
        .collect()
}

#[test]
fn text_boxes_take_macos_text_keys_on_macos() {
    assert_eq!(text_key(Os::MacOs, TextEdit::WordLeft), ["alt-left"]);
    assert_eq!(text_key(Os::MacOs, TextEdit::LineStart), ["cmd-left"]);
    assert_eq!(text_key(Os::MacOs, TextEdit::TextEnd), ["cmd-down"]);
    assert_eq!(
        text_key(Os::MacOs, TextEdit::BackspaceLine),
        ["cmd-backspace"]
    );
    assert_eq!(text_key(Os::MacOs, TextEdit::Paste), ["cmd-v"]);
}

#[test]
fn text_boxes_take_linux_text_keys_on_linux_with_no_super() {
    assert_eq!(text_key(Os::Linux, TextEdit::WordLeft), ["ctrl-left"]);
    assert_eq!(
        text_key(Os::Linux, TextEdit::BackspaceWord),
        ["ctrl-backspace"]
    );
    assert_eq!(text_key(Os::Linux, TextEdit::TextEnd), ["ctrl-end"]);
    assert_eq!(
        text_key(Os::Linux, TextEdit::SelectLineStart),
        ["shift-home"]
    );
    assert_eq!(text_key(Os::Linux, TextEdit::Paste), ["ctrl-v"]);
    for (keys, _) in Os::Linux.text_keys() {
        let modifiers = Keystroke::parse(keys).unwrap().modifiers;
        assert!(!modifiers.platform, "{keys} holds Super");
    }
}

#[test]
fn every_text_key_is_one_gpui_can_bind() {
    for os in [Os::MacOs, Os::Linux] {
        for (keys, _) in os.text_keys() {
            Keystroke::parse(keys).unwrap_or_else(|_| panic!("{keys} is not a key"));
        }
    }
}

fn comment_keys(keymap: &Keymap) -> [Vec<String>; 4] {
    [
        Command::AddToReview,
        Command::CommentNow,
        Command::AskAgent,
        Command::AskAgentNow,
    ]
    .map(|command| keys(keymap, command))
}

#[test]
fn with_no_primary_comment_action_add_to_review_has_the_main_key() {
    let keymap = keymap_of("").unwrap();

    assert_eq!(
        keymap.primary_comment_action(),
        PrimaryCommentAction::AddToReview
    );
    assert_eq!(
        comment_keys(&keymap),
        [
            vec!["primary-enter"],
            vec!["primary-alt-enter"],
            vec!["primary-shift-enter"],
            vec!["primary-alt-shift-enter"],
        ]
    );
}

#[test]
fn comment_now_as_primary_takes_the_main_keys_and_gives_add_to_review_the_secondary_ones() {
    let keymap = keymap_of("primary_comment_action = \"comment_now\"\n").unwrap();

    assert_eq!(
        keymap.primary_comment_action(),
        PrimaryCommentAction::CommentNow
    );
    assert_eq!(
        comment_keys(&keymap),
        [
            vec!["primary-alt-enter"],
            vec!["primary-enter"],
            vec!["primary-alt-shift-enter"],
            vec!["primary-shift-enter"],
        ]
    );
    assert!(!keymap.is_changed(Command::CommentNow));
}

#[test]
fn a_key_written_in_keys_wins_over_the_primary_comment_action() {
    let keymap = keymap_of(
        "primary_comment_action = \"comment_now\"\n\n[keys]\nadd_to_review = \"primary-j\"\n",
    )
    .unwrap();

    assert_eq!(keys(&keymap, Command::AddToReview), ["primary-j"]);
    assert_eq!(keys(&keymap, Command::CommentNow), ["primary-enter"]);
}

#[test]
fn a_key_written_in_keys_that_the_primary_action_takes_is_a_clash() {
    let problems = keymap_of(
        "primary_comment_action = \"comment_now\"\n\n[keys]\nadd_to_review = \"primary-enter\"\n",
    )
    .unwrap_err();

    assert!(matches!(
        problems.as_slice(),
        [KeyProblem::Clash {
            commands: [Command::AddToReview, Command::CommentNow],
            ..
        }]
    ));
}

#[test]
fn a_primary_comment_action_that_names_no_action_reads_as_add_to_review() {
    let config = Config::from_text("primary_comment_action = \"post\"\n").unwrap();

    assert_eq!(
        config.primary_comment_action(),
        PrimaryCommentAction::AddToReview
    );
    assert_eq!(
        Config::from_text("primary_comment_action = \"comment_now\"\n")
            .unwrap()
            .primary_comment_action(),
        PrimaryCommentAction::CommentNow
    );
}

#[test]
fn a_box_with_one_send_action_makes_it_primary_whatever_is_chosen() {
    assert_eq!(
        PrimaryCommentAction::CommentNow.effective(false),
        PrimaryCommentAction::AddToReview
    );
    assert_eq!(
        PrimaryCommentAction::CommentNow.effective(true),
        PrimaryCommentAction::CommentNow
    );
    assert_eq!(
        PrimaryCommentAction::AddToReview.effective(true),
        PrimaryCommentAction::AddToReview
    );
}

#[test]
fn a_local_review_puts_send_now_first_when_comment_now_is_primary() {
    let local = Sending::Tasks(&SendMode::LOCAL);

    assert_eq!(
        PrimaryCommentAction::AddToReview.ordered_tasks(local),
        [SendMode::AddToTasks, SendMode::SendNow]
    );
    assert_eq!(
        PrimaryCommentAction::CommentNow.ordered_tasks(local),
        [SendMode::SendNow, SendMode::AddToTasks]
    );
    assert_eq!(
        PrimaryCommentAction::CommentNow.ordered_tasks(Sending::GitHub),
        Vec::<SendMode>::new()
    );
}

fn button_keys(config: &str, comment_now_is_offered: bool) -> Vec<Option<String>> {
    let keymap = keymap_of(config).unwrap();
    [
        SendButton::AddToReview,
        SendButton::CommentNow,
        SendButton::AskAgent,
        SendButton::AskAgentNow,
        SendButton::Task(0),
        SendButton::Task(1),
    ]
    .map(|button| {
        keymap
            .send_button_chord(button, comment_now_is_offered)
            .map(Chord::written)
    })
    .to_vec()
}

fn some(texts: [&str; 6]) -> Vec<Option<String>> {
    texts.map(|text| Some(text.to_owned())).to_vec()
}

#[test]
fn send_buttons_show_the_keys_the_primary_action_gave_them() {
    assert_eq!(
        button_keys("", true),
        some([
            "primary-enter",
            "primary-alt-enter",
            "primary-shift-enter",
            "primary-alt-shift-enter",
            "primary-enter",
            "primary-alt-enter",
        ])
    );
    assert_eq!(
        button_keys("primary_comment_action = \"comment_now\"\n", true),
        some([
            "primary-alt-enter",
            "primary-enter",
            "primary-alt-shift-enter",
            "primary-shift-enter",
            "primary-alt-enter",
            "primary-enter",
        ])
    );
}

#[test]
fn the_only_send_action_left_shows_the_main_key_whatever_the_primary_action() {
    let keys = button_keys("primary_comment_action = \"comment_now\"\n", false);

    assert_eq!(keys[0].as_deref(), Some("primary-enter"));
    assert_eq!(keys[2].as_deref(), Some("primary-shift-enter"));
}

#[test]
fn send_buttons_show_keys_written_in_keys_and_none_for_an_emptied_command() {
    let keys = button_keys(
        "[keys]\ncomment_now = \"primary-j\"\nask_agent = []\n",
        true,
    );

    assert_eq!(keys[1].as_deref(), Some("primary-j"));
    assert_eq!(keys[2], None);
}
