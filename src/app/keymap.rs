use std::sync::{Arc, LazyLock};

use gpui::{App, Global, Keystroke};

use crate::config::Config;
use crate::viewer::{Chord, Key, Keymap, Modifier, ResolvedKeymap};

/// The keys the app is bound to now, which buttons, the shortcuts sheet and the palette show.
pub struct AppKeymap(Arc<Keymap>);

impl Global for AppKeymap {}

static DEFAULTS: LazyLock<Arc<Keymap>> = LazyLock::new(Arc::default);

pub fn keymap(cx: &App) -> &Keymap {
    shared(cx)
}

/// For a render that needs the keys while it borrows `cx` again.
pub fn shared_keymap(cx: &App) -> Arc<Keymap> {
    Arc::clone(shared(cx))
}

fn shared(cx: &App) -> &Arc<Keymap> {
    cx.try_global::<AppKeymap>()
        .map_or(&*DEFAULTS, |current| &current.0)
}

/// The config's keymap with what is wrong in it, which Settings shows. While the config cannot
/// be read at all the keys are the defaults.
pub fn configured_keymap() -> ResolvedKeymap {
    let config = Config::load().unwrap_or_default();
    config.resolved_keymap()
}

pub fn set_keymap(keymap: Keymap, cx: &mut App) {
    cx.set_global(AppKeymap(Arc::new(keymap)));
}

/// The chord a key press is. A printed key that Shift changes, such as `?`, is that key alone, as
/// gpui binds it.
pub fn chord_of(keystroke: &Keystroke) -> Option<Chord> {
    let modifiers = &keystroke.modifiers;
    let shifted_face = keystroke
        .key_char
        .as_deref()
        .filter(|_| modifiers.shift)
        .and_then(Key::of_name)
        .filter(|key| matches!(key, Key::Printed(face) if !face.is_ascii_alphanumeric()));
    let (key, shift_is_in_the_key) = match shifted_face {
        Some(key) => (key, true),
        None => (Key::of_name(&keystroke.key)?, false),
    };
    let held = [
        (modifiers.platform, Modifier::Cmd),
        (modifiers.control, Modifier::Ctrl),
        (modifiers.alt, Modifier::Alt),
        (modifiers.shift && !shift_is_in_the_key, Modifier::Shift),
    ];
    Some(
        held.into_iter()
            .filter(|(is_held, _)| *is_held)
            .fold(Chord::key(key), |chord, (_, modifier)| chord.with(modifier)),
    )
}
