pub mod desktop_file;

use super::{Adapter, HerdrRaise, KeySpelling, PrimaryKey, TextEdit};

pub(super) const ADAPTER: Adapter = Adapter {
    triple_system: "unknown-linux-gnu",
    // Desktops differ in what they install; the last comes with most.
    text_fonts: &[
        "Adwaita Sans",
        "Noto Sans",
        "Liberation Sans",
        "DejaVu Sans",
    ],
    code_fonts: &[
        "Adwaita Mono",
        "Noto Sans Mono",
        "Liberation Mono",
        "DejaVu Sans Mono",
    ],
    primary_key: PrimaryKey::Control,
    key_spelling: KeySpelling {
        primary: "Ctrl",
        ctrl: "Ctrl",
        alt: "Alt",
        shift: "Shift",
        joiner: "+",
    },
    text_keys: &[
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
    ],
    // The window manager draws them, or none, as Hyprland does.
    window_buttons: None,
    // Linux desktops have no app menu bar.
    app_menu: None,
    // Hyprland's; other desktops have no such command, so Herdr stays where it is there.
    herdr_raise: HerdrRaise {
        program: "hyprctl",
        args: &["dispatch", "focuswindow", "class:herdr-app"],
        best_effort: true,
    },
};
