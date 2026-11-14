/// Plain files, so it compiles and is tested on every host.
pub mod app_bundle;
#[cfg(target_os = "macos")]
pub mod dock_icon;
#[cfg(target_os = "macos")]
pub mod word_keys;

use super::{Adapter, AppMenu, HerdrRaise, KeySpelling, PrimaryKey, TextEdit, WindowButtons};

pub(super) const ADAPTER: Adapter = Adapter {
    triple_system: "apple-darwin",
    // Both come with macOS.
    text_fonts: &["Helvetica Neue"],
    code_fonts: &["Menlo"],
    primary_key: PrimaryKey::Command,
    key_spelling: KeySpelling {
        primary: "⌘",
        ctrl: "⌃",
        alt: "⌥",
        shift: "⇧",
        joiner: "",
    },
    text_keys: &[
        ("alt-backspace", TextEdit::BackspaceWord),
        ("cmd-backspace", TextEdit::BackspaceLine),
        ("alt-delete", TextEdit::DeleteWord),
        ("alt-left", TextEdit::WordLeft),
        ("alt-right", TextEdit::WordRight),
        ("cmd-left", TextEdit::LineStart),
        ("cmd-right", TextEdit::LineEnd),
        ("cmd-up", TextEdit::TextStart),
        ("cmd-down", TextEdit::TextEnd),
        ("alt-shift-left", TextEdit::SelectWordLeft),
        ("alt-shift-right", TextEdit::SelectWordRight),
        ("cmd-shift-left", TextEdit::SelectLineStart),
        ("cmd-shift-right", TextEdit::SelectLineEnd),
        ("cmd-a", TextEdit::SelectAll),
        ("cmd-c", TextEdit::Copy),
        ("cmd-x", TextEdit::Cut),
        ("cmd-v", TextEdit::Paste),
    ],
    // The traffic lights.
    window_buttons: Some(WindowButtons {
        corner: 14.,
        inset: 80.,
    }),
    // macOS shows Quit's key beside it.
    app_menu: Some(AppMenu {
        name: "guided-review",
        quit: "Quit guided-review",
    }),
    herdr_raise: HerdrRaise {
        program: "/usr/bin/open",
        args: &["-a", "Herdr"],
        best_effort: false,
    },
};
