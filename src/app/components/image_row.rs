use std::rc::Rc;
use std::sync::{Arc, LazyLock};

use gpui::{
    AnyElement, App, ElementId, Image, ImageFormat, InteractiveElement, IntoElement, ObjectFit,
    ParentElement, RenderOnce, SharedString, StatefulInteractiveElement, Styled, StyledImage,
    Window, canvas, div, fill, img, point, px, size,
};

use crate::file_path::FilePath;
use crate::image_facts::{self, PixelSize};
use crate::viewer::{ImageMark, ImagePreview, ImageSide};

use super::button::{ButtonKind, ButtonSize, icon_label_button};
use super::chip::chip;
use super::diff_view::OnImageComment;
use super::icon::Icon;
use super::palette::{CONTROL_RADIUS, Colors, Tone};

const PICTURE_BOX: (f32, f32) = (460., 360.);
const CHECKER_CELL: f32 = 8.;
const SVG_DRAW_SCALE: f32 = 2.;
const MAX_SVG_SIDE_PX: f32 = 4096.;

/// What can be drawn of each side, made off the main thread because an SVG has to be rasterized.
/// Kept by the layout, so the same decoded bitmap is drawn on every frame.
pub struct ImagePictures {
    base: Option<Picture>,
    head: Option<Picture>,
}

struct Picture {
    image: Arc<Image>,
    /// The size to draw at; an SVG is rasterized at twice this for a sharp picture.
    pixels: PixelSize,
}

impl ImagePictures {
    pub fn of(preview: &ImagePreview) -> Self {
        Self {
            base: preview
                .base
                .as_ref()
                .and_then(|side| Picture::of(preview.path.as_str(), side)),
            head: preview
                .head
                .as_ref()
                .and_then(|side| Picture::of(preview.path.as_str(), side)),
        }
    }
}

impl Picture {
    fn of(path: &str, side: &ImageSide) -> Option<Self> {
        let ImageSide::Loaded(loaded) = side else {
            return None;
        };
        let pixels = loaded.pixels?;
        let image = if image_facts::is_svg_path(path) {
            Image::from_bytes(ImageFormat::Png, rasterize_svg(&loaded.bytes, pixels)?)
        } else {
            Image::from_bytes(format_of(path)?, loaded.bytes.to_vec())
        };
        Some(Self {
            image: Arc::new(image),
            pixels,
        })
    }
}

fn format_of(path: &str) -> Option<ImageFormat> {
    match path.rsplit('.').next()?.to_ascii_lowercase().as_str() {
        "png" => Some(ImageFormat::Png),
        "jpg" | "jpeg" => Some(ImageFormat::Jpeg),
        "gif" => Some(ImageFormat::Gif),
        "webp" => Some(ImageFormat::Webp),
        _ => None,
    }
}

static SVG_OPTIONS: LazyLock<usvg::Options<'static>> = LazyLock::new(|| {
    let mut fonts = usvg::fontdb::Database::new();
    fonts.load_system_fonts();
    usvg::Options {
        fontdb: Arc::new(fonts),
        ..usvg::Options::default()
    }
});

/// A PNG of the SVG at twice its size, capped so a huge canvas stays small enough to hold.
fn rasterize_svg(svg: &[u8], size: PixelSize) -> Option<Vec<u8>> {
    let tree = usvg::Tree::from_data(svg, &SVG_OPTIONS).ok()?;
    let longest = size.width.max(size.height) as f32;
    let scale = SVG_DRAW_SCALE.min(MAX_SVG_SIDE_PX / longest);
    let (width, height) = (
        image_facts::whole_pixels((size.width as f32 * scale).ceil()),
        image_facts::whole_pixels((size.height as f32 * scale).ceil()),
    );
    let mut pixmap = resvg::tiny_skia::Pixmap::new(width, height)?;
    let fit = resvg::tiny_skia::Transform::from_scale(
        width as f32 / tree.size().width(),
        height as f32 / tree.size().height(),
    );
    resvg::render(&tree, fit, &mut pixmap.as_mut());
    let mut rgba = Vec::with_capacity(pixmap.pixels().len() * 4);
    for pixel in pixmap.pixels() {
        let straight = pixel.demultiply();
        rgba.extend([
            straight.red(),
            straight.green(),
            straight.blue(),
            straight.alpha(),
        ]);
    }
    let mut png = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_raw(width, height, rgba)?
        .write_to(&mut png, image::ImageFormat::Png)
        .ok()?;
    Some(png.into_inner())
}

/// The pictures of one changed image: base left, head right, a facts row above, and a button
/// for a comment on the whole file. An added or deleted image is one picture with a mark.
#[derive(IntoElement)]
pub struct ImageRow {
    path: FilePath,
    preview: Option<ImagePreview>,
    pictures: Option<Rc<ImagePictures>>,
    on_comment: OnImageComment,
}

impl ImageRow {
    pub fn new(
        path: FilePath,
        preview: Option<ImagePreview>,
        pictures: Option<Rc<ImagePictures>>,
        on_comment: OnImageComment,
    ) -> Self {
        Self {
            path,
            preview,
            pictures,
            on_comment,
        }
    }
}

