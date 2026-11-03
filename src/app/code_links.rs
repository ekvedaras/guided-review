//! Code links and the links of markdown text open only through here, so where code opens is the
//! `CodeOpener`'s decision and never a view's.

use gpui::{App, Global, SharedString};

use crate::viewer::{CodeLink, CodeOpener, Destination};

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

/// For a tooltip on what opens `link`: where it goes and, for GitHub, why not the IDE.
pub fn open_code_hint(link: &CodeLink, cx: &App) -> SharedString {
    let destination = cx.global::<ActiveCodeOpener>().0.destination(link);
    destination
        .explanation()
        .unwrap_or_else(|| destination.label().to_string())
        .into()
}

#[allow(clippy::disallowed_methods, reason = "the one place that opens a URL")]
fn open(destination: &Destination, cx: &mut App) {
    destination.open(|url| cx.open_url(url));
}
