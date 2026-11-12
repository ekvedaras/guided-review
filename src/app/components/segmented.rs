use gpui::prelude::FluentBuilder;
use gpui::{
    App, ClickEvent, FontWeight, InteractiveElement, IntoElement, ParentElement, Pixels,
    RenderOnce, SharedString, StatefulInteractiveElement, Styled, Window, div, px, relative,
};

use super::icon::{Icon, sized_icon};
use super::motion::glide_to;
use super::palette::{CONTROL_RADIUS, Colors, INNER_RADIUS, Tone};

type OnPick = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

/// One option of a `Segmented` control.
pub struct Segment {
    id: SharedString,
    label: SharedString,
    icon: Option<Icon>,
    on_pick: OnPick,
}

impl Segment {
    pub fn new(
        id: impl Into<SharedString>,
        label: impl Into<SharedString>,
        on_pick: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            icon: None,
            on_pick: Box::new(on_pick),
        }
    }

    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }
}

/// How the options line up: side by side, each as wide as the widest needs, or one under another.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SegmentLayout {
    Row { segment_width: Pixels },
    Column,
}

/// A one-of-N choice drawn as a sunken track whose picked option is a raised thumb; picking
/// another glides the thumb over to it.
#[derive(IntoElement)]
pub struct Segmented {
    id: SharedString,
    layout: SegmentLayout,
    segments: Vec<Segment>,
    picked: Option<usize>,
    thumb_tone: Tone,
    is_invalid: bool,
    is_compact: bool,
}

impl Segmented {
    pub fn new(id: impl Into<SharedString>, layout: SegmentLayout, segments: Vec<Segment>) -> Self {
        Self {
            id: id.into(),
            layout,
            segments,
            picked: None,
            thumb_tone: Tone::Neutral,
            is_invalid: false,
            is_compact: false,
        }
    }

    pub fn picked(mut self, index: Option<usize>) -> Self {
        self.picked = index;
        self
    }

    /// The thumb wears a state's colours, such as an approval's green.
    pub fn thumb_tone(mut self, tone: Tone) -> Self {
        self.thumb_tone = tone;
        self
    }

    /// Short enough to sit in a box header beside its buttons.
    pub fn compact(mut self) -> Self {
        self.is_compact = true;
        self
    }

    /// What is picked cannot be saved: the track's edge turns danger.
    pub fn invalid(mut self, is_invalid: bool) -> Self {
        self.is_invalid = is_invalid;
        self
    }
}

const SEGMENT_HEIGHT: Pixels = px(28.);
const COMPACT_SEGMENT_HEIGHT: Pixels = px(20.);
const TRACK_PADDING: Pixels = px(3.);
const COMPACT_TRACK_PADDING: Pixels = px(2.);

fn count_of(segments: &[Segment]) -> f32 {
    f32::from(u16::try_from(segments.len().max(1)).unwrap_or(u16::MAX))
}

fn index_of(index: usize) -> f32 {
    f32::from(u16::try_from(index).unwrap_or(u16::MAX))
}

impl RenderOnce for Segmented {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let colors = Colors::active(cx);
        let count = count_of(&self.segments);
        let layout = self.layout;
        let is_compact = self.is_compact;
        let (thumb_ground, thumb_border, picked_text) = match self.thumb_tone {
            Tone::Neutral => (colors.raised_fill(), colors.border, colors.accent),
            tone => {
                let tone = colors.tone(tone);
                (tone.background.into(), tone.border, tone.text)
            }
        };
        let thumb = self.picked.map(|picked| {
            let thumb = div()
                .absolute()
                .rounded(px(INNER_RADIUS))
                .border_1()
                .border_color(thumb_border)
                .bg(thumb_ground)
                .shadow(colors.control_shadow());
            let thumb = match layout {
                SegmentLayout::Row { .. } => thumb.top_0().bottom_0().w(relative(1. / count)),
                SegmentLayout::Column => thumb.left_0().right_0().h(relative(1. / count)),
            };
            glide_to(
                thumb,
                "thumb",
                index_of(picked),
                move |thumb, at| match layout {
                    SegmentLayout::Row { .. } => thumb.left(relative(at / count)),
                    SegmentLayout::Column => thumb.top(relative(at / count)),
                },
            )
        });
        let segments = self
            .segments
            .into_iter()
            .enumerate()
            .map(|(index, segment)| {
                let is_picked = self.picked == Some(index);
                let text = if is_picked {
                    picked_text
                } else {
                    colors.muted_text
                };
                div()
                    .id(segment.id)
                    .relative()
                    .h(if is_compact {
                        COMPACT_SEGMENT_HEIGHT
                    } else {
                        SEGMENT_HEIGHT
                    })
                    .flex()
                    .items_center()
                    .gap_2()
                    .when(is_compact, |segment| segment.px_2().text_xs())
                    .when(!is_compact, Styled::px_3)
                    .rounded(px(INNER_RADIUS))
                    .cursor_pointer()
                    .whitespace_nowrap()
                    .text_color(text)
                    .map(|segment| match layout {
                        SegmentLayout::Row { segment_width } => {
                            segment.w(segment_width).justify_center()
                        }
                        SegmentLayout::Column => segment.w_full(),
                    })
                    .when(is_picked, |segment| segment.font_weight(FontWeight::MEDIUM))
                    .when(!is_picked, |segment| {
                        segment.hover(|segment| segment.text_color(colors.text))
                    })
                    .on_click(segment.on_pick)
                    .children(
                        segment
                            .icon
                            .map(|icon| sized_icon(icon, text, px(14.)).flex_none()),
                    )
                    .child(segment.label)
            });
        div()
            .id(self.id)
            .flex_none()
            .p(if is_compact {
                COMPACT_TRACK_PADDING
            } else {
                TRACK_PADDING
            })
            .rounded(px(CONTROL_RADIUS))
            .border_1()
            .border_color(if self.is_invalid {
                colors.danger
            } else {
                colors.subtle_border
            })
            .bg(colors.inset)
            .when(matches!(layout, SegmentLayout::Row { .. }), |track| {
                track.flex().self_start()
            })
            .child(
                div()
                    .relative()
                    .flex_none()
                    .flex()
                    .when(layout == SegmentLayout::Column, Styled::flex_col)
                    .children(thumb)
                    .children(segments),
            )
    }
}
