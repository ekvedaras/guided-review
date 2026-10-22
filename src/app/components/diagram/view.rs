use std::sync::Arc;

use gpui::{
    App, Bounds, Context, Corners, Entity, InteractiveElement, IntoElement, ParentElement, Pixels,
    RenderImage, RenderOnce, StatefulInteractiveElement, Styled, Window, canvas, div, px,
};

use crate::app::components::palette::{Appearance, Palette};

use super::drawer::DiagramDrawer;
use super::memory::{DiagramMemory, Showing};
use super::strip::{Strip, rasterize};

/// Owns every diagram of the tour and draws them off the main thread.
/// The layout tells it each frame which diagram is shown; `DiagramView` puts that one on screen.
#[derive(Default)]
pub struct Diagrams {
    memory: DiagramMemory<Arc<RenderImage>>,
}

impl Diagrams {
    /// `source` is the Mermaid text of the shown step, None when the step shows no diagram.
    pub fn show(&mut self, source: Option<&str>, window: &mut Window, cx: &mut Context<Self>) {
        let showing = source.map(|source| Showing {
            source,
            appearance: Appearance::of_window(window.appearance()),
            scale: window.scale_factor(),
        });
        for bitmap in self.memory.show(showing) {
            window.drop_image(bitmap).ok();
        }
        self.draw_svgs(cx);
    }

    fn draw_svgs(&mut self, cx: &mut Context<Self>) {
        for job in self.memory.svgs_to_draw() {
            cx.spawn(async move |this, cx| {
                let (job, svg) = cx
                    .background_executor()
                    .spawn(async move {
                        let svg = DiagramDrawer::for_appearance(job.appearance).svg(&job.source);
                        (job, svg)
                    })
                    .await;
                this.update(cx, |this, cx| {
                    this.memory.put_svg(job, svg);
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
    }

    fn set_on_screen(
        &mut self,
        strip: usize,
        on_screen: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(bitmap) = self.memory.set_on_screen(strip, on_screen) {
            window.drop_image(bitmap).ok();
        }
        self.draw_strips(cx);
    }

    fn draw_strips(&mut self, cx: &mut Context<Self>) {
        for job in self.memory.strips_to_draw() {
            cx.spawn(async move |this, cx| {
                let (job, bitmap) = cx
                    .background_executor()
                    .spawn(async move {
                        let bitmap = rasterize(&job.svg, job.scale, &job.strip);
                        (job, bitmap)
                    })
                    .await;
                this.update(cx, |this, cx| match bitmap {
                    Ok(bitmap) => {
                        // A bitmap turned away was never painted, so it has no GPU copy to free.
                        if this.memory.put_bitmap(job, bitmap).is_none() {
                            cx.notify();
                        }
                    }
                    Err(error) => eprintln!("could not draw a diagram strip: {error}"),
                })
                .ok();
            })
            .detach();
        }
    }
}

/// The shown step's diagram: one block per strip, sized from the SVG before any bitmap exists,
/// so the page does not jump when bitmaps arrive or leave.
#[derive(IntoElement)]
pub struct DiagramView {
    diagrams: Entity<Diagrams>,
}

impl DiagramView {
    pub fn new(diagrams: Entity<Diagrams>) -> Self {
        Self { diagrams }
    }
}

impl RenderOnce for DiagramView {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let memory = &self.diagrams.read(cx).memory;
        let frame = div()
            .id("diagram")
            .overflow_x_scroll()
            .rounded_md()
            .border_1()
            .border_color(Palette::border());
        if let Some(svg) = memory.svg() {
            let width = svg.width();
            let strips = memory
                .strips()
                .into_iter()
                .map(|strip| strip_block(strip, width, self.diagrams.clone()));
            return frame.child(div().flex().flex_col().children(strips));
        }
        let note = match memory.error() {
            Some(error) => format!("Could not draw the diagram: {error}"),
            None => "Drawing the diagram…".to_string(),
        };
        frame
            .p_3()
            .text_sm()
            .text_color(Palette::muted_text())
            .child(note)
    }
}

/// Reports in prepaint whether the strip is near the visible part of the page, so
/// the bitmap is made just before it scrolls in and freed once it scrolls out.
fn strip_block(strip: Strip, width: f32, diagrams: Entity<Diagrams>) -> impl IntoElement {
    let index = strip.index();
    let reporter = diagrams.clone();
    canvas(
        move |bounds, window, cx| {
            let on_screen = near_visible_part(bounds, window);
            reporter.update(cx, |diagrams, cx| {
                diagrams.set_on_screen(index, on_screen, window, cx)
            });
        },
        move |bounds, (), window, cx| {
            if let Some(bitmap) = diagrams.read(cx).memory.bitmap(index).cloned() {
                window
                    .paint_image(bounds, bounds, Corners::default(), bitmap, 0, false)
                    .ok();
            }
        },
    )
    .w(px(width))
    .h(px(strip.height()))
}

fn near_visible_part(bounds: Bounds<Pixels>, window: &Window) -> bool {
    let visible = window.content_mask().bounds;
    visible.dilate(visible.size.height / 2.).intersects(&bounds)
}
