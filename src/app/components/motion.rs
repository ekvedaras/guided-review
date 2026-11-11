//! The app's motion vocabulary: how long a move takes, how it eases, and the few moves there are.
//! Components take their numbers from here and do not invent their own.
//!
//! Every move runs through gpui's animation elements, which honour `App::reduce_motion`: one-shot
//! moves then draw their end state and repeating ones their start state. An animation plays once per
//! element id, so a move that must replay on a state change takes an id that changes with that state.

use std::ops::Range;
use std::time::Duration;

use gpui::{
    Animation, AnimationElement, AnimationExt, ElementId, HighlightStyle, IntoElement,
    ParentElement, Pixels, Rgba, SharedString, SpringAnimation, SpringAnimationElement,
    SpringConfig, Styled, StyledText, Svg, Transformation, div, percentage, px, relative, size,
};

use super::palette::Colors;

/// Press, hover-adjacent swaps, a chevron turning.
pub const FEEDBACK: Duration = Duration::from_millis(120);
/// A part of the page changing state: a banner, a box opening, a mark appearing.
pub const STATE: Duration = Duration::from_millis(180);
/// Something floating in over the page: a palette, a dialog, a panel.
pub const OVERLAY: Duration = Duration::from_millis(240);
/// The page's content changing to another screen.
pub const SCREEN: Duration = Duration::from_millis(260);
/// One pass of the busy sweep.
const SWEEP: Duration = Duration::from_millis(1400);
/// One breath of the busy status dot.
const PULSE: Duration = Duration::from_millis(1600);
/// Repeating moves redraw no more often than this, so a busy screen does not hold the GPU at 120 Hz.
const BUSY_FPS: f32 = 60.;

/// How far a screen's content travels in from the side the reader is heading to.
pub const SCREEN_SLIDE: Pixels = px(12.);
/// How far an overlay rises into place.
pub const OVERLAY_RISE: Pixels = px(8.);
/// How far an opening box (composer, reply) moves into place.
pub const BOX_RISE: Pixels = px(6.);
/// How far a band under the title bar (new commits, refresh, a notice) drops into place.
pub const BANNER_DROP: Pixels = px(8.);
/// How far a docked panel (Ask) slides in from its edge.
pub const PANEL_SLIDE: Pixels = px(24.);

/// Fast at first, settling softly: the one easing for things arriving.
pub fn ease_out(delta: f32) -> f32 {
    1. - (1. - delta).powi(5)
}

/// The trail's spring: critically damped, so a share glides to its new length without overshoot,
/// mostly there within `OVERLAY` and still within half a second.
const TRAIL_SPRING: SpringConfig = SpringConfig::new(400., 40., 1.);
/// A thumb's spring: critically damped and a little quicker than the trail, about `STATE` to land.
const GLIDE_SPRING: SpringConfig = SpringConfig::new(900., 60., 1.);
/// A chevron's spring: critically damped and quick, about `FEEDBACK` to turn.
const TURN_SPRING: SpringConfig = SpringConfig::new(1600., 80., 1.);
/// The read mark's spring: a touch under critical, so a new check lands with a small, single pop.
const POP_SPRING: SpringConfig = SpringConfig::new(520., 32., 1.);

/// Which side something comes in from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    Left,
    Right,
    Top,
    Bottom,
}

fn one_shot(duration: Duration) -> Animation {
    Animation::new(duration).with_easing(ease_out)
}

/// Fades in over `duration`.
pub fn fade_in<E: IntoElement + Styled + 'static>(
    element: E,
    id: impl Into<ElementId>,
    duration: Duration,
) -> AnimationElement<E> {
    element.with_animation(id, one_shot(duration), |element, delta| {
        element.opacity(delta)
    })
}

