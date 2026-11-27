//! Every command the app answers to a key, in one table: what it does, its default keys, where the
//! keys work and where the shortcuts sheet lists it. The reviewer's `[keys]` change only the keys
//! (`Keymap`); the app binds from the keymap and the sheet reads it, so the two cannot drift apart.

use super::chord::{Chord, Key, Modifier};
use super::send_action::{SendAction, SendChoice, SendRole};

const fn key(face: char) -> Chord {
    Chord::key(Key::Printed(face))
}

const fn primary(face: char) -> Chord {
    key(face).with(Modifier::Primary)
}

const S: Chord = key('s');
const A: Chord = key('a');
const J: Chord = key('j');
const K: Chord = key('k');
const N: Chord = key('n');
const M: Chord = key('m');
/// Shift as a merge is weightier than marking a step read.
const SHIFT_M: Chord = M.with(Modifier::Shift);
const W: Chord = key('w');
const R: Chord = key('r');
const L: Chord = key('l');
const O: Chord = key('o');
const D: Chord = key('d');
const F: Chord = key('f');
const C: Chord = key('c');
const T: Chord = key('t');
const G: Chord = key('g');
const I: Chord = key('i');
const P: Chord = key('p');
const QUESTION: Chord = key('?');
const OPEN_BRACKET: Chord = key('[');
const CLOSE_BRACKET: Chord = key(']');
const TAB: Chord = Chord::key(Key::Tab);
const SHIFT_TAB: Chord = TAB.with(Modifier::Shift);
const UP: Chord = Chord::key(Key::Up);
const DOWN: Chord = Chord::key(Key::Down);
const LEFT: Chord = Chord::key(Key::Left);
const RIGHT: Chord = Chord::key(Key::Right);
const PRIMARY_ENTER: Chord = Chord::key(Key::Enter).with(Modifier::Primary);
const PRIMARY_SHIFT_ENTER: Chord = PRIMARY_ENTER.with(Modifier::Shift);
const PRIMARY_ALT_ENTER: Chord = PRIMARY_ENTER.with(Modifier::Alt);
const PRIMARY_ALT_SHIFT_ENTER: Chord = PRIMARY_ALT_ENTER.with(Modifier::Shift);
const PRIMARY_ALT_A: Chord = primary('a').with(Modifier::Alt);
const PRIMARY_ALT_S: Chord = primary('s').with(Modifier::Alt);
const PRIMARY_ALT_G: Chord = primary('g').with(Modifier::Alt);
const ESCAPE: Chord = Chord::key(Key::Escape);
const PRIMARY_K: Chord = primary('k');
const PRIMARY_OPEN_BRACKET: Chord = primary('[');
const PRIMARY_CLOSE_BRACKET: Chord = primary(']');
const PRIMARY_P: Chord = primary('p');
const PRIMARY_C: Chord = primary('c');
const PRIMARY_COMMA: Chord = primary(',');
const PRIMARY_W: Chord = primary('w');
const PRIMARY_Q: Chord = primary('q');

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
    HistoryBack,
    HistoryForward,
    NextUnreadStep,
    FindingsList,
    CommentsList,
    FilesList,
    NextChange,
    PreviousChange,
    ReadDown,
    ReadUp,
    Jump,
    ToggleRead,
    CopyCode,
    AddToReview,
    AskAgent,
    CommentNow,
    AskAgentNow,
    SuggestChange,
    CloseBox,
    WrapLines,
    HideWhitespace,
    AskAboutStep,
    MoreAbove,
    MoreBelow,
    ToggleRenderedView,
    TogglePieceLabels,
    ToggleFold,
    Settings,
    Shortcuts,
    OpenOnHost,
    OpenIssue,
    OpenPr,
    SwitchPr,
    ToggleDraft,
    ToggleAutoMerge,
    CloseWindow,
    Quit,
}

impl From<SendAction> for Command {
    fn from(action: SendAction) -> Self {
        match action {
            SendAction::AddToReview => Self::AddToReview,
            SendAction::CommentNow => Self::CommentNow,
            SendAction::SuggestChange => Self::SuggestChange,
            SendAction::AskAgent => Self::AskAgent,
            SendAction::AskAgentNow => Self::AskAgentNow,
        }
    }
}

