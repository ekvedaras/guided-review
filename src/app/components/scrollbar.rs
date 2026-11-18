//! The one scrollbar look: a thumb on the right edge of what scrolls, that shows where the view is.
//! The page, the PR switcher and the issue card all draw it here, so they look the same.

use gpui::prelude::FluentBuilder;
use std::rc::Rc;

use gpui::{
    Bounds, Context, DispatchPhase, Div, ElementId, InteractiveElement, IntoElement, ListState,
    MouseButton, MouseMoveEvent, MouseUpEvent, ParentElement, Pixels, Point, ScrollHandle,
    Stateful, Styled, WeakEntity, canvas, div, point, px,
};

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

/// While a scrollbar is dragged: every pointer move and the let-go, wherever in the window the pointer
/// is. An element hears a move only while the pointer is over it, and a card floating above its owner
/// (the issue card over the page) hides the owner from the pointer. The owner adds it while it drags.
pub fn drag_listener<V: 'static>(
    owner: WeakEntity<V>,
    on_move: impl Fn(&mut V, Point<Pixels>, &mut Context<V>) + 'static,
    on_release: impl Fn(&mut V, Point<Pixels>, &mut Context<V>) + 'static,
) -> impl IntoElement {
    let (on_move, on_release) = (Rc::new(on_move), Rc::new(on_release));
    canvas(
        |_, _, _| {},
        move |_, (), window, _| {
            let (moved, released) = (owner.clone(), owner);
            let release_on_move = Rc::clone(&on_release);
            window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                if phase != DispatchPhase::Bubble {
                    return;
                }
                let is_held = event.pressed_button == Some(MouseButton::Left);
                moved
                    .update(cx, |owner, cx| {
                        if is_held {
                            on_move(owner, event.position, cx);
                        } else {
                            release_on_move(owner, event.position, cx);
                        }
                    })
                    .ok();
            });
            window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
                if phase == DispatchPhase::Bubble && event.button == MouseButton::Left {
                    released
                        .update(cx, |owner, cx| on_release(owner, event.position, cx))
                        .ok();
                }
            });
        },
    )
    .absolute()
    .size_full()
}

/// What a scrollbar scrolls: gpui's list (the page, the PR switcher) or a scrolling box (the issue card).
pub trait ScrollSource {
    /// The part on screen, in window pixels; the track runs beside it, top to bottom.
    fn viewport(&self) -> Bounds<Pixels>;
    /// How far it is scrolled down.
    fn scrolled(&self) -> Pixels;
    /// How far it can scroll down.
    fn scrollable(&self) -> Pixels;
    fn scroll_to(&self, scrolled: Pixels);
    fn drag_started(&self) {}
    fn drag_ended(&self) {}
}

impl ScrollSource for ListState {
    fn viewport(&self) -> Bounds<Pixels> {
        self.viewport_bounds()
    }

    fn scrolled(&self) -> Pixels {
        -self.scroll_px_offset_for_scrollbar().y
    }

    fn scrollable(&self) -> Pixels {
        self.max_offset_for_scrollbar().y
    }

    fn scroll_to(&self, scrolled: Pixels) {
        self.set_offset_from_scrollbar(point(Pixels::ZERO, -scrolled));
    }

    fn drag_started(&self) {
        self.scrollbar_drag_started();
    }

    fn drag_ended(&self) {
        self.scrollbar_drag_ended();
    }
}

impl ScrollSource for ScrollHandle {
    fn viewport(&self) -> Bounds<Pixels> {
        self.bounds()
    }

    fn scrolled(&self) -> Pixels {
        -self.offset().y
    }

    fn scrollable(&self) -> Pixels {
        self.max_offset().y
    }

    fn scroll_to(&self, scrolled: Pixels) {
        self.set_offset(point(self.offset().x, -scrolled));
    }
}

/// A drag of a scrollbar's thumb: where on the thumb the pointer holds it. A press on the track off
/// the thumb takes the thumb's middle there. The owner calls `drag_to` on every pointer move until
/// `release`, wherever the pointer is.
#[derive(Debug, Default)]
pub struct ScrollbarDrag {
    grab: Option<Pixels>,
}

impl ScrollbarDrag {
    pub fn thumb(source: &impl ScrollSource) -> Option<ScrollThumb> {
        ScrollThumb::of(
            source.viewport().size.height,
            source.scrolled(),
            source.scrollable(),
        )
    }

    pub const fn is_dragging(&self) -> bool {
        self.grab.is_some()
    }

    /// `pointer_y` is in window pixels.
    pub fn press(&mut self, source: &impl ScrollSource, pointer_y: Pixels) {
        let Some(thumb) = Self::thumb(source) else {
            return;
        };
        let on_track = pointer_y - source.viewport().top();
        let is_on_thumb = on_track >= thumb.top && on_track <= thumb.top + thumb.height;
        self.grab = Some(if is_on_thumb {
            on_track - thumb.top
        } else {
            thumb.height / 2.
        });
        source.drag_started();
        self.drag_to(source, pointer_y);
    }

    pub fn drag_to(&self, source: &impl ScrollSource, pointer_y: Pixels) {
        let (Some(grab), Some(thumb)) = (self.grab, Self::thumb(source)) else {
            return;
        };
        let viewport = source.viewport();
        source.scroll_to(thumb.scrolled_at(
            pointer_y - viewport.top() - grab,
            viewport.size.height,
            source.scrollable(),
        ));
    }

    /// Whether a drag was under way.
    pub fn release(&mut self, source: &impl ScrollSource) -> bool {
        let was_dragging = self.grab.take().is_some();
        if was_dragging {
            source.drag_ended();
        }
        was_dragging
    }
}

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