/// Fades in while moving `distance` from `edge` into place. Moves by offset, not by margin, so
/// the layout of what it holds (a long list) is not measured again each frame.
pub fn slide_in<E: IntoElement + Styled + 'static>(
    element: E,
    id: impl Into<ElementId>,
    duration: Duration,
    edge: Edge,
    distance: Pixels,
) -> AnimationElement<E> {
    element.with_animation(id, one_shot(duration), move |element, delta| {
        let away = distance * (1. - delta);
        let element = element.relative().opacity(delta);
        match edge {
            Edge::Left => element.left(-away),
            Edge::Right => element.left(away),
            Edge::Top => element.top(-away),
            Edge::Bottom => element.top(away),
        }
    })
}

/// An overlay's entrance: fades in and rises `OVERLAY_RISE` over `OVERLAY`.
pub fn rise_in<E: IntoElement + Styled + 'static>(
    element: E,
    id: impl Into<ElementId>,
) -> AnimationElement<E> {
    slide_in(element, id, OVERLAY, Edge::Bottom, OVERLAY_RISE)
}

/// A bar whose width follows a share from 0 to 1. A new share glides from the length on show; the
/// first drawing starts at the share itself, so opening a window does not replay old progress.
pub fn glide_to_share<E: IntoElement + Styled + 'static>(
    element: E,
    id: impl Into<ElementId>,
    share: f32,
) -> SpringAnimationElement<E> {
    element.with_spring(
        id,
        SpringAnimation::new(TRAIL_SPRING)
            .to(share.clamp(0., 1.))
            .with_epsilon(0.002),
        |element, share| element.w(relative(share.clamp(0., 1.))),
    )
}

/// Something that moves to a new place along one coordinate (a segment's thumb sliding to the
/// picked option): it glides there from where it was, and starts at `target` on first drawing.
pub fn glide_to<E: IntoElement + Styled + 'static>(
    element: E,
    id: impl Into<ElementId>,
    target: f32,
    place: impl FnOnce(E, f32) -> E + 'static,
) -> SpringAnimationElement<E> {
    element.with_spring(
        id,
        SpringAnimation::new(GLIDE_SPRING)
            .to(target)
            .with_epsilon(0.002),
        place,
    )
}

/// An icon that pops to full size when `is_shown` turns true, and is drawn at full size when it
/// already was on first drawing.
pub fn pop_when_shown(
    icon: Svg,
    id: impl Into<ElementId>,
    is_shown: bool,
) -> SpringAnimationElement<Svg> {
    icon.with_spring(
        id,
        SpringAnimation::new(POP_SPRING).to(is_shown),
        |icon, phase| {
            let scale = phase.0.max(0.);
            icon.with_transformation(Transformation::scale(size(scale, scale)))
        },
    )
}

/// An icon that pops in once when it first appears: the finish card's check. It grows from a little
/// under its size with the arrival easing, so it lands without a bounce.
pub fn pop_in(icon: Svg, id: impl Into<ElementId>) -> AnimationElement<Svg> {
    icon.with_animation(id, one_shot(STATE), |icon, delta| {
        let scale = 0.4 + 0.6 * delta;
        icon.with_transformation(Transformation::scale(size(scale, scale)))
    })
}

/// Like `pop_when_shown`, for an icon inside a label: it grows from no width to `size` and its
/// `gap` after it, so the label makes room as the icon pops in, and takes none while hidden.
pub fn grow_when_shown(
    icon: Svg,
    id: impl Into<ElementId>,
    is_shown: bool,
    size: Pixels,
    gap: Pixels,
) -> SpringAnimationElement<Svg> {
    icon.with_spring(
        id,
        SpringAnimation::new(POP_SPRING).to(is_shown),
        move |icon, phase| {
            let grown = phase.0.max(0.);
            icon.size(size * grown).mr(gap * grown.min(1.))
        },
    )
}

/// An icon that turns between two angles (in turns of a full circle) as `turned` changes; used
/// for chevrons that point right when folded and down when open.
pub fn turn_to(
    icon: Svg,
    id: impl Into<ElementId>,
    turned: bool,
    quarter_turns: f32,
) -> SpringAnimationElement<Svg> {
    icon.with_spring(
        id,
        SpringAnimation::new(TURN_SPRING).to(turned),
        move |icon, phase| {
            icon.with_transformation(Transformation::rotate(percentage(
                phase.0 * quarter_turns / 4.,
            )))
        },
    )
}

