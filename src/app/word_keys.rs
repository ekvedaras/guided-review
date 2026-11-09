//! Some macOS layouts, Lithuanian among them, type nothing for option with an arrow, backspace or
//! forward delete. The macOS text system then takes the key and does nothing with it, so gpui
//! never sees it and moving or deleting by word stops working. The app takes these keys before
//! the text system does and hands them to gpui as the keystroke they are on a US layout.

use gpui::{Keystroke, Modifiers};

const LEFT: u16 = 123;
const RIGHT: u16 = 124;
const BACKSPACE: u16 = 51;
const FORWARD_DELETE: u16 = 117;

#[cfg(target_os = "macos")]
#[expect(
    unsafe_code,
    reason = "objc2 marks the monitor unsafe; its block returns the event it was given or null, as AppKit asks"
)]
pub fn take_before_macos(cx: &mut gpui::App) {
    use futures::StreamExt;
    use objc2_app_kit::{NSEvent, NSEventMask, NSEventModifierFlags};
    use std::ptr::NonNull;

    let (taken, mut to_hand_on) = futures::channel::mpsc::unbounded::<Keystroke>();
    let monitor = block2::RcBlock::new(move |event: NonNull<NSEvent>| -> *mut NSEvent {
        // SAFETY: AppKit hands the monitor a live event for the length of the call.
        let pressed = unsafe { event.as_ref() };
        let flags = pressed.modifierFlags();
        let modifiers = Modifiers {
            alt: flags.contains(NSEventModifierFlags::Option),
            shift: flags.contains(NSEventModifierFlags::Shift),
            control: flags.contains(NSEventModifierFlags::Control),
            platform: flags.contains(NSEventModifierFlags::Command),
            function: false,
        };
        let handed_on = word_keystroke(pressed.keyCode(), modifiers)
            .is_some_and(|keystroke| taken.unbounded_send(keystroke).is_ok());
        if handed_on {
            std::ptr::null_mut()
        } else {
            event.as_ptr()
        }
    });
    // SAFETY: the block returns the event it was given or null.
    let installed = unsafe {
        NSEvent::addLocalMonitorForEventsMatchingMask_handler(NSEventMask::KeyDown, &monitor)
    };
    // The monitor lives as long as the app.
    std::mem::forget(installed);
    cx.spawn(async move |cx| {
        while let Some(keystroke) = to_hand_on.next().await {
            cx.update(|cx| {
                let Some(window) = cx.active_window() else {
                    return;
                };
                // A window that closed meanwhile has nothing to move in.
                window
                    .update(cx, |_, window, cx| window.dispatch_keystroke(keystroke, cx))
                    .ok();
            });
        }
    })
    .detach();
}

#[cfg(not(target_os = "macos"))]
pub fn take_before_macos(_: &mut gpui::App) {}

/// The keystroke to hand gpui for a key the text system would take, or `None` to leave the key alone.
fn word_keystroke(key_code: u16, modifiers: Modifiers) -> Option<Keystroke> {
    if !modifiers.alt || modifiers.control || modifiers.platform || modifiers.function {
        return None;
    }
    let key = match key_code {
        LEFT => "left",
        RIGHT => "right",
        BACKSPACE => "backspace",
        FORWARD_DELETE => "delete",
        _ => return None,
    };
    Some(Keystroke {
        modifiers,
        key: key.to_owned(),
        key_char: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn modifiers(alt: bool, shift: bool) -> Modifiers {
        Modifiers {
            alt,
            shift,
            ..Modifiers::default()
        }
    }

    fn keystroke(key_code: u16, modifiers: Modifiers) -> Option<String> {
        word_keystroke(key_code, modifiers).map(|keystroke| keystroke.unparse())
    }

    #[test]
    fn option_with_an_arrow_backspace_or_delete_is_handed_on_as_its_keystroke() {
        let option = modifiers(true, false);
        assert_eq!(keystroke(LEFT, option).as_deref(), Some("alt-left"));
        assert_eq!(keystroke(RIGHT, option).as_deref(), Some("alt-right"));
        assert_eq!(
            keystroke(BACKSPACE, option).as_deref(),
            Some("alt-backspace")
        );
        assert_eq!(
            keystroke(FORWARD_DELETE, option).as_deref(),
            Some("alt-delete")
        );
        assert_eq!(
            keystroke(LEFT, modifiers(true, true)).as_deref(),
            Some("alt-shift-left")
        );
    }

    #[test]
    fn other_keys_and_other_modifiers_are_left_to_macos() {
        assert_eq!(keystroke(LEFT, modifiers(false, false)), None);
        assert_eq!(keystroke(LEFT, modifiers(false, true)), None);
        assert_eq!(keystroke(0, modifiers(true, false)), None);
        let with_cmd = Modifiers {
            platform: true,
            ..modifiers(true, false)
        };
        assert_eq!(keystroke(LEFT, with_cmd), None);
        let with_control = Modifiers {
            control: true,
            ..modifiers(true, false)
        };
        assert_eq!(keystroke(LEFT, with_control), None);
    }
}
