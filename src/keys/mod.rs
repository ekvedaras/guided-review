//! What each key does: the table of commands and their default keys, the chords a key is written
//! as, the reviewer's changes from `config.toml` and the send actions a comment box binds to keys.

mod chord;
mod keymap;
mod send_action;
mod shortcuts;

pub use chord::{Chord, Key, KeyCap, Modifier, Modifiers};
pub use keymap::{KeyChanges, KeyProblem, Keymap, PlannedSend, ResolvedKeymap, WrittenKeys};
pub use send_action::{SendAction, SendChoice, SendRole};
pub use shortcuts::{Command, KeyScope, ShortcutGroup, in_group};