/// Where the sweep's segment starts, as a share of its track, at a phase of the loop. Phase 0
/// already shows the segment well inside the track, so a still picture (reduce motion) shows it.
fn sweep_left(phase: f32) -> f32 {
    const SEGMENT: f32 = 0.32;
    const START: f32 = 0.42;
    ((phase + START) % 1.) * (1. + SEGMENT) - SEGMENT
}

/// The indeterminate busy bar: a trail segment sweeping along a quiet track, while busy only.
pub fn busy_sweep(colors: &Colors, id: impl Into<ElementId>) -> impl IntoElement {
    let segment = div()
        .absolute()
        .top_0()
        .bottom_0()
        .w(relative(0.32))
        .rounded_full()
        .bg(colors.trail.left_right());
    div()
        .relative()
        .w_full()
        .h(px(3.))
        .rounded_full()
        .overflow_hidden()
        .bg(colors.subtle_border)
        .child(segment.with_animation(
            id,
            Animation::new(SWEEP).repeat().with_max_fps(BUSY_FPS),
            |segment, phase| segment.left(relative(sweep_left(phase))),
        ))
}

/// Breathes the opacity of a status mark while busy.
pub fn pulse<E: IntoElement + Styled + 'static>(
    element: E,
    id: impl Into<ElementId>,
) -> AnimationElement<E> {
    element.with_animation(
        id,
        Animation::new(PULSE)
            .repeat()
            .with_easing(gpui::pulsating_between(0.45, 1.))
            .with_max_fps(BUSY_FPS),
        Styled::opacity,
    )
}

/// A small trail-coloured dot that breathes while the app works; put it before a status line.
pub fn busy_dot(colors: &Colors, id: impl Into<ElementId>) -> impl IntoElement {
    pulse(
        div()
            .flex_none()
            .size(px(8.))
            .rounded_full()
            .bg(colors.trail.left_right()),
        id,
    )
}

/// One pass of the shine over a busy line.
const SHIMMER: Duration = Duration::from_millis(1800);
/// How far the shine reaches either side of its centre, as a share of the line.
const SHINE_REACH: f32 = 0.18;

/// A line that a lighter shine runs over again and again while busy. Reduce motion holds the shine
/// before the line's start, so the line stands still in its own colour.
pub fn shimmer_text(
    text: SharedString,
    color: Rgba,
    shine: Rgba,
    id: impl Into<ElementId>,
) -> impl IntoElement {
    div().text_color(color).with_animation(
        id,
        Animation::new(SHIMMER).repeat().with_max_fps(BUSY_FPS),
        move |line, phase| {
            let highlights = shine_over(&text, phase, color, shine);
            line.child(StyledText::new(text.clone()).with_highlights(highlights))
        },
    )
}

/// Each character's colour at `phase`: `shine` at the centre of the shine, fading to `color` at its reach.
fn shine_over(
    text: &str,
    phase: f32,
    color: Rgba,
    shine: Rgba,
) -> Vec<(Range<usize>, HighlightStyle)> {
    let count = text.chars().count().max(1) as f32;
    let centre = phase * (1. + 2. * SHINE_REACH) - SHINE_REACH;
    text.char_indices()
        .enumerate()
        .filter_map(|(index, (start, character))| {
            let place = (index as f32 + 0.5) / count;
            let strength = 1. - (place - centre).abs() / SHINE_REACH;
            (strength > 0.).then(|| {
                let style = HighlightStyle {
                    color: Some(mixed(color, shine, strength).into()),
                    ..HighlightStyle::default()
                };
                (start..start + character.len_utf8(), style)
            })
        })
        .collect()
}

fn mixed(from: Rgba, to: Rgba, share: f32) -> Rgba {
    let at = |a: f32, b: f32| a + (b - a) * share;
    Rgba {
        r: at(from.r, to.r),
        g: at(from.g, to.g),
        b: at(from.b, to.b),
        a: at(from.a, to.a),
    }
}
