#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "a panic in a test helper is the test failing"
)]
#![allow(
    clippy::needless_pass_by_value,
    reason = "test helpers take owned arguments so a case reads as a plain call"
)]

use std::fmt::Write;

use guided_review::app::components::diagram::{DiagramDrawer, Strip, Svg, rasterize};
use guided_review::app::components::palette::{Appearance, Colors, TEXT_FONT};

const FLOWCHART: &str = "flowchart TD\n  A[Open the tour] --> B{Has diagram?}\n  B -->|yes| C[Draw it]\n  B -->|no| D[Skip]\n";

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a colour channel is 0.0..=1.0, so the byte is 0..=255"
)]
fn hex(color: gpui::Rgba) -> String {
    let [r, g, b] = [color.r, color.g, color.b].map(|channel| (channel * 255.0).round() as u8);
    format!("#{r:02x}{g:02x}{b:02x}")
}

fn svg(appearance: Appearance, source: &str) -> Svg {
    DiagramDrawer::for_appearance(appearance)
        .svg(source)
        .expect("diagram draws")
}

#[test]
fn a_diagram_is_drawn_on_the_app_background_in_the_app_font() {
    for appearance in [Appearance::Light, Appearance::Dark] {
        let colors = Colors::of(appearance);
        let svg = svg(appearance, FLOWCHART);

        assert!(
            svg.text().contains(&hex(colors.background)),
            "{appearance:?} diagram is not on the app background"
        );
        assert!(
            svg.text().contains(TEXT_FONT),
            "{appearance:?} diagram is not in the app font"
        );
        assert!(svg.width() > 0.0 && svg.height() > 0.0);
    }
}

#[test]
fn nodes_take_the_colours_of_the_appearance() {
    let light = svg(Appearance::Light, FLOWCHART);
    let dark = svg(Appearance::Dark, FLOWCHART);

    assert!(
        light
            .text()
            .contains(&hex(Colors::of(Appearance::Light).text))
    );
    assert!(
        dark.text()
            .contains(&hex(Colors::of(Appearance::Dark).text))
    );
    assert!(
        !dark
            .text()
            .contains(&hex(Colors::of(Appearance::Light).background))
    );
}

#[test]
fn text_that_is_not_mermaid_is_an_error() {
    assert!(
        DiagramDrawer::for_appearance(Appearance::Dark)
            .svg("this is not a diagram")
            .is_err()
    );
}

fn tall_flowchart(nodes: usize) -> String {
    let mut source = String::from("flowchart TD\n");
    for node in 1..nodes {
        writeln!(source, "  N{} --> N{node}", node - 1).ok();
    }
    source
}

#[test]
fn a_diagram_is_cut_into_strips_of_at_most_4096_device_pixels() {
    let strips = Strip::cut(5000.0, 2.0);

    let rows: Vec<_> = strips.iter().map(Strip::rows).collect();
    assert_eq!(rows, [0..4096, 4096..8192, 8192..10000]);
    assert_eq!(strips[2].height(), 904.0);
}

#[test]
fn a_short_diagram_is_one_strip() {
    let strips = Strip::cut(300.0, 2.0);

    assert_eq!(strips.len(), 1);
    assert_eq!(strips[0].rows(), 0..600);
    assert_eq!(strips[0].height(), 300.0);
}

#[test]
fn a_strip_is_drawn_at_screen_scale_with_blue_and_red_swapped_for_gpui() {
    let svg = svg(Appearance::Dark, &tall_flowchart(80));
    let strips = Strip::cut(svg.height(), 2.0);
    assert!(
        strips.len() > 1,
        "the test diagram must be taller than one strip"
    );

    let last = strips.last().unwrap();
    let bitmap = rasterize(&svg, 2.0, last).unwrap();

    let size = bitmap.size(0);
    assert_eq!(size.width.0 as f32, (svg.width() * 2.0).ceil());
    assert_eq!(size.height.0, i32::try_from(last.rows().len()).unwrap());
    let corner = &bitmap.as_bytes(0).unwrap()[..4];
    let dark_background_as_bgra = [0x17, 0x11, 0x0d, 0xff];
    assert_eq!(corner, dark_background_as_bgra);
}

mod memory {
    use super::*;
    use guided_review::app::components::diagram::{DiagramMemory, Showing};

    /// Bitmaps are opaque to the memory; a name is enough to tell them apart.
    type Bitmap = &'static str;

    const OTHER: &str = "flowchart LR\n  X --> Y\n";

