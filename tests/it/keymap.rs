use gpui::Keystroke;
use guided_review::app::keymap::chord_on;
use guided_review::config::Config;
use guided_review::platform::{Os, TextEdit};
use guided_review::viewer::{
    Chord, Command, KeyProblem, Keymap, Modifier, SendAction, SendChoice, SendRole,
};

use SendAction::{AddToReview, AskAgent, AskAgentNow, CommentNow, SuggestChange};
use SendRole::{Other, Primary, Secondary};

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

const SEND_COMMANDS: [Command; 5] = [
    Command::AddToReview,
    Command::CommentNow,
    Command::SuggestChange,
    Command::AskAgent,
    Command::AskAgentNow,
];

fn send_keys(keymap: &Keymap) -> Vec<String> {
    SEND_COMMANDS
        .map(|command| keys(keymap, command).join(" "))
        .to_vec()
}

fn choice_config(primary: SendAction, secondary: SendAction) -> String {
    format!(
        "primary_comment_action = \"{}\"\nsecondary_comment_action = \"{}\"\n",
        primary.config_text(),
        secondary.config_text()
    )
}

#[test]
fn with_no_choice_add_to_review_is_primary_and_ask_agent_secondary_and_the_rest_have_their_own_keys()
 {
    let keymap = keymap_of("").unwrap();

    assert_eq!(keymap.send_choice(), SendChoice::default());
    assert_eq!(
        send_keys(&keymap),
        [
            "primary-enter",
            "primary-alt-enter",
            "primary-alt-s",
            "primary-shift-enter",
            "primary-alt-shift-enter",
        ]
    );
}

#[test]
fn the_primary_action_has_cmd_enter_the_secondary_cmd_shift_enter_and_the_others_keep_their_own() {
    let keymap = keymap_of(&choice_config(CommentNow, SuggestChange)).unwrap();

    assert_eq!(
        send_keys(&keymap),
        [
            "primary-alt-a",
            "primary-enter",
            "primary-shift-enter",
            "primary-alt-g",
            "primary-alt-shift-enter",
        ]
    );
}

#[test]
fn no_choice_of_primary_and_secondary_gives_two_send_actions_one_key() {
    for primary in SendAction::ALL {
        for secondary in SendAction::ALL.into_iter().filter(|it| *it != primary) {
            let config = choice_config(primary, secondary);
            let keymap = keymap_of(&config).unwrap();
            let mut chords: Vec<_> = SEND_COMMANDS
                .into_iter()
                .flat_map(|command| keymap.chords(command).to_vec())
                .collect();
            let count = chords.len();
            chords.sort_by_key(|chord| chord.written());
            chords.dedup();

            assert_eq!(chords.len(), count, "{config}");
            assert_eq!(count, 5, "{config}");
            assert_eq!(keys(&keymap, Command::from(primary)), ["primary-enter"]);
            assert_eq!(
                keys(&keymap, Command::from(secondary)),
                ["primary-shift-enter"]
            );
        }
    }
}

#[test]
fn a_config_made_before_the_secondary_setting_still_reads() {
    let keymap = keymap_of("primary_comment_action = \"comment_now\"\n").unwrap();

    assert_eq!(keymap.send_choice(), SendChoice::new(CommentNow, AskAgent));
    assert_eq!(keys(&keymap, Command::CommentNow), ["primary-enter"]);
    assert_eq!(keys(&keymap, Command::AddToReview), ["primary-alt-a"]);
}

#[test]
fn a_key_written_in_keys_wins_over_the_choice() {
    let keymap = keymap_of(&format!(
        "{}\n[keys]\nadd_to_review = \"primary-j\"\n",
        choice_config(CommentNow, AskAgent)
    ))
    .unwrap();

    assert_eq!(keys(&keymap, Command::AddToReview), ["primary-j"]);
    assert_eq!(keys(&keymap, Command::CommentNow), ["primary-enter"]);
}

#[test]
fn a_key_written_in_keys_that_a_main_key_takes_is_a_clash() {
    let problems = keymap_of(&format!(
        "{}\n[keys]\nadd_to_review = \"primary-enter\"\n",
        choice_config(CommentNow, AskAgent)
    ))
    .unwrap_err();

    assert_eq!(
        problems,
        [KeyProblem::Clash {
            chord: chord("primary-enter"),
            commands: [Command::AddToReview, Command::CommentNow],
        }]
    );
}

#[test]
fn texts_that_name_no_action_or_the_same_one_twice_read_as_a_choice_of_two() {
    let read = SendChoice::of_config_text;

    assert_eq!(read(None, None), SendChoice::default());
    assert_eq!(read(Some("post"), Some("send")), SendChoice::default());
    assert_eq!(
        read(Some("ask_agent_now"), None),
        SendChoice::new(AskAgentNow, AskAgent)
    );
    assert_eq!(
        read(Some("ask_agent"), None),
        SendChoice::new(AskAgent, AddToReview)
    );
    assert_eq!(
        read(Some("comment_now"), Some("comment_now")),
        SendChoice::new(CommentNow, AskAgent)
    );
}

