use std::path::Path;
use std::sync::Arc;

use gpui::{
    App, Bounds, Context, Entity, FocusHandle, Global, InteractiveElement, IntoElement,
    KeyDownEvent, MouseButton, ParentElement, Pixels, Render, Styled, WeakEntity, Window, div, img,
    point, px, size,
};

use super::motion::{OVERLAY, fade_in, fade_out, move_between};
use super::overlay::{close_presence, leaving_modal_layer, modal_layer};
use super::palette::{CARD_RADIUS, Colors};
use crate::viewer::{Presence, Shown};

/// A picture of a markdown body drawn as large as the window allows, over everything else.
pub struct PictureZoom {
    presence: Presence,
    /// What is zoomed; kept while the picture leaves, so it is drawn until it is gone.
    zoom: Option<Zoom>,
    focus: FocusHandle,
    owner_focus: FocusHandle,
}

struct Zoom {
    picture: ZoomedPicture,
    /// Where the picture lies on the page; `None` when it opened without a click.
    page_place: Option<Bounds<Pixels>>,
}

#[derive(Clone)]
pub struct ZoomedPicture {
    pub path: Arc<Path>,
    /// Width over height.
    pub ratio: f32,
    /// Its width in pixels; drawn wider than that, it only blurs. Endless for a drawing (SVG).
    pub pixel_width: f32,
}

/// Room left between the picture and the window's edge.
const MARGIN: f32 = 48.;

impl PictureZoom {
    pub const KEY_CONTEXT: &str = "PictureZoom";

    pub fn new(owner_focus: FocusHandle, cx: &mut Context<Self>) -> Self {
        Self {
            presence: Presence::default(),
            zoom: None,
            focus: cx.focus_handle(),
            owner_focus,
        }
    }

    pub fn open(
        &mut self,
        picture: ZoomedPicture,
        page_place: Option<Bounds<Pixels>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.zoom = Some(Zoom {
            picture,
            page_place,
        });
        self.presence.open();
        window.focus(&self.focus, cx);
        cx.notify();
    }

    pub fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.presence.is_open() {
            return;
        }
        close_presence(self, cx, |this| &mut this.presence);
        window.focus(&self.owner_focus, cx);
        cx.notify();
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.key == "escape" {
            cx.stop_propagation();
            self.close(window, cx);
        }
    }
}

/// The middle of the window, as large as it holds the picture.
fn large_place(picture: &ZoomedPicture, window: &Window) -> Bounds<Pixels> {
    let room = window.viewport_size();
    let width = (f32::from(room.width) - 2. * MARGIN)
        .min((f32::from(room.height) - 2. * MARGIN) * picture.ratio)
        .min(picture.pixel_width)
        .max(1.);
    let picture_size = size(px(width), px(width / picture.ratio));
    Bounds {
        origin: point(
            (room.width - picture_size.width) / 2.,
            (room.height - picture_size.height) / 2.,
        ),
        size: picture_size,
    }
}

fn in_place(shown: gpui::Img, place: Bounds<Pixels>) -> gpui::Img {
    shown
        .absolute()
        .left(place.origin.x)
        .top(place.origin.y)
        .w(place.size.width)
        .h(place.size.height)
}

impl Render for PictureZoom {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = Colors::active(cx);
        let (Some(zoom), Some(shown)) = (&self.zoom, self.presence.shown()) else {
            return div().into_any_element();
        };
        let is_leaving = shown == Shown::Leaving;
        let turn = self.presence.turn();
        let large = large_place(&zoom.picture, window);
        let shown = img(Arc::clone(&zoom.picture.path))
            .rounded(px(CARD_RADIUS))
            .shadow(colors.overlay_shadow());
        let shown = match (zoom.page_place, is_leaving) {
            (Some(page), false) => {
                move_between(shown, ("picture-zoom-grow", turn), page, large).into_any_element()
            }
            (Some(page), true) => {
                move_between(shown, ("picture-zoom-shrink", turn), large, page).into_any_element()
            }
            (None, false) => fade_in(in_place(shown, large), ("picture-zoom-rise", turn), OVERLAY)
                .into_any_element(),
            (None, true) => fade_out(in_place(shown, large), ("picture-zoom-fade", turn), OVERLAY)
                .into_any_element(),
        };
        if is_leaving {
            return leaving_modal_layer(&colors, ("picture-zoom-scrim-out", turn), OVERLAY)
                .child(shown)
                .into_any_element();
        }
        modal_layer(&colors, ("picture-zoom-scrim", turn))
            .id("picture-zoom")
            .key_context(Self::KEY_CONTEXT)
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key_down))
            .cursor_pointer()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| this.close(window, cx)),
            )
            .child(shown)
            .into_any_element()
    }
}

/// The zoom of the window on screen, reached from a markdown body that is not handed it.
struct ActivePictureZoom(WeakEntity<PictureZoom>);

impl Global for ActivePictureZoom {}

pub fn use_picture_zoom(zoom: &Entity<PictureZoom>, cx: &mut App) {
    cx.set_global(ActivePictureZoom(zoom.downgrade()));
}

pub fn zoom_picture(
    picture: ZoomedPicture,
    grows_from: Option<Bounds<Pixels>>,
    window: &mut Window,
    cx: &mut App,
) {
    let zoom = cx
        .try_global::<ActivePictureZoom>()
        .and_then(|active| active.0.upgrade());
    if let Some(zoom) = zoom {
        zoom.update(cx, |zoom, cx| zoom.open(picture, grows_from, window, cx));
    }
}
