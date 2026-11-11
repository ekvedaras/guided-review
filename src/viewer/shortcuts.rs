//! Every key the app answers, in one table: what it does, which keys, and where the shortcuts sheet
//! lists it. The app binds its keys from here and the sheet reads the same table, so the two cannot
//! drift apart.

/// One key of a chord as the reader presses it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyCap {
    Cmd,
    Shift,
    Left,
    Right,
    Up,
    Down,
    Enter,
    Escape,
    /// A key with a printed face: what gpui calls it, and what its cap shows.
    Key {
        name: &'static str,
        face: &'static str,
    },
}

const fn key(name: &'static str, face: &'static str) -> KeyCap {
    KeyCap::Key { name, face }
}

const J: KeyCap = key("j", "J");
const K: KeyCap = key("k", "K");
const N: KeyCap = key("n", "N");
const M: KeyCap = key("m", "M");
const C: KeyCap = key("c", "C");
const W: KeyCap = key("w", "W");
const COMMA: KeyCap = key(",", ",");
const QUESTION: KeyCap = key("?", "?");

/// Keys pressed together.
pub type Chord = &'static [KeyCap];

/// Where a command's keys work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyScope {
    /// On the page, while no box takes the typing and no overlay is open.
    Page,
    /// Anywhere in the review's window.
    Window,
    /// In a comment, reply or edit box.
    TextBox,
    /// Anywhere in the review's window but a box that takes typing, which has its own: a comment
    /// box left open elsewhere on the page does not stop it.
    NotTyping,
}

/// The sheet's headings, in the order it shows them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ShortcutGroup {
    Navigate,
    Read,
    Comment,
    View,
    App,
}

impl ShortcutGroup {
    pub const ALL: [Self; 5] = [
        Self::Navigate,
        Self::Read,
        Self::Comment,
        Self::View,
        Self::App,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Self::Navigate => "Navigate",
            Self::Read => "Read",
            Self::Comment => "Comment",
            Self::View => "View",
            Self::App => "App",
        }
    }
}

/// What a shortcut does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Command {
    NextScreen,
    PreviousScreen,
    NextUnreadStep,
    Jump,
    ToggleRead,
    CopyCode,
    AddToReview,
    AskAgent,
    CloseBox,
    WrapLines,
    Settings,
    Shortcuts,
    CloseWindow,
}

impl Command {
    pub const ALL: [Self; 13] = [
        Self::NextScreen,
        Self::PreviousScreen,
        Self::NextUnreadStep,
        Self::Jump,
        Self::ToggleRead,
        Self::CopyCode,
        Self::AddToReview,
        Self::AskAgent,
        Self::CloseBox,
        Self::WrapLines,
        Self::Settings,
        Self::Shortcuts,
        Self::CloseWindow,
    ];

    /// Every way to press it; the first is the one a button shows.
    pub fn chords(self) -> &'static [Chord] {
        use KeyCap::{Cmd, Enter, Escape, Left, Right, Shift};
        match self {
            Self::NextScreen => &[&[Right], &[J]],
            Self::PreviousScreen => &[&[Left], &[K]],
            Self::NextUnreadStep => &[&[N]],
            Self::Jump => &[&[Cmd, K]],
            Self::ToggleRead => &[&[M]],
            Self::CopyCode => &[&[Cmd, C]],
            Self::AddToReview => &[&[Cmd, Enter]],
            Self::AskAgent => &[&[Cmd, Shift, Enter]],
            Self::CloseBox => &[&[Escape]],
            Self::WrapLines => &[&[W]],
            Self::Settings => &[&[Cmd, COMMA]],
            Self::Shortcuts => &[&[QUESTION]],
            Self::CloseWindow => &[&[Cmd, W]],
        }
    }

    pub fn scope(self) -> KeyScope {
        match self {
            Self::NextScreen
            | Self::PreviousScreen
            | Self::NextUnreadStep
            | Self::ToggleRead
            | Self::WrapLines
            | Self::Shortcuts => KeyScope::Page,
            Self::Jump | Self::Settings | Self::CloseWindow => KeyScope::Window,
            Self::AddToReview | Self::AskAgent | Self::CloseBox => KeyScope::TextBox,
            Self::CopyCode => KeyScope::NotTyping,
        }
    }

    pub fn group(self) -> ShortcutGroup {
        match self {
            Self::NextScreen | Self::PreviousScreen | Self::NextUnreadStep | Self::Jump => {
                ShortcutGroup::Navigate
            }
            Self::ToggleRead | Self::CopyCode => ShortcutGroup::Read,
            Self::AddToReview | Self::AskAgent | Self::CloseBox => ShortcutGroup::Comment,
            Self::WrapLines => ShortcutGroup::View,
            Self::Settings | Self::Shortcuts | Self::CloseWindow => ShortcutGroup::App,
        }
    }

    /// What the sheet says it does.
    pub fn label(self) -> &'static str {
        match self {
            Self::NextScreen => "Next screen; marks the step read once you saw its end",
            Self::PreviousScreen => "Previous screen",
            Self::NextUnreadStep => "Next unread step",
            Self::Jump => "Jump to a step, file, symbol or action",
            Self::ToggleRead => "Mark the step as read, or unread",
            Self::CopyCode => "Copy the selected code",
            Self::AddToReview => "Add the comment or reply to your review",
            Self::AskAgent => "Add it and ask your agent",
            Self::CloseBox => "Leave the box you are typing in",
            Self::WrapLines => "Wrap long lines",
            Self::Settings => "Settings",
            Self::Shortcuts => "This list of shortcuts",
            Self::CloseWindow => "Close the window and quit",
        }
    }

    /// The chord a button that does this shows.
    pub fn first_chord(self) -> Chord {
        self.chords().first().copied().unwrap_or_default()
    }
}

/// The sheet's commands under one heading, in table order.
pub fn in_group(group: ShortcutGroup) -> impl Iterator<Item = Command> {
    Command::ALL
        .into_iter()
        .filter(move |command| command.group() == group)
}

/// A chord in gpui's keystroke syntax, such as `cmd-shift-enter`.
pub fn keystroke(chord: Chord) -> String {
    chord
        .iter()
        .map(|cap| match cap {
            KeyCap::Cmd => "cmd",
            KeyCap::Shift => "shift",
            KeyCap::Left => "left",
            KeyCap::Right => "right",
            KeyCap::Up => "up",
            KeyCap::Down => "down",
            KeyCap::Enter => "enter",
            KeyCap::Escape => "escape",
            KeyCap::Key { name, .. } => name,
        })
        .collect::<Vec<_>>()
        .join("-")
}