#[test]
fn picking_the_other_ones_action_swaps_primary_and_secondary() {
    let choice = SendChoice::default();

    assert_eq!(
        choice.with_primary(AskAgent),
        SendChoice::new(AskAgent, AddToReview)
    );
    assert_eq!(
        choice.with_primary(CommentNow),
        SendChoice::new(CommentNow, AskAgent)
    );
    assert_eq!(
        choice.with_secondary(AddToReview),
        SendChoice::new(AskAgent, AddToReview)
    );
    assert_eq!(
        choice.with_secondary(SuggestChange),
        SendChoice::new(AddToReview, SuggestChange)
    );
    assert_eq!(choice.role_of(AddToReview), Primary);
    assert_eq!(choice.role_of(AskAgent), Secondary);
    assert_eq!(choice.role_of(CommentNow), Other);
}

#[test]
fn an_action_a_box_does_not_offer_is_stood_in_for_by_the_nearest_one_it_does() {
    assert_eq!(CommentNow.resolve(&[AddToReview, CommentNow]), CommentNow);
    assert_eq!(CommentNow.resolve(&[AddToReview, AskAgent]), AddToReview);
    assert_eq!(
        SuggestChange.resolve(&[AddToReview, CommentNow]),
        AddToReview
    );
    assert_eq!(AskAgent.resolve(&[AddToReview, CommentNow]), AddToReview);
    assert_eq!(
        AskAgentNow.resolve(&[AddToReview, AskAgent, CommentNow]),
        AskAgent
    );
    assert_eq!(AskAgentNow.resolve(&[AddToReview, CommentNow]), CommentNow);
    assert_eq!(AskAgentNow.resolve(&[AddToReview]), AddToReview);
}

type Button = (SendAction, SendRole, Option<String>);

fn plan(config: &str, offered: &[SendAction]) -> Vec<Button> {
    keymap_of(config)
        .unwrap()
        .send_plan(offered)
        .into_iter()
        .map(|send| (send.action, send.role, send.chord.map(Chord::written)))
        .collect()
}

fn button(action: SendAction, role: SendRole, keys: &str) -> Button {
    (action, role, Some(keys.to_owned()))
}

const EVERY_ACTION: [SendAction; 5] = SendAction::ALL;

#[test]
fn a_box_lists_the_primary_first_then_the_secondary_then_the_rest_each_with_its_key() {
    assert_eq!(
        plan("", &EVERY_ACTION),
        [
            button(AddToReview, Primary, "primary-enter"),
            button(AskAgent, Secondary, "primary-shift-enter"),
            button(CommentNow, Other, "primary-alt-enter"),
            button(SuggestChange, Other, "primary-alt-s"),
            button(AskAgentNow, Other, "primary-alt-shift-enter"),
        ]
    );
    assert_eq!(
        plan(&choice_config(AskAgentNow, CommentNow), &EVERY_ACTION),
        [
            button(AskAgentNow, Primary, "primary-enter"),
            button(CommentNow, Secondary, "primary-shift-enter"),
            button(AddToReview, Other, "primary-alt-a"),
            button(SuggestChange, Other, "primary-alt-s"),
            button(AskAgent, Other, "primary-alt-g"),
        ]
    );
}

#[test]
fn a_main_action_the_box_lacks_is_stood_in_for_and_the_stand_in_shows_the_main_key() {
    let no_comment_now = [AddToReview, SuggestChange, AskAgent];

    assert_eq!(
        plan(&choice_config(CommentNow, AskAgent), &no_comment_now),
        [
            button(AddToReview, Primary, "primary-enter"),
            button(AskAgent, Secondary, "primary-shift-enter"),
            button(SuggestChange, Other, "primary-alt-s"),
        ]
    );
    let reply = [AddToReview, CommentNow, AskAgent, AskAgentNow];
    assert_eq!(
        plan(&choice_config(SuggestChange, AskAgentNow), &reply),
        [
            button(AddToReview, Primary, "primary-enter"),
            button(AskAgentNow, Secondary, "primary-shift-enter"),
            button(CommentNow, Other, "primary-alt-enter"),
            button(AskAgent, Other, "primary-alt-g"),
        ]
    );
}

#[test]
fn two_main_actions_that_end_up_as_one_leave_one_button() {
    let local = [AddToReview, CommentNow];

    assert_eq!(
        plan("", &local),
        [
            button(AddToReview, Primary, "primary-enter"),
            button(CommentNow, Other, "primary-alt-enter"),
        ]
    );
    assert_eq!(
        plan(&choice_config(CommentNow, AddToReview), &local),
        [
            button(CommentNow, Primary, "primary-enter"),
            button(AddToReview, Secondary, "primary-shift-enter"),
        ]
    );
}

#[test]
fn a_button_shows_a_key_written_in_keys_and_none_for_an_emptied_command() {
    let config = "[keys]\nadd_to_review = \"primary-j\"\nask_agent = []\n";

    assert_eq!(
        plan(config, &[AddToReview, AskAgent]),
        [
            button(AddToReview, Primary, "primary-j"),
            (AskAgent, Secondary, None),
        ]
    );
}

#[test]
fn a_key_runs_its_action_where_offered_and_a_main_key_runs_what_stands_in_for_it() {
    let keymap = keymap_of(&choice_config(CommentNow, SuggestChange)).unwrap();
    let reply = [AddToReview, AskAgent];

    assert_eq!(keymap.send_key_runs(AskAgent, &reply), Some(AskAgent));
    assert_eq!(keymap.send_key_runs(CommentNow, &reply), Some(AddToReview));
    assert_eq!(
        keymap.send_key_runs(SuggestChange, &reply),
        Some(AddToReview)
    );
    assert_eq!(keymap.send_key_runs(AskAgentNow, &reply), None);
    assert_eq!(keymap.send_key_runs(AddToReview, &reply), Some(AddToReview));
}
