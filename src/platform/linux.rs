use super::{KeySpelling, PrimaryKey, TextEdit, WindowButtons};

pub const TRIPLE_SYSTEM: &str = "unknown-linux-gnu";

/// Best first. Desktops differ in what they install; the last comes with most.
pub const TEXT_FONTS: &[&str] = &[
    "Adwaita Sans",
    "Noto Sans",
    "Liberation Sans",
    "DejaVu Sans",
];
pub const CODE_FONTS: &[&str] = &[
    "Adwaita Mono",
    "Noto Sans Mono",
    "Liberation Mono",
    "DejaVu Sans Mono",
];

pub const PRIMARY_KEY: PrimaryKey = PrimaryKey::Control;

pub const KEY_SPELLING: KeySpelling = KeySpelling {
    primary: "Ctrl",
    ctrl: "Ctrl",
    alt: "Alt",
    shift: "Shift",
    joiner: "+",
};

pub const TEXT_KEYS: &[(&str, TextEdit)] = &[
    ("ctrl-backspace", TextEdit::BackspaceWord),
    ("ctrl-delete", TextEdit::DeleteWord),
    ("ctrl-left", TextEdit::WordLeft),
    ("ctrl-right", TextEdit::WordRight),
    ("ctrl-home", TextEdit::TextStart),
    ("ctrl-end", TextEdit::TextEnd),
    ("ctrl-shift-left", TextEdit::SelectWordLeft),
    ("ctrl-shift-right", TextEdit::SelectWordRight),
    ("shift-home", TextEdit::SelectLineStart),
    ("shift-end", TextEdit::SelectLineEnd),
    ("ctrl-a", TextEdit::SelectAll),
    ("ctrl-c", TextEdit::Copy),
    ("ctrl-x", TextEdit::Cut),
    ("ctrl-v", TextEdit::Paste),
];

/// The window manager draws them, or none, as Hyprland does.
pub const WINDOW_BUTTONS: Option<WindowButtons> = None;
