//! Code links and the links of markdown text open only through here, so where code opens is the
//! `CodeOpener`'s decision and never a view's.

use gpui::{App, Global, SharedString};

use gpui::Entity;

use crate::app::review_session::ReviewSession;
use crate::viewer::{CodeLink, CodeOpener, Destination, Opened, Review};

struct ActiveCodeOpener(CodeOpener, Entity<ReviewSession>);

impl Global for ActiveCodeOpener {}

/// Set again whenever the review changes: its head and whether the clone is at it decide where code opens.
pub fn use_code_opener(opener: CodeOpener, session: Entity<ReviewSession>, cx: &mut App) {
    cx.set_global(ActiveCodeOpener(opener, session));
}

pub fn open_code(link: &CodeLink, cx: &mut App) {
    let destination = cx.global::<ActiveCodeOpener>().0.destination(link);
    open(&destination, cx);
}

pub fn open_link(url: &str, cx: &mut App) {
    let destination = cx.global::<ActiveCodeOpener>().0.destination_of_url(url);
    open(&destination, cx);
}

/// "Open in IDE" or "Open on GitHub", for the button that opens `link`.
pub fn open_code_label(link: &CodeLink, cx: &App) -> &'static str {
    cx.global::<ActiveCodeOpener>().0.destination(link).label()
}

/// For a tooltip on what opens `link`: where it goes, why not the IDE, or what to mind there.
pub fn open_code_hint(link: &CodeLink, cx: &App) -> SharedString {
    cx.global::<ActiveCodeOpener>()
        .0
        .destination(link)
        .explanation()
        .into()
}

#[allow(clippy::disallowed_methods, reason = "the one place that opens a URL")]
fn open(destination: &Destination, cx: &mut App) {
    let session = cx.global::<ActiveCodeOpener>().1.clone();
    match destination.open(|url| cx.open_url(url)) {
        Opened::InIde => session.update(cx, |session, cx| {
            session.change(cx, Review::ide_started);
        }),
        Opened::IdeNotStarted(reason) => session.update(cx, |session, cx| {
            session.change(cx, |review| review.ide_not_started(&reason));
        }),
        Opened::Elsewhere => {}
    }
}
