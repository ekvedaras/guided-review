//! A key with the modifiers held down for it, as the reviewer presses it, writes it in
//! `config.toml` and sees it on a key cap.

use crate::platform::{Os, PrimaryKey};

/// Held down while the key is pressed. The order is the order gpui and the caps show them in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Modifier {
    /// The key the app's own shortcuts are held with: Cmd on macOS, Ctrl on Linux.
    Primary,
    Ctrl,
    Alt,
    Shift,
}

impl Modifier {
    const ALL: [Self; 4] = [Self::Primary, Self::Ctrl, Self::Alt, Self::Shift];

    const fn bit(self) -> u8 {
        match self {
            Self::Primary => 1,
            Self::Ctrl => 2,
            Self::Alt => 4,
            Self::Shift => 8,
        }
    }

    /// How `config.toml` writes it.
    fn name(self) -> &'static str {
        match self {
            Self::Primary => "primary",
            Self::Ctrl => "ctrl",
            Self::Alt => "alt",
            Self::Shift => "shift",
        }
    }

    /// What gpui binds it as: gpui's `secondary` is Cmd on macOS and Ctrl elsewhere.
    fn gpui_name(self) -> &'static str {
        match self {
            Self::Primary => "secondary",
            other => other.name(),
        }
    }

    /// `cmd` is how keys were saved before there was a Primary modifier. Where Primary is Ctrl, a
    /// written `ctrl` is Primary too, so the two can never bind one key twice.
    fn of_name(name: &str, os: Os) -> Option<Self> {
        match (name, os.primary_key()) {
            ("cmd", _) | ("ctrl", PrimaryKey::Control) => Some(Self::Primary),
            _ => Self::ALL
                .into_iter()
                .find(|modifier| modifier.name() == name),
        }
    }

    /// What its cap shows and a line of text spells it as.
    pub fn face(self, os: Os) -> &'static str {
        let spelling = os.key_spelling();
        match self {
            Self::Primary => spelling.primary,
            Self::Ctrl => spelling.ctrl,
            Self::Alt => spelling.alt,
            Self::Shift => spelling.shift,
        }
    }
}

/// The one key that is not a modifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Key {
    Left,
    Right,
    Up,
    Down,
    Enter,
    Escape,
    Tab,
    Space,
    Backspace,
    Delete,
    /// A printed key, lower case for a letter: what gpui calls it.
    Printed(char),
}

impl Key {
    const NAMED: [Self; 10] = [
        Self::Left,
        Self::Right,
        Self::Up,
        Self::Down,
        Self::Enter,
        Self::Escape,
        Self::Tab,
        Self::Space,
        Self::Backspace,
        Self::Delete,
    ];

    /// Only ASCII keys, so that each has a face to show; letters are kept in lower case.
    pub fn printed(face: char) -> Option<Self> {
        (face.is_ascii_graphic()).then(|| Self::Printed(face.to_ascii_lowercase()))
    }

    /// What gpui calls the key.
    pub fn name(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Right => "right",
            Self::Up => "up",
            Self::Down => "down",
            Self::Enter => "enter",
            Self::Escape => "escape",
            Self::Tab => "tab",
            Self::Space => "space",
            Self::Backspace => "backspace",
            Self::Delete => "delete",
            Self::Printed(face) => ascii_text(face),
        }
    }

    /// What its cap shows when it has no drawn arrow: a letter in upper case.
    pub fn face(self) -> &'static str {
        match self {
            Self::Escape => "esc",
            Self::Tab => "tab",
            Self::Space => "space",
            Self::Backspace => "⌫",
            Self::Delete => "⌦",
            Self::Printed(face) => ascii_text(face.to_ascii_uppercase()),
            Self::Left | Self::Right | Self::Up | Self::Down | Self::Enter => self.name(),
        }
    }

    pub fn of_name(name: &str) -> Option<Self> {
        if let Some(named) = Self::NAMED.into_iter().find(|key| key.name() == name) {
            return Some(named);
        }
        let mut chars = name.chars();
        match (chars.next(), chars.next()) {
            (Some(face), None) => Self::printed(face),
            _ => None,
        }
    }
}

