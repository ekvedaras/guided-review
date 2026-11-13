//! A key with the modifiers held down for it, as the reviewer presses it, writes it in
//! `config.toml` and sees it on a key cap.

/// Held down while the key is pressed. The order is the order gpui and the caps show them in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Modifier {
    Cmd,
    Ctrl,
    Alt,
    Shift,
}

impl Modifier {
    const ALL: [Self; 4] = [Self::Cmd, Self::Ctrl, Self::Alt, Self::Shift];

    const fn bit(self) -> u8 {
        match self {
            Self::Cmd => 1,
            Self::Ctrl => 2,
            Self::Alt => 4,
            Self::Shift => 8,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Cmd => "cmd",
            Self::Ctrl => "ctrl",
            Self::Alt => "alt",
            Self::Shift => "shift",
        }
    }

    fn of_name(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|modifier| modifier.name() == name)
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

    /// gpui's keystroke text, such as `cmd-shift-enter`, with the modifiers in any order.
    pub fn parse(text: &str) -> Option<Self> {
        let (held, key) = match (text, text.strip_suffix("--")) {
            ("-", _) => ("", "-"),
            (_, Some(held)) => (held, "-"),
            _ => text.rsplit_once('-').unwrap_or(("", text)),
        };
        let modifiers = held
            .split('-')
            .filter(|name| !name.is_empty() || !held.is_empty())
            .try_fold(Modifiers::NONE, |modifiers, name| {
                Some(modifiers.with(Modifier::of_name(name)?))
            })?;
        Some(Self::new(modifiers, Key::of_name(key)?))
    }

    /// gpui's keystroke text, with the modifiers in their one order.
    pub fn keystroke(self) -> String {
        self.modifiers
            .iter()
            .map(Modifier::name)
            .chain(std::iter::once(self.key.name()))
            .collect::<Vec<_>>()
            .join("-")
    }

    pub fn caps(self) -> impl Iterator<Item = KeyCap> {
        self.modifiers
            .iter()
            .map(KeyCap::Modifier)
            .chain(std::iter::once(KeyCap::Key(self.key)))
    }
}
