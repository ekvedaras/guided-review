//! The one scrollbar look: a thumb on the right edge of what scrolls, that shows where the view is.
//! The page, the PR switcher and the issue card all draw it here, so they look the same.

use gpui::prelude::FluentBuilder;
use gpui::{Div, ElementId, InteractiveElement, ParentElement, Pixels, Stateful, Styled, div, px};

use super::palette::Colors;

/// The track's width at the right edge; the thumb sits in its middle.
pub const TRACK_WIDTH: Pixels = px(12.);
const THUMB_WIDTH: Pixels = px(6.);

/// The track and its thumb, quiet at rest and brighter under the pointer or while dragged. The
/// caller places the track (its top and bottom, which must span the `track` the thumb was made for)
/// and adds a drag when it has one.
pub fn scrollbar(
    colors: &Colors,
    id: impl Into<ElementId>,
    thumb: ScrollThumb,
    is_dragging: bool,
) -> Stateful<Div> {
    let id = id.into();
    let group = gpui::SharedString::from(format!("{id}"));
    div()
        .id(id)
        .group(group.clone())
        .absolute()
        .right_0()
        .w(TRACK_WIDTH)
        .child(
            div()
                .absolute()
                .top(thumb.top)
                .right((TRACK_WIDTH - THUMB_WIDTH) / 2.)
                .w(THUMB_WIDTH)
                .h(thumb.height)
                .rounded_full()
                .bg(colors.muted_text.opacity(0.3))
                .when(is_dragging, |thumb| {
                    thumb.bg(colors.muted_text.opacity(0.6))
                })
                .group_hover(group, |thumb| thumb.bg(colors.muted_text.opacity(0.6))),
        )
}

const MIN_THUMB_HEIGHT: Pixels = px(24.);

/// Where the thumb sits on its track, in pixels from the track's top.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScrollThumb {
    pub top: Pixels,
    pub height: Pixels,
}

impl ScrollThumb {
    /// `None` when the page fits its view, so there is nothing to scroll.
    pub fn of(track: Pixels, scrolled: Pixels, scrollable: Pixels) -> Option<Self> {
        if scrollable <= Pixels::ZERO || track <= Pixels::ZERO {
            return None;
        }
        let in_view = track / (track + scrollable);
        let height = (track * in_view).max(MIN_THUMB_HEIGHT).min(track);
        let scrolled_share = (scrolled / scrollable).clamp(0., 1.);
        Some(Self {
            top: (track - height) * scrolled_share,
            height,
        })
    }

    /// How far the page is scrolled when the thumb's top is moved to `thumb_top`.
    pub fn scrolled_at(self, thumb_top: Pixels, track: Pixels, scrollable: Pixels) -> Pixels {
        let room = track - self.height;
        if room <= Pixels::ZERO {
            return Pixels::ZERO;
        }
        scrollable * (thumb_top / room).clamp(0., 1.)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_page_that_fits_its_view_has_no_thumb() {
        assert_eq!(ScrollThumb::of(px(400.), px(0.), px(0.)), None);
    }

    #[test]
    fn the_thumb_is_the_share_of_the_page_in_view_and_sits_where_the_view_is() {
        let track = px(400.);
        let scrollable = px(1200.);

        assert_eq!(
            ScrollThumb::of(track, px(0.), scrollable),
            Some(ScrollThumb {
                top: px(0.),
                height: px(100.)
            })
        );
        assert_eq!(
            ScrollThumb::of(track, px(1200.), scrollable).map(|thumb| thumb.top),
            Some(px(300.)),
            "at the end the thumb touches the bottom"
        );
    }

    #[test]
    fn a_long_page_keeps_a_thumb_big_enough_to_grab() {
        let thumb = ScrollThumb::of(px(400.), px(100_000.), px(100_000.)).unwrap();

        assert_eq!(thumb.height, MIN_THUMB_HEIGHT);
        assert_eq!(thumb.top, px(400.) - MIN_THUMB_HEIGHT);
    }

    #[test]
    fn dragging_the_thumb_scrolls_the_page_by_its_share_and_stops_at_the_ends() {
        let track = px(400.);
        let scrollable = px(1200.);
        let thumb = ScrollThumb::of(track, px(0.), scrollable).unwrap();

        assert_eq!(thumb.scrolled_at(px(150.), track, scrollable), px(600.));
        assert_eq!(thumb.scrolled_at(px(-50.), track, scrollable), px(0.));
        assert_eq!(thumb.scrolled_at(px(900.), track, scrollable), px(1200.));
    }
}
