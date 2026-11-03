//! Code links and the links of markdown text open only through here, so where code opens is the
//! `CodeOpener`'s decision and never a view's.

use gpui::{App, Global};

use crate::viewer::{CodeLink, CodeOpener, Destination};

struct ActiveCodeOpener(CodeOpener);

impl Global for ActiveCodeOpener {}

/// Set again whenever the review is opened anew, since its head is what relative links open at.
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

fn open(destination: &Destination, cx: &mut App) {
    destination.open(|url| cx.open_url(url));
}
