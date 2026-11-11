use std::path::Path;
use std::sync::Arc;

use gpui::{
    App, Bounds, Context, Entity, FocusHandle, Global, InteractiveElement, IntoElement,
    KeyDownEvent, MouseButton, ParentElement, Pixels, Render, Styled, WeakEntity, Window, div, img,
    point, px, size,
};

use super::motion::{grow_from, rise_in};
use super::overlay::modal_layer;
use super::palette::{CARD_RADIUS, Colors};

/// A picture of a markdown body drawn as large as the window allows, over everything else.
pub struct PictureZoom {
    picture: Option<ZoomedPicture>,
    /// Where the picture lay on the page when clicked; `None` when it opened without a click.
    grows_from: Option<Bounds<Pixels>>,
    /// Counts the openings, so each one plays its growth again.
    openings: usize,
    focus: FocusHandle,
    owner_focus: FocusHandle,
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
            picture: None,
            grows_from: None,
            openings: 0,
            focus: cx.focus_handle(),
            owner_focus,
        }
    }

    pub fn open(
        &mut self,
        picture: ZoomedPicture,
        grows_from: Option<Bounds<Pixels>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.picture = Some(picture);
        self.grows_from = grows_from;
        self.openings += 1;
        window.focus(&self.focus, cx);
        cx.notify();
    }

    pub fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.picture = None;
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

impl Render for PictureZoom {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(picture) = &self.picture else {
            return div().into_any_element();
        };
        let colors = Colors::active(cx);
        let room = window.viewport_size();
        let width = (f32::from(room.width) - 2. * MARGIN)
            .min((f32::from(room.height) - 2. * MARGIN) * picture.ratio)
            .min(picture.pixel_width)
            .max(1.);
        let shown = img(Arc::clone(&picture.path))
            .rounded(px(CARD_RADIUS))
            .shadow(colors.overlay_shadow());
        let target = Bounds {
            origin: point(
                (room.width - px(width)) / 2.,
                (room.height - px(width / picture.ratio)) / 2.,
            ),
            size: size(px(width), px(width / picture.ratio)),
        };
        let id = ("picture-zoom-grow", self.openings);
        let shown = match self.grows_from {
            Some(from) => grow_from(shown, id, from, target).into_any_element(),
            None => {
                rise_in(shown.w(target.size.width).h(target.size.height), id).into_any_element()
            }
        };
        modal_layer(&colors, ("picture-zoom-scrim", self.openings))
            .items_center()
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
