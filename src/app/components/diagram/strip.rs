use std::ops::Range;
use std::path::Path;
use std::sync::Arc;

use gpui::{RenderImage, Rgba};
use resvg::tiny_skia::{Color, Pixmap, Transform};

use crate::image_facts::whole_pixels;

use super::drawer::Svg;

/// GPU textures larger than this fail on some Macs, so a tall diagram is drawn in strips.
const MAX_STRIP_PX: u32 = 4096;

/// A horizontal band of a diagram, in device pixels at one screen scale.
#[derive(Debug, Clone, PartialEq)]
pub struct Strip {
    index: usize,
    rows: Range<u32>,
    scale: f32,
}

impl Strip {
    /// `height` is in logical pixels.
    pub fn cut(height: f32, scale: f32) -> Vec<Self> {
        let height_px = whole_pixels((height * scale).ceil());
        (0..height_px)
            .step_by(MAX_STRIP_PX as usize)
            .enumerate()
            .map(|(index, top)| Self {
                index,
                rows: top..(top + MAX_STRIP_PX).min(height_px),
                scale,
            })
            .collect()
    }

    pub fn index(&self) -> usize {
        self.index
    }

    pub fn rows(&self) -> Range<u32> {
        self.rows.clone()
    }

    /// Logical pixels, the unit of the app layout.
    pub fn height(&self) -> f32 {
        self.height_px() as f32 / self.scale
    }

    fn height_px(&self) -> u32 {
        self.rows.end - self.rows.start
    }
}

/// Our own resvg drawing: GPUI's SVG path draws one colour only, and its image path draws at 1x.
pub fn rasterize(svg: &Svg, scale: f32, strip: &Strip) -> Result<Arc<RenderImage>, String> {
    let tree = svg.tree()?;
    let width_px = whole_pixels((svg.width() * scale).ceil());
    let height_px = strip.height_px();
    let mut pixmap = Pixmap::new(width_px, height_px).ok_or("the strip has no pixels")?;
    let transform =
        Transform::from_scale(scale, scale).post_translate(0.0, -(strip.rows.start as f32));
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    let mut pixels = pixmap.take();
    // resvg's pixels are premultiplied; a GPUI image is not, so see-through edges would come out dark.
    for pixel in pixels.as_chunks_mut::<4>().0 {
        gpui::swap_rgba_pa_to_bgra(pixel);
    }
    let bgra = image::RgbaImage::from_raw(width_px, height_px, pixels)
        .ok_or("the strip buffer has the wrong size")?;
    Ok(Arc::new(RenderImage::new(vec![image::Frame::new(bgra)])))
}

/// The whole diagram as one PNG on the page's ground, for the picture zoom: see-through, its lines
/// would be lost on the zoom's dark backdrop. Returns the scale it was drawn at, kept low enough
/// that its texture stays within `MAX_STRIP_PX`.
pub fn write_png(svg: &Svg, wanted_scale: f32, ground: Rgba, path: &Path) -> Result<f32, String> {
    let tree = svg.tree()?;
    let scale = wanted_scale.min(MAX_STRIP_PX as f32 / svg.width().max(svg.height()));
    let width_px = whole_pixels((svg.width() * scale).ceil());
    let height_px = whole_pixels((svg.height() * scale).ceil());
    let mut pixmap = Pixmap::new(width_px, height_px).ok_or("the diagram has no pixels")?;
    pixmap.fill(Color::from_rgba(ground.r, ground.g, ground.b, 1.).unwrap_or(Color::WHITE));
    resvg::render(
        &tree,
        Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    let pixels = pixmap
        .pixels()
        .iter()
        .flat_map(|pixel| {
            let color = pixel.demultiply();
            [color.red(), color.green(), color.blue(), color.alpha()]
        })
        .collect();
    image::RgbaImage::from_raw(width_px, height_px, pixels)
        .ok_or("the diagram buffer has the wrong size")?
        .save(path)
        .map_err(|error| error.to_string())?;
    Ok(scale)
}
