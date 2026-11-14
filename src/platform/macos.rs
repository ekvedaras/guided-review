/// Plain files, so it compiles and is tested on every host.
pub mod app_bundle;
#[cfg(target_os = "macos")]
pub mod dock_icon;
#[cfg(target_os = "macos")]
pub mod word_keys;

use super::{HerdrRaise, KeySpelling, PrimaryKey, TextEdit, WindowButtons};

pub const TRIPLE_SYSTEM: &str = "apple-darwin";

/// Both come with macOS.
pub const TEXT_FONTS: &[&str] = &["Helvetica Neue"];
pub const CODE_FONTS: &[&str] = &["Menlo"];

pub const PRIMARY_KEY: PrimaryKey = PrimaryKey::Command;

pub const KEY_SPELLING: KeySpelling = KeySpelling {
    primary: "⌘",
    ctrl: "⌃",
    alt: "⌥",
    shift: "⇧",
    joiner: "",
};

pub const TEXT_KEYS: &[(&str, TextEdit)] = &[
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
];

/// The traffic lights.
pub const WINDOW_BUTTONS: Option<WindowButtons> = Some(WindowButtons {
    corner: 14.,
    inset: 80.,
});

pub const HERDR_RAISE: HerdrRaise = HerdrRaise {
    program: "/usr/bin/open",
    args: &["-a", "Herdr"],
    best_effort: false,
};
