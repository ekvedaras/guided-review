pub mod desktop_file;

use std::sync::Arc;

use super::{Adapter, FontChoice, HerdrRaise, KeySpelling, PrimaryKey, RaiseFailed, TextEdit};
use desktop_file::DesktopFile;

pub(super) const ADAPTER: Adapter = Adapter {
    triple_system: "unknown-linux-gnu",
    // Desktops differ in what they install; the last comes with most.
    text_font: FontChoice::FirstInstalled(&[
        "Adwaita Sans",
        "Noto Sans",
        "Liberation Sans",
        "DejaVu Sans",
    ]),
    code_font: FontChoice::FirstInstalled(&[
        "Adwaita Mono",
        "Noto Sans Mono",
        "Liberation Mono",
        "DejaVu Sans Mono",
    ]),
    // None: usvg drew Noto Color Emoji with no colour on Ubuntu CI (2026-11-25), so an SVG's emoji
    // keep usvg's own fallback here, as before.
    emoji_families: &[],
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
    // Hyprland's; other desktops have no such command, so Herdr stays where it is there. Since
    // Hyprland 0.56 a dispatch is Lua; the old `focuswindow class:...` form fails with exit 7.
    herdr_raise: HerdrRaise {
        program: "hyprctl",
        args: &[
            "dispatch",
            r#"hl.dsp.focus({ window = "class:herdr-app" })"#,
        ],
        when_it_fails: RaiseFailed::LeavesHerdrBehind,
    },
    desktop_entry: |home, bin_link| Arc::new(DesktopFile::in_home(home, bin_link)),
};