    #[allow(
        clippy::unnecessary_wraps,
        reason = "DiagramMemory::show takes an Option, where None is the step without a diagram"
    )]
    fn showing(source: &str, appearance: Appearance, scale: f32) -> Option<Showing<'_>> {
        Some(Showing {
            source,
            appearance,
            scale,
        })
    }

    /// Draws every SVG the memory asks for, as the app does in the background.
    fn draw_svgs(memory: &mut DiagramMemory<Bitmap>) {
        for job in memory.svgs_to_draw() {
            let svg = DiagramDrawer::for_appearance(job.appearance).svg(&job.source);
            memory.put_svg(&job, svg);
        }
    }

    fn put_bitmaps(memory: &mut DiagramMemory<Bitmap>, bitmap: Bitmap) -> usize {
        let jobs = memory.strips_to_draw();
        let count = jobs.len();
        for job in jobs {
            assert!(memory.put_bitmap(&job, bitmap).is_none());
        }
        count
    }

    fn on_screen(source: &str, appearance: Appearance) -> DiagramMemory<Bitmap> {
        let mut memory = DiagramMemory::default();
        memory.show(showing(source, appearance, 2.0));
        draw_svgs(&mut memory);
        memory.set_on_screen(0, true);
        put_bitmaps(&mut memory, "first");
        memory
    }

    #[test]
    fn showing_a_diagram_draws_its_svg_for_both_appearances_current_first() {
        let mut memory = DiagramMemory::<Bitmap>::default();
        memory.show(showing(FLOWCHART, Appearance::Dark, 2.0));

        let jobs = memory.svgs_to_draw();

        let appearances: Vec<_> = jobs.iter().map(|job| job.appearance).collect();
        assert_eq!(appearances, [Appearance::Dark, Appearance::Light]);
        assert!(memory.svgs_to_draw().is_empty(), "drawing twice");
    }

    #[test]
    fn svgs_are_kept_across_theme_switches_and_steps() {
        let mut memory = DiagramMemory::<Bitmap>::default();
        memory.show(showing(FLOWCHART, Appearance::Dark, 2.0));
        draw_svgs(&mut memory);
        memory.show(showing(OTHER, Appearance::Dark, 2.0));
        draw_svgs(&mut memory);

        memory.show(showing(FLOWCHART, Appearance::Light, 2.0));

        assert!(memory.svgs_to_draw().is_empty());
        assert_ne!(memory.strips(), [] as [Strip; 0]);
    }

    #[test]
    fn a_strip_on_screen_is_drawn_once_in_the_current_appearance_at_screen_scale() {
        let mut memory = DiagramMemory::<Bitmap>::default();
        memory.show(showing(FLOWCHART, Appearance::Light, 2.0));
        draw_svgs(&mut memory);

        memory.set_on_screen(0, true);
        let jobs = memory.strips_to_draw();

        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].scale, 2.0);
        assert!(
            jobs[0]
                .svg
                .text()
                .contains(&hex(Colors::of(Appearance::Light).background))
        );
        assert!(memory.strips_to_draw().is_empty(), "drawing twice");
        memory.put_bitmap(&jobs[0].clone(), "light");
        memory.set_on_screen(0, true);
        assert!(memory.strips_to_draw().is_empty(), "drawing what is drawn");
        assert_eq!(memory.bitmap(0), Some(&"light"));
    }

    #[test]
    fn a_strip_that_leaves_the_screen_gives_up_its_bitmap() {
        let mut memory = on_screen(FLOWCHART, Appearance::Dark);

        let dropped = memory.set_on_screen(0, false);

        assert_eq!(dropped, Some("first"));
        assert_eq!(memory.bitmap(0), None);
    }

    #[test]
    fn no_strip_is_drawn_while_off_screen() {
        let mut memory = DiagramMemory::<Bitmap>::default();
        memory.show(showing(FLOWCHART, Appearance::Dark, 2.0));
        draw_svgs(&mut memory);

        memory.set_on_screen(0, false);

        assert!(memory.strips_to_draw().is_empty());
    }

    #[test]
    fn a_strip_counts_as_new_on_screen_again_after_a_theme_or_scale_change() {
        let mut memory = on_screen(FLOWCHART, Appearance::Dark);
        assert!(memory.is_on_screen(0));

        memory.show(showing(FLOWCHART, Appearance::Light, 2.0));
        assert!(
            !memory.is_on_screen(0),
            "the new theme's strip must be drawn"
        );

        memory.set_on_screen(0, true);
        memory.show(showing(FLOWCHART, Appearance::Light, 1.0));
        assert!(
            !memory.is_on_screen(0),
            "the new scale's strip must be drawn"
        );
    }

    #[test]
    fn a_theme_switch_drops_the_bitmaps_of_the_old_theme() {
        let mut memory = on_screen(FLOWCHART, Appearance::Dark);

        let dropped = memory.show(showing(FLOWCHART, Appearance::Light, 2.0));
        memory.set_on_screen(0, true);
        let jobs = memory.strips_to_draw();

        assert_eq!(dropped, ["first"]);
        assert_eq!(memory.bitmap(0), None);
        assert!(
            jobs[0]
                .svg
                .text()
                .contains(&hex(Colors::of(Appearance::Light).background))
        );
    }

    #[test]
    fn another_step_drops_the_bitmaps_of_the_last_one() {
        let mut memory = on_screen(FLOWCHART, Appearance::Dark);

        assert_eq!(
            memory.show(showing(OTHER, Appearance::Dark, 2.0)),
            ["first"]
        );
        assert_eq!(memory.bitmap(0), None);
    }

    #[test]
    fn a_step_with_no_diagram_drops_every_bitmap() {
        let mut memory = on_screen(FLOWCHART, Appearance::Dark);

        assert_eq!(memory.show(None), ["first"]);
    }

    #[test]
    fn a_new_screen_scale_drops_the_bitmaps_and_draws_again() {
        let mut memory = on_screen(FLOWCHART, Appearance::Dark);

        let dropped = memory.show(showing(FLOWCHART, Appearance::Dark, 1.0));
        memory.set_on_screen(0, true);

        assert_eq!(dropped, ["first"]);
        assert_eq!(memory.strips_to_draw()[0].scale, 1.0);
    }

    #[test]
    fn showing_the_same_thing_again_keeps_the_bitmaps() {
        let mut memory = on_screen(FLOWCHART, Appearance::Dark);

        assert_eq!(
            memory.show(showing(FLOWCHART, Appearance::Dark, 2.0)),
            [] as [&str; 0]
        );
        assert_eq!(memory.bitmap(0), Some(&"first"));
    }

    #[test]
    fn a_bitmap_drawn_for_an_old_theme_is_turned_away() {
        let mut memory = DiagramMemory::<Bitmap>::default();
        memory.show(showing(FLOWCHART, Appearance::Dark, 2.0));
        draw_svgs(&mut memory);
        memory.set_on_screen(0, true);
        let job = memory.strips_to_draw().remove(0);

        memory.show(showing(FLOWCHART, Appearance::Light, 2.0));

        assert_eq!(memory.put_bitmap(&job, "late dark"), Some("late dark"));
        assert_eq!(memory.bitmap(0), None);
    }

    #[test]
    fn a_strip_that_could_not_be_drawn_says_why_and_is_not_drawn_again_until_the_diagram_changes() {
        let mut memory = DiagramMemory::<Bitmap>::default();
        memory.show(showing(FLOWCHART, Appearance::Dark, 2.0));
        draw_svgs(&mut memory);
        memory.set_on_screen(0, true);
        let job = memory.strips_to_draw().remove(0);

        memory.strip_failed(&job, "the strip has no pixels".into());
        memory.set_on_screen(0, true);

        assert_eq!(
            memory.strip_failure(0).map(AsRef::as_ref),
            Some("the strip has no pixels")
        );
        assert!(
            memory.strips_to_draw().is_empty(),
            "drawing what cannot be drawn"
        );
        memory.show(showing(FLOWCHART, Appearance::Light, 2.0));
        assert!(memory.strip_failure(0).is_none());
    }

    #[test]
    fn a_bitmap_for_a_strip_that_left_the_screen_is_turned_away() {
        let mut memory = DiagramMemory::<Bitmap>::default();
        memory.show(showing(FLOWCHART, Appearance::Dark, 2.0));
        draw_svgs(&mut memory);
        memory.set_on_screen(0, true);
        let job = memory.strips_to_draw().remove(0);

        memory.set_on_screen(0, false);

        assert_eq!(memory.put_bitmap(&job, "late"), Some("late"));
    }

    #[test]
    fn only_strips_on_screen_hold_bitmaps() {
        let mut memory = DiagramMemory::<Bitmap>::default();
        memory.show(showing(&tall_flowchart(80), Appearance::Dark, 2.0));
        draw_svgs(&mut memory);
        let strips = memory.strips().len();
        assert!(strips > 1);

        memory.set_on_screen(strips - 1, true);
        put_bitmaps(&mut memory, "bottom");

        let held: Vec<_> = (0..strips)
            .filter(|&i| memory.bitmap(i).is_some())
            .collect();
        assert_eq!(held, [strips - 1]);
    }

    #[test]
    fn a_broken_diagram_has_no_strips_and_says_why() {
        let mut memory = DiagramMemory::<Bitmap>::default();
        memory.show(showing("not a diagram", Appearance::Dark, 2.0));
        draw_svgs(&mut memory);

        assert_eq!(memory.strips(), [] as [Strip; 0]);
        assert!(memory.error().is_some());
    }
}
