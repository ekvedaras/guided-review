//! The session a window shows, reached from the code that draws and clicks without being handed it.
//! Held weakly, so a replaced session is freed and not kept alive by the app.

use gpui::{App, Entity, Global, WeakEntity};

use crate::app::review_session::ReviewSession;

struct ActiveSession(WeakEntity<ReviewSession>);

impl Global for ActiveSession {}

pub fn use_active_session(session: &Entity<ReviewSession>, cx: &mut App) {
    cx.set_global(ActiveSession(session.downgrade()));
}

pub fn session_of(cx: &App) -> Option<Entity<ReviewSession>> {
    cx.try_global::<ActiveSession>()?.0.upgrade()
}

pub fn details_are_open(key: &str, cx: &App) -> bool {
    session_of(cx).is_some_and(|session| session.read(cx).is_details_open(key))
}

pub fn toggle_details(key: &str, cx: &mut App) {
    if let Some(session) = session_of(cx) {
        session.update(cx, |session, cx| session.toggle_details(key, cx));
    }
}