impl RenderOnce for ImageRow {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let colors = Colors::active(cx);
        let Some(preview) = self.preview else {
            return div()
                .px_2()
                .py_1()
                .text_sm()
                .text_color(colors.muted_text)
                .child("Loading the picture...")
                .into_any_element();
        };
        let pictures = self.pictures;
        let (base, head) = (
            pictures
                .as_ref()
                .and_then(|pictures| pictures.base.as_ref()),
            pictures
                .as_ref()
                .and_then(|pictures| pictures.head.as_ref()),
        );
        let path = SharedString::from(Arc::<str>::from(self.path));
        let on_comment = self.on_comment;
        div()
            .flex()
            .flex_col()
            .gap_2()
            .p_2()
            .child(facts_row(&preview, &colors))
            .child(
                div()
                    .flex()
                    .gap_4()
                    .children(preview.base.as_ref().map(|side| {
                        side_column("before", preview.mark(), side, base, &path, &colors)
                    }))
                    .children(preview.head.as_ref().map(|side| {
                        side_column("after", preview.mark(), side, head, &path, &colors)
                    })),
            )
            .child(
                div().flex().child(
                    icon_label_button(
                        &colors,
                        ElementId::from(SharedString::from(format!("comment-on-image-{path}"))),
                        ButtonKind::Secondary,
                        ButtonSize::Small,
                        Icon::MessageSquare,
                        "Comment on this image",
                    )
                    .on_click(move |_, window, cx| on_comment(window, cx)),
                ),
            )
            .into_any_element()
    }
}

fn facts_row(preview: &ImagePreview, colors: &Colors) -> impl IntoElement {
    let side = |label: &str, side: &Option<ImageSide>| {
        side.as_ref()
            .map(|side| div().child(format!("{label}: {}", side.facts())))
    };
    div()
        .flex()
        .flex_wrap()
        .gap_4()
        .text_sm()
        .text_color(colors.muted_text)
        .children(side("before", &preview.base))
        .children(side("after", &preview.head))
        .children(preview.size_change().map(|change| div().child(change)))
}

fn side_column(
    label: &str,
    mark: Option<ImageMark>,
    side: &ImageSide,
    picture: Option<&Picture>,
    path: &SharedString,
    colors: &Colors,
) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(
            div()
                .flex()
                .gap_2()
                .text_xs()
                .text_color(colors.muted_text)
                .child(label.to_owned())
                .children(mark.map(|mark| {
                    let tone = match mark {
                        ImageMark::Added => Tone::Success,
                        ImageMark::Deleted => Tone::Danger,
                    };
                    chip(colors, tone).child(mark.label())
                })),
        )
        .child(match picture {
            Some(picture) => drawn(picture, path, label, colors).into_any_element(),
            None => div()
                .text_sm()
                .text_color(colors.muted_text)
                .child(match side {
                    ImageSide::TooLarge { .. } => "No preview: the file is too large.",
                    _ => "No preview.",
                })
                .into_any_element(),
        })
        .into_any_element()
}

fn drawn(picture: &Picture, path: &SharedString, label: &str, colors: &Colors) -> impl IntoElement {
    let (width, height) = fitted(picture.pixels);
    div()
        .id(ElementId::from(SharedString::from(format!(
            "{path}-{label}"
        ))))
        .relative()
        .w(px(width))
        .h(px(height))
        .rounded(px(CONTROL_RADIUS))
        .overflow_hidden()
        .border_1()
        .border_color(colors.subtle_border)
        .child(checkerboard(width, height, colors))
        .child(
            img(Arc::clone(&picture.image))
                .absolute()
                .top_0()
                .left_0()
                .w(px(width))
                .h(px(height))
                .object_fit(ObjectFit::Contain),
        )
}

/// The logical size to draw at: the picture's own size, shrunk to fit the box and never enlarged.
fn fitted(pixels: PixelSize) -> (f32, f32) {
    let (width, height) = (pixels.width.max(1) as f32, pixels.height.max(1) as f32);
    let scale = (PICTURE_BOX.0 / width).min(PICTURE_BOX.1 / height).min(1.);
    ((width * scale).max(1.), (height * scale).max(1.))
}

/// What transparent parts of a picture show through to.
fn checkerboard(width: f32, height: f32, colors: &Colors) -> impl IntoElement {
    let (light, dark) = (colors.checker_light, colors.checker_dark);
    canvas(
        |_, _, _| (),
        move |bounds, (), window, _| {
            let columns = image_facts::whole_pixels((width / CHECKER_CELL).ceil());
            let rows = image_facts::whole_pixels((height / CHECKER_CELL).ceil());
            for row in 0..rows {
                for column in 0..columns {
                    let origin = bounds.origin
                        + point(
                            px(column as f32 * CHECKER_CELL),
                            px(row as f32 * CHECKER_CELL),
                        );
                    let cell = gpui::Bounds::new(origin, size(px(CHECKER_CELL), px(CHECKER_CELL)))
                        .intersect(&bounds);
                    let color = if (row + column) % 2 == 0 { light } else { dark };
                    window.paint_quad(fill(cell, color));
                }
            }
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .w(px(width))
    .h(px(height))
}
