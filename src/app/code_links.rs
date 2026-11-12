//! Code links and the links of markdown text open only through here, so where code opens is the
//! `CodeOpener`'s decision and never a view's.

use gpui::{App, Global, SharedString};

use crate::app::active_session::session_of;
use crate::app::components::icon::Icon;
use crate::ide::IdeLaunch;
use crate::viewer::{CodeLink, CodeOpener, Destination, Opened};

struct ActiveCodeOpener(CodeOpener);

impl Global for ActiveCodeOpener {}

/// Set again whenever the review changes: its head and whether the clone is at it decide where code opens.
pub fn use_code_opener(opener: CodeOpener, cx: &mut App) {
    cx.set_global(ActiveCodeOpener(opener));
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

/// The icon beside that label: code for the IDE, a link out for the browser.
pub fn open_code_icon(link: &CodeLink, cx: &App) -> Icon {
    match cx.global::<ActiveCodeOpener>().0.destination(link) {
        Destination::Ide { .. } => Icon::Code,
        Destination::GitHub { .. } | Destination::Browser(_) => Icon::ExternalLink,
    }
}

/// For a tooltip on what opens `link`: where it goes, why not the IDE, or what to mind there.
pub fn open_code_hint(link: &CodeLink, cx: &App) -> SharedString {
    cx.global::<ActiveCodeOpener>()
        .0
        .destination(link)
        .explanation()
        .into()
}

/// A file of the reviewer's own, outside any clone, in the IDE they set.
#[allow(clippy::disallowed_methods, reason = "opens a URL, beside `open`")]
pub fn open_file_in_ide(launch: &IdeLaunch, cx: &mut App) -> std::io::Result<()> {
    launch.run(|url| cx.open_url(url))
}

#[allow(clippy::disallowed_methods, reason = "the one place that opens a URL")]
fn open(destination: &Destination, cx: &mut App) {
    let Some(session) = session_of(cx) else {
        destination.open(|url| cx.open_url(url));
        return;
    };
    match destination.open(|url| cx.open_url(url)) {
        Opened::InIde => session.update(cx, |session, cx| {
            session.change(cx, |review| review.ide_started());
        }),
        Opened::IdeNotStarted(reason) => session.update(cx, |session, cx| {
            session.change(cx, |review| review.ide_not_started(&reason));
        }),
        Opened::Elsewhere => {}
    }
}