impl Command {
    /// The send action a command runs in a comment or reply box.
    pub fn send_action(self) -> Option<SendAction> {
        match self {
            Self::AddToReview => Some(SendAction::AddToReview),
            Self::CommentNow => Some(SendAction::CommentNow),
            Self::SuggestChange => Some(SendAction::SuggestChange),
            Self::AskAgent => Some(SendAction::AskAgent),
            Self::AskAgentNow => Some(SendAction::AskAgentNow),
            _ => None,
        }
    }

    /// The keys a command has with no `[keys]` entry. A send action holds Cmd+Enter while it is
    /// the primary one and Cmd+Shift+Enter while it is the secondary one; any other keeps a key of
    /// its own, which no main key can clash with.
    pub fn default_chords_for(self, choice: SendChoice) -> &'static [Chord] {
        let Some(action) = self.send_action() else {
            return self.default_chords();
        };
        match choice.role_of(action) {
            SendRole::Primary => &[PRIMARY_ENTER],
            SendRole::Secondary => &[PRIMARY_SHIFT_ENTER],
            SendRole::Other => match action {
                SendAction::AddToReview => &[PRIMARY_ALT_A],
                SendAction::CommentNow => &[PRIMARY_ALT_ENTER],
                SendAction::SuggestChange => &[PRIMARY_ALT_S],
                SendAction::AskAgent => &[PRIMARY_ALT_G],
                SendAction::AskAgentNow => &[PRIMARY_ALT_SHIFT_ENTER],
            },
        }
    }

    pub const ALL: [Self; 39] = [
        Self::NextScreen,
        Self::PreviousScreen,
        Self::HistoryBack,
        Self::HistoryForward,
        Self::NextUnreadStep,
        Self::FindingsList,
        Self::CommentsList,
        Self::FilesList,
        Self::NextChange,
        Self::PreviousChange,
        Self::ReadDown,
        Self::ReadUp,
        Self::Jump,
        Self::ToggleRead,
        Self::CopyCode,
        Self::AddToReview,
        Self::AskAgent,
        Self::CommentNow,
        Self::AskAgentNow,
        Self::SuggestChange,
        Self::CloseBox,
        Self::WrapLines,
        Self::HideWhitespace,
        Self::AskAboutStep,
        Self::MoreAbove,
        Self::MoreBelow,
        Self::ToggleRenderedView,
        Self::TogglePieceLabels,
        Self::ToggleFold,
        Self::Settings,
        Self::Shortcuts,
        Self::OpenOnHost,
        Self::OpenIssue,
        Self::OpenPr,
        Self::SwitchPr,
        Self::ToggleDraft,
        Self::ToggleAutoMerge,
        Self::CloseWindow,
        Self::Quit,
    ];

    /// Every way to press it unless the reviewer set other keys; the first is the one a button shows.
    pub fn default_chords(self) -> &'static [Chord] {
        match self {
            Self::NextScreen => &[RIGHT, J],
            Self::PreviousScreen => &[LEFT, K],
            Self::HistoryBack => &[PRIMARY_OPEN_BRACKET],
            Self::HistoryForward => &[PRIMARY_CLOSE_BRACKET],
            Self::NextUnreadStep => &[N],
            Self::FindingsList => &[F],
            Self::CommentsList => &[C],
            Self::FilesList => &[T],
            Self::NextChange => &[TAB],
            Self::PreviousChange => &[SHIFT_TAB],
            Self::ReadDown => &[DOWN],
            Self::ReadUp => &[UP],
            Self::MoreAbove => &[OPEN_BRACKET],
            Self::MoreBelow => &[CLOSE_BRACKET],
            Self::ToggleRenderedView => &[R],
            Self::TogglePieceLabels => &[L],
            Self::ToggleFold => &[O],
            Self::Jump => &[PRIMARY_K],
            Self::ToggleRead => &[M],
            Self::CopyCode => &[PRIMARY_C],
            Self::AddToReview
            | Self::AskAgent
            | Self::CommentNow
            | Self::AskAgentNow
            | Self::SuggestChange => self.default_chords_for(SendChoice::default()),
            Self::CloseBox => &[ESCAPE],
            Self::WrapLines => &[W],
            Self::HideWhitespace => &[S],
            Self::AskAboutStep => &[A],
            Self::Settings => &[PRIMARY_COMMA],
            Self::Shortcuts => &[QUESTION],
            Self::OpenOnHost => &[G],
            Self::OpenIssue => &[I],
            Self::OpenPr => &[P],
            Self::SwitchPr => &[PRIMARY_P],
            Self::ToggleDraft => &[D],
            Self::ToggleAutoMerge => &[SHIFT_M],
            Self::CloseWindow => &[PRIMARY_W],
            Self::Quit => &[PRIMARY_Q],
        }
    }

    /// Its key in the `[keys]` table of `config.toml`.
    pub fn config_name(self) -> &'static str {
        match self {
            Self::NextScreen => "next_screen",
            Self::PreviousScreen => "previous_screen",
            Self::HistoryBack => "history_back",
            Self::HistoryForward => "history_forward",
            Self::NextUnreadStep => "next_unread_step",
            Self::FindingsList => "findings_list",
            Self::CommentsList => "comments_list",
            Self::FilesList => "files_list",
            Self::NextChange => "next_change",
            Self::PreviousChange => "previous_change",
            Self::ReadDown => "read_down",
            Self::ReadUp => "read_up",
            Self::Jump => "jump",
            Self::ToggleRead => "toggle_read",
            Self::CopyCode => "copy_code",
            Self::AddToReview => "add_to_review",
            Self::AskAgent => "ask_agent",
            Self::CommentNow => "comment_now",
            Self::AskAgentNow => "ask_agent_now",
            Self::SuggestChange => "suggest_change",
            Self::CloseBox => "close_box",
            Self::WrapLines => "wrap_lines",
            Self::HideWhitespace => "hide_whitespace",
            Self::AskAboutStep => "ask_about_step",
            Self::MoreAbove => "more_above",
            Self::MoreBelow => "more_below",
            Self::ToggleRenderedView => "toggle_rendered_view",
            Self::TogglePieceLabels => "toggle_piece_labels",
            Self::ToggleFold => "toggle_fold",
            Self::Settings => "settings",
            Self::Shortcuts => "shortcuts",
            Self::OpenOnHost => "open_on_host",
            Self::OpenIssue => "open_issue",
            Self::OpenPr => "open_pr",
            Self::SwitchPr => "switch_pr",
            Self::ToggleDraft => "toggle_draft",
            Self::ToggleAutoMerge => "toggle_auto_merge",
            Self::CloseWindow => "close_window",
            Self::Quit => "quit",
        }
    }

    /// A name the command was written under before, still read: `open_on_github`, from before the
    /// app named code hosts. Settings writes the new name in its place.
    pub fn former_config_name(self) -> Option<&'static str> {
        match self {
            Self::OpenOnHost => Some("open_on_github"),
            _ => None,
        }
    }

    pub fn of_config_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|command| {
            command.config_name() == name || command.former_config_name() == Some(name)
        })
    }

    pub fn scope(self) -> KeyScope {
        match self {
            Self::NextScreen
            | Self::PreviousScreen
            | Self::NextUnreadStep
            | Self::FindingsList
            | Self::CommentsList
            | Self::FilesList
            | Self::NextChange
            | Self::PreviousChange
            | Self::ReadDown
            | Self::ReadUp
            | Self::ToggleRead
            | Self::WrapLines
            | Self::HideWhitespace
            | Self::AskAboutStep
            | Self::MoreAbove
            | Self::MoreBelow
            | Self::ToggleRenderedView
            | Self::TogglePieceLabels
            | Self::ToggleFold
            | Self::Shortcuts
            | Self::OpenOnHost
            | Self::OpenIssue
            | Self::OpenPr
            | Self::ToggleDraft
            | Self::ToggleAutoMerge => KeyScope::Page,
            Self::Jump
            | Self::HistoryBack
            | Self::HistoryForward
            | Self::SwitchPr
            | Self::Settings
            | Self::CloseWindow => KeyScope::Window,
            Self::Quit => KeyScope::App,
            Self::AddToReview
            | Self::AskAgent
            | Self::CommentNow
            | Self::AskAgentNow
            | Self::SuggestChange
            | Self::CloseBox => KeyScope::TextBox,
            Self::CopyCode => KeyScope::NotTyping,
        }
    }

    pub fn group(self) -> ShortcutGroup {
        match self {
            Self::NextScreen
            | Self::PreviousScreen
            | Self::HistoryBack
            | Self::HistoryForward
            | Self::NextUnreadStep
            | Self::FindingsList
            | Self::CommentsList
            | Self::FilesList
            | Self::NextChange
            | Self::PreviousChange
            | Self::ReadDown
            | Self::ReadUp
            | Self::Jump => ShortcutGroup::Navigate,
            Self::ToggleRead | Self::CopyCode | Self::AskAboutStep => ShortcutGroup::Read,
            Self::AddToReview
            | Self::AskAgent
            | Self::CommentNow
            | Self::AskAgentNow
            | Self::SuggestChange
            | Self::CloseBox => ShortcutGroup::Comment,
            Self::WrapLines
            | Self::HideWhitespace
            | Self::MoreAbove
            | Self::MoreBelow
            | Self::ToggleRenderedView
            | Self::TogglePieceLabels
            | Self::ToggleFold => ShortcutGroup::View,
            Self::Settings
            | Self::Shortcuts
            | Self::OpenOnHost
            | Self::OpenIssue
            | Self::OpenPr
            | Self::SwitchPr
            | Self::ToggleDraft
            | Self::ToggleAutoMerge
            | Self::CloseWindow
            | Self::Quit => ShortcutGroup::App,
        }
    }

    /// What the sheet says it does.
    pub fn label(self) -> &'static str {
        match self {
            Self::NextScreen => "Next screen; marks the step read once you saw its end",
            Self::PreviousScreen => "Previous screen",
            Self::HistoryBack => "Back to where you came from, like the mouse's back button",
            Self::HistoryForward => "Forward again after Back, like the mouse's forward button",
            Self::NextUnreadStep => "Next unread step",
            Self::FindingsList => "List the findings of the AI review",
            Self::CommentsList => "List every comment thread, to go to one",
            Self::FilesList => "List the changed files as a tree, to go to one",
            Self::NextChange => "Focus the next change",
            Self::PreviousChange => "Focus the previous change",
            Self::ReadDown => "Scroll down through the focused change, then focus the next",
            Self::ReadUp => "Scroll up through the focused change, then focus the previous",
            Self::Jump => "Jump to a step, file, symbol or action",
            Self::ToggleRead => "Mark the step as read, or unread",
            Self::CopyCode => "Copy the selected code",
            Self::AddToReview => "Add the comment or reply to your review",
            Self::AskAgent => "Add it and ask your agent",
            Self::CommentNow => "Post the comment or reply at once, without your review",
            Self::AskAgentNow => "Post it at once and ask your agent",
            Self::SuggestChange => "Put the commented lines in the box as a suggestion",
            Self::CloseBox => "Leave the box you are typing in",
            Self::WrapLines => "Wrap long lines",
            Self::HideWhitespace => "Hide whitespace-only changes",
            Self::AskAboutStep => "Ask your agent about this step",
            Self::MoreAbove => "Show more lines above the focused change",
            Self::MoreBelow => "Show more lines below the focused change",
            Self::ToggleRenderedView => "Show the focused change's file rendered, or as code",
            Self::TogglePieceLabels => "Show or hide the piece labels in the focused change's file",
            Self::ToggleFold => "Open all the pieces of a folded step, or fold them again",
            Self::Settings => "Settings",
            Self::Shortcuts => "This list of shortcuts",
            Self::OpenOnHost => "Open the pull request on its code host",
            Self::OpenIssue => "Open the related issue, or list them when there are several",
            Self::SwitchPr => "Switch to another pull request or local review",
            Self::OpenPr => "Ask your agent to open a pull request for the branch",
            Self::ToggleDraft => {
                "Convert the pull request to a draft, or mark it ready; press again to confirm"
            }
            Self::ToggleAutoMerge => {
                "Turn auto-merge on or off, or merge now when nothing blocks it; press again to confirm"
            }
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