/// Every printed ASCII key in code order, so a key's text is a `&'static str`.
const ASCII_GRAPHIC: &str = "!\"#$%&'()*+,-./0123456789:;<=>?@ABCDEFGHIJKLMNOPQRSTUVWXYZ[\\]^_`abcdefghijklmnopqrstuvwxyz{|}~";

fn ascii_text(face: char) -> &'static str {
    let at = usize::try_from(u32::from(face).saturating_sub(u32::from('!'))).unwrap_or(usize::MAX);
    ASCII_GRAPHIC.get(at..=at).unwrap_or("?")
}

/// The modifiers held down, as a set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Modifiers(u8);

impl Modifiers {
    pub const NONE: Self = Self(0);

    #[must_use]
    pub const fn with(self, modifier: Modifier) -> Self {
        Self(self.0 | modifier.bit())
    }

    pub fn holds(self, modifier: Modifier) -> bool {
        self.0 & modifier.bit() != 0
    }

    pub fn iter(self) -> impl Iterator<Item = Modifier> {
        Modifier::ALL
            .into_iter()
            .filter(move |modifier| self.holds(*modifier))
    }
}

/// One thing a cap shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyCap {
    Modifier(Modifier),
    Key(Key),
}

/// Keys pressed together: any modifiers and one key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Chord {
    modifiers: Modifiers,
    key: Key,
}

impl Chord {
    pub const fn new(modifiers: Modifiers, key: Key) -> Self {
        Self { modifiers, key }
    }

    pub const fn key(key: Key) -> Self {
        Self::new(Modifiers::NONE, key)
    }

    #[must_use]
    pub const fn with(self, modifier: Modifier) -> Self {
        Self::new(self.modifiers.with(modifier), self.key)
    }

    /// The text `config.toml` writes, such as `primary-shift-enter`, with the modifiers in any
    /// order, read as this OS reads it.
    pub fn parse(text: &str) -> Option<Self> {
        Self::parse_on(Os::RUNNING, text)
    }

    pub fn parse_on(os: Os, text: &str) -> Option<Self> {
        let (held, key) = match (text, text.strip_suffix("--")) {
            ("-", _) => ("", "-"),
            (_, Some(held)) => (held, "-"),
            _ => text.rsplit_once('-').unwrap_or(("", text)),
        };
        let modifiers = held
            .split('-')
            .filter(|name| !name.is_empty() || !held.is_empty())
            .try_fold(Modifiers::NONE, |modifiers, name| {
                Some(modifiers.with(Modifier::of_name(name, os)?))
            })?;
        Some(Self::new(modifiers, Key::of_name(key)?))
    }

    /// The text `config.toml` writes, with the modifiers in their one order.
    pub fn written(self) -> String {
        self.joined(Modifier::name)
    }

    /// gpui's keystroke text, which binds the chord.
    pub fn keystroke(self) -> String {
        self.joined(Modifier::gpui_name)
    }

    fn joined(self, modifier_name: fn(Modifier) -> &'static str) -> String {
        self.modifiers
            .iter()
            .map(modifier_name)
            .chain(std::iter::once(self.key.name()))
            .collect::<Vec<_>>()
            .join("-")
    }

    /// The chord as a line of text says it on this OS: `⌘⇧↵` or `Ctrl+Shift+↵`, `W`, `esc`.
    pub fn spelled(self) -> String {
        self.spelled_on(Os::RUNNING)
    }

    pub fn spelled_on(self, os: Os) -> String {
        self.caps()
            .map(|cap| match cap {
                KeyCap::Modifier(modifier) => modifier.face(os),
                KeyCap::Key(Key::Left) => "←",
                KeyCap::Key(Key::Right) => "→",
                KeyCap::Key(Key::Up) => "↑",
                KeyCap::Key(Key::Down) => "↓",
                KeyCap::Key(Key::Enter) => "↵",
                KeyCap::Key(key) => key.face(),
            })
            .collect::<Vec<_>>()
            .join(os.key_spelling().joiner)
    }

    pub fn caps(self) -> impl Iterator<Item = KeyCap> {
        self.modifiers
            .iter()
            .map(KeyCap::Modifier)
            .chain(std::iter::once(KeyCap::Key(self.key)))
    }
}
