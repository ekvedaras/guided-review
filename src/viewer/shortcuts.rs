//! Every command the app answers to a key, in one table: what it does, its default keys, where the
//! keys work and where the shortcuts sheet lists it. The reviewer's `[keys]` change only the keys
//! (`Keymap`); the app binds from the keymap and the sheet reads it, so the two cannot drift apart.

use super::chord::{Chord, Key, Modifier};

const fn key(face: char) -> Chord {
    Chord::key(Key::Printed(face))
}

const fn cmd(face: char) -> Chord {
    key(face).with(Modifier::Cmd)
}

const J: Chord = key('j');
const K: Chord = key('k');
const N: Chord = key('n');
const M: Chord = key('m');
const W: Chord = key('w');
const G: Chord = key('g');
const QUESTION: Chord = key('?');
const LEFT: Chord = Chord::key(Key::Left);
const RIGHT: Chord = Chord::key(Key::Right);
const CMD_ENTER: Chord = Chord::key(Key::Enter).with(Modifier::Cmd);
const CMD_SHIFT_ENTER: Chord = CMD_ENTER.with(Modifier::Shift);
const ESCAPE: Chord = Chord::key(Key::Escape);
const CMD_K: Chord = cmd('k');
const CMD_C: Chord = cmd('c');
const CMD_COMMA: Chord = cmd(',');
const CMD_W: Chord = cmd('w');
const CMD_Q: Chord = cmd('q');

/// Where a command's keys work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyScope {
    /// On the page, while no box takes the typing and no overlay is open.
    Page,
    /// Anywhere in the review's window.
    Window,
    /// In a comment, reply or edit box.
    TextBox,
    /// Anywhere in the app, whichever of its windows has the focus.
    App,
    /// Anywhere in the review's window but a box that takes typing, which has its own: a comment
    /// box left open elsewhere on the page does not stop it.
    NotTyping,
}

impl KeyScope {
    /// Whether one key press can reach both, so one key cannot do two things there.
    pub fn overlaps(self, other: Self) -> bool {
        use KeyScope::{App, NotTyping, Page, TextBox, Window};
        match (self, other) {
            (App | Window, _)
            | (_, App | Window)
            | (TextBox, TextBox)
            | (Page | NotTyping, Page | NotTyping) => true,
            (TextBox, Page | NotTyping) | (Page | NotTyping, TextBox) => false,
        }
    }
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
    OpenOnGitHub,
    CloseWindow,
    Quit,
}

impl Command {
    pub const ALL: [Self; 15] = [
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
        Self::OpenOnGitHub,
        Self::CloseWindow,
        Self::Quit,
    ];

    /// Every way to press it unless the reviewer set other keys; the first is the one a button shows.
    pub fn default_chords(self) -> &'static [Chord] {
        match self {
            Self::NextScreen => &[RIGHT, J],
            Self::PreviousScreen => &[LEFT, K],
            Self::NextUnreadStep => &[N],
            Self::Jump => &[CMD_K],
            Self::ToggleRead => &[M],
            Self::CopyCode => &[CMD_C],
            Self::AddToReview => &[CMD_ENTER],
            Self::AskAgent => &[CMD_SHIFT_ENTER],
            Self::CloseBox => &[ESCAPE],
            Self::WrapLines => &[W],
            Self::Settings => &[CMD_COMMA],
            Self::Shortcuts => &[QUESTION],
            Self::OpenOnGitHub => &[G],
            Self::CloseWindow => &[CMD_W],
            Self::Quit => &[CMD_Q],
        }
    }

    /// Its key in the `[keys]` table of `config.toml`.
    pub fn config_name(self) -> &'static str {
        match self {
            Self::NextScreen => "next_screen",
            Self::PreviousScreen => "previous_screen",
            Self::NextUnreadStep => "next_unread_step",
            Self::Jump => "jump",
            Self::ToggleRead => "toggle_read",
            Self::CopyCode => "copy_code",
            Self::AddToReview => "add_to_review",
            Self::AskAgent => "ask_agent",
            Self::CloseBox => "close_box",
            Self::WrapLines => "wrap_lines",
            Self::Settings => "settings",
            Self::Shortcuts => "shortcuts",
            Self::OpenOnGitHub => "open_on_github",
            Self::CloseWindow => "close_window",
            Self::Quit => "quit",
        }
    }

    pub fn of_config_name(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|command| command.config_name() == name)
    }

    pub fn scope(self) -> KeyScope {
        match self {
            Self::NextScreen
            | Self::PreviousScreen
            | Self::NextUnreadStep
            | Self::ToggleRead
            | Self::WrapLines
            | Self::Shortcuts
            | Self::OpenOnGitHub => KeyScope::Page,
            Self::Jump | Self::Settings | Self::CloseWindow => KeyScope::Window,
            Self::Quit => KeyScope::App,
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
            Self::Settings
            | Self::Shortcuts
            | Self::OpenOnGitHub
            | Self::CloseWindow
            | Self::Quit => ShortcutGroup::App,
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
            Self::OpenOnGitHub => "Open the pull request on GitHub",
            Self::CloseWindow => "Close the window and quit",
            Self::Quit => "Quit",
        }
    }
}

/// The sheet's commands under one heading, in table order.
pub fn in_group(group: ShortcutGroup) -> impl Iterator<Item = Command> {
    Command::ALL
        .into_iter()
        .filter(move |command| command.group() == group)
}
