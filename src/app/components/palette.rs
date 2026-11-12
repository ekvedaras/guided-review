use gpui::{
    App, Background, BoxShadow, Global, Rgba, Window, WindowAppearance, linear_color_stop,
    linear_gradient, px, rgb,
};

use crate::viewer::{Capture, ReadProgress};

/// GPUI has no usable default font outside Zed, so every text names its family.
pub const TEXT_FONT: &str = "Helvetica Neue";
pub const CODE_FONT: &str = "Menlo";
/// Diagrams measure their text with this file, so it must hold `TEXT_FONT`.
pub const TEXT_FONT_FILE: &str = "/System/Library/Fonts/HelveticaNeue.ttc";

/// One radius for every card on the page, one for every control and one for what sits inside a
/// control or is too small for the control's (a segment's thumb, a chip, a key cap), so corners agree.
pub const CARD_RADIUS: f32 = 12.;
pub const CONTROL_RADIUS: f32 = 8.;
pub const INNER_RADIUS: f32 = 6.;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Appearance {
    Light,
    Dark,
}

/// The window sets the active appearance at start and when macOS flips it.
impl Global for Appearance {}

impl Appearance {
    pub fn active(cx: &App) -> Self {
        *cx.global::<Self>()
    }

    pub fn of(window: &Window) -> Self {
        #[cfg(feature = "snapshots")]
        if let Some(staged) = crate::app::snapshots::appearance() {
            return staged;
        }
        Self::of_window(window.appearance())
    }

    fn of_window(appearance: WindowAppearance) -> Self {
        match appearance {
            WindowAppearance::Light | WindowAppearance::VibrantLight => Self::Light,
            WindowAppearance::Dark | WindowAppearance::VibrantDark => Self::Dark,
        }
    }
}

/// The colour ladders the roles are picked from (OKLCH-derived, chroma falling near white and
/// black). Components never read these; they read the roles of `Colors`.
#[expect(
    dead_code,
    reason = "the ladders are kept whole, so a role can move a step without a new colour"
)]
mod ladder {
    pub const WHITE: u32 = 0xffffff;
    pub const BLACK: u32 = 0x000000;

    /// Neutral, tinted to the brand hue.
    pub mod ink {
        pub const S0: u32 = 0xfdfdfe;
        pub const S1: u32 = 0xf8f9fd;
        pub const S2: u32 = 0xf2f3fa;
        pub const S3: u32 = 0xe9ebf3;
        pub const S4: u32 = 0xdbdde7;
        pub const S5: u32 = 0xc4c7d2;
        pub const S6: u32 = 0xa1a4b2;
        pub const S7: u32 = 0x7c7f8e;
        pub const S8: u32 = 0x5f6271;
        pub const S9: u32 = 0x494c5c;
        pub const S10: u32 = 0x353848;
        pub const S11: u32 = 0x272a39;
        pub const S12: u32 = 0x1d202d;
        pub const S13: u32 = 0x151724;
        pub const S14: u32 = 0x0e111c;
        pub const S15: u32 = 0x080a14;
    }

    /// Brand and action.
    pub mod iris {
        pub const S50: u32 = 0xf4f7fe;
        pub const S100: u32 = 0xe7ecfe;
        pub const S200: u32 = 0xd1dbff;
        pub const S300: u32 = 0xb2c0ff;
        pub const S400: u32 = 0x92a1fe;
        pub const S500: u32 = 0x767efe;
        pub const S600: u32 = 0x605eeb;
        pub const S700: u32 = 0x4e48c0;
        pub const S800: u32 = 0x3d3793;
        pub const S900: u32 = 0x2a275e;
        pub const S950: u32 = 0x1c1c34;
    }

    /// The trail's far end, and information.
    pub mod azure {
        pub const S50: u32 = 0xf0f8fe;
        pub const S100: u32 = 0xddf0fe;
        pub const S200: u32 = 0xbce2fe;
        pub const S300: u32 = 0x86cdfe;
        pub const S400: u32 = 0x42b4f9;
        pub const S500: u32 = 0x0399e0;
        pub const S600: u32 = 0x027eb9;
        pub const S700: u32 = 0x026494;
        pub const S800: u32 = 0x034d73;
        pub const S900: u32 = 0x013450;
        pub const S950: u32 = 0x0d212f;
    }

    pub mod green {
        pub const S50: u32 = 0xeafdee;
        pub const S100: u32 = 0xd7f6de;
        pub const S200: u32 = 0xb9ebc5;
        pub const S300: u32 = 0x8cd9a1;
        pub const S400: u32 = 0x59c27b;
        pub const S500: u32 = 0x1aaa5a;
        pub const S600: u32 = 0x078d48;
        pub const S700: u32 = 0x037138;
        pub const S800: u32 = 0x02572a;
        pub const S900: u32 = 0x063b1c;
        pub const S950: u32 = 0x112416;
    }

    pub mod red {
        pub const S50: u32 = 0xfef4f3;
        pub const S100: u32 = 0xfee7e4;
        pub const S200: u32 = 0xffcfca;
        pub const S300: u32 = 0xfeaba4;
        pub const S400: u32 = 0xff7b74;
        pub const S500: u32 = 0xee514f;
        pub const S600: u32 = 0xd03034;
        pub const S700: u32 = 0xaa2025;
        pub const S800: u32 = 0x821a1d;
        pub const S900: u32 = 0x551918;
        pub const S950: u32 = 0x311615;
    }

    pub mod amber {
        pub const S50: u32 = 0xfff6e3;
        pub const S100: u32 = 0xfbebcd;
        pub const S200: u32 = 0xf3d9a9;
        pub const S300: u32 = 0xe6bd74;
        pub const S400: u32 = 0xd59e35;
        pub const S500: u32 = 0xbc8304;
        pub const S600: u32 = 0x9c6b05;
        pub const S700: u32 = 0x7e5402;
        pub const S800: u32 = 0x633f00;
        pub const S900: u32 = 0x442a01;
        pub const S950: u32 = 0x2a1c0c;
    }

    /// Code constants only.
    pub mod orchid {
        pub const S50: u32 = 0xfff2ff;
        pub const S100: u32 = 0xfee3ff;
        pub const S200: u32 = 0xf9cafb;
        pub const S300: u32 = 0xeca8f0;
        pub const S400: u32 = 0xdb83e0;
        pub const S500: u32 = 0xc561cc;
        pub const S600: u32 = 0xaa44b1;
        pub const S700: u32 = 0x89338f;
        pub const S800: u32 = 0x69286e;
        pub const S900: u32 = 0x451f48;
        pub const S950: u32 = 0x29182a;
    }
}

use ladder::{BLACK, WHITE, amber, azure, green, ink, iris, red};

fn translucent(hex: u32, alpha: f32) -> Rgba {
    rgb(hex).opacity(alpha)
}

/// Two colours a fill runs between; `start` is the top or the left end.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Gradient {
    pub start: Rgba,
    pub end: Rgba,
}

impl Gradient {
    fn of(start: u32, end: u32) -> Self {
        Self {
            start: rgb(start),
            end: rgb(end),
        }
    }

    pub fn top_down(self) -> Background {
        linear_gradient(
            180.,
            linear_color_stop(self.start, 0.),
            linear_color_stop(self.end, 1.),
        )
    }

    pub fn left_right(self) -> Background {
        linear_gradient(
            90.,
            linear_color_stop(self.start, 0.),
            linear_color_stop(self.end, 1.),
        )
    }

    /// From the top left corner to the bottom right one.
    pub fn diagonal(self) -> Background {
        linear_gradient(
            135.,
            linear_color_stop(self.start, 0.),
            linear_color_stop(self.end, 1.),
        )
    }

    /// The same run at a share of its opacity, for a fill that fades in or out.
    pub fn opacity(self, factor: f32) -> Self {
        Self {
            start: self.start.opacity(factor),
            end: self.end.opacity(factor),
        }
    }
}

/// Every colour of the app for one appearance. Drawings that must match the app (diagrams)
/// take a `Colors` of the appearance they draw for and do not read the active one.
#[derive(Debug, Clone, Copy)]
pub struct Colors {
    /// The canvas and the ground of code.
    pub background: Rgba,
    /// The coloured rail on the left, top to bottom; draw it with `sidebar_ground`.
    pub sidebar: Gradient,
    /// Raised parts of the page: piece headers, bands, cards.
    pub surface: Rgba,
    /// A header band's surface runs to this a step darker; draw it with `header_band`.
    pub surface_sheen: Rgba,
    /// What floats over the page: palettes, the switcher, dialogs, tooltips; with `overlay_shadow`.
    pub overlay: Rgba,
    /// A control that stands up from the page like a key: a secondary button, a segment's thumb.
    pub raised: Gradient,
    /// What a control is sunk into: the track of a segmented control.
    pub inset: Rgba,
    /// The edge of a raised control under the mouse, leaning to the accent.
    pub accent_border: Rgba,
    /// The ground of a section's icon tile, iris running to azure.
    pub icon_tile: Gradient,
    /// The ground of a button for something finished.
    pub success_fill: Gradient,
    /// Under the mouse on anything clickable that is not a primary button.
    pub hover: Rgba,
    /// Under the mouse button while it is held down (`.active()`).
    pub pressed: Rgba,
    pub border: Rgba,
    pub subtle_border: Rgba,
    pub text: Rgba,
    /// The sidebar's labels: a step quieter than `text`, so the page leads.
    pub sidebar_text: Rgba,
    pub muted_text: Rgba,
    /// Behind text the reader selected.
    pub text_selection: Rgba,
    /// The selected row, the open screen in the sidebar, a toggle that is on.
    pub current: Rgba,
    /// The 1 px edge of a `current` ground.
    pub current_border: Rgba,
    pub removed_line: Rgba,
    pub added_line: Rgba,
    pub removed_gutter: Rgba,
    pub added_gutter: Rgba,
    /// Behind the words that changed inside a removed or added line, which are drawn in `text`.
    pub removed_word: Rgba,
    pub added_word: Rgba,
    /// Links, marks, the focus ring and lines in the accent colour.
    pub accent: Rgba,
    /// The fill of a primary button, top to bottom; `accent` reads too light under white text in the dark.
    pub accent_fill: Gradient,
    pub accent_fill_hover: Gradient,
    pub accent_fill_pressed: Rgba,
    pub on_accent_fill: Rgba,
    pub warning: Rgba,
    pub warning_background: Rgba,
    pub warning_border: Rgba,
    pub success: Rgba,
    pub success_background: Rgba,
    pub success_border: Rgba,
    pub danger: Rgba,
    pub danger_background: Rgba,
    pub danger_border: Rgba,
    /// Progress, and only that: the tour's meter, the current step's dot, the busy sweep and the
    /// line under the title bar that follows the scroll down a screen.
    pub trail: Gradient,
    /// The trail once every step is read.
    pub trail_done: Gradient,
    /// Dims what lies under a modal overlay.
    pub scrim: Rgba,
    /// The far, soft layer of a floating thing's shadow; `overlay_shadow` adds a nearer one.
    pub shadow: Rgba,
    /// The faint lift of a card on the page; transparent in the dark, where cards do not cast.
    pub card_shadow: Rgba,
    /// What a raised control casts, in both appearances: it reads as a key that can be pushed in.
    pub control_shadow: Rgba,
    /// A quiet iris wash at the top of a screen, behind its heading, fading into `background`.
    pub page_wash: Gradient,
    pub checker_light: Rgba,
    pub checker_dark: Rgba,
    code: CodeColors,
}

#[derive(Debug, Clone, Copy)]
struct CodeColors {
    keyword: Rgba,
    string: Rgba,
    comment: Rgba,
    function: Rgba,
    type_name: Rgba,
    number: Rgba,
    constant: Rgba,
    tag: Rgba,
    attribute: Rgba,
    property: Rgba,
    variable: Rgba,
    parameter: Rgba,
    punctuation: Rgba,
}

/// What a mark, note or band says about its state, which picks its text, ground and edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Neutral,
    Accent,
    Success,
    Warning,
    Danger,
}

/// The three roles one `Tone` is drawn with.
#[derive(Debug, Clone, Copy)]
pub struct ToneColors {
    pub text: Rgba,
    pub background: Rgba,
    pub border: Rgba,
}

/// Whether the trail has reached the end of the tour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrailStage {
    UnderWay,
    Complete,
}

impl TrailStage {
    pub fn of(progress: ReadProgress) -> Self {
        if progress.is_complete() {
            Self::Complete
        } else {
            Self::UnderWay
        }
    }
}

impl Colors {
    fn light() -> Self {
        let text = rgb(ink::S13);
        let muted_text = rgb(0x5a5d6c);
        Self {
            background: rgb(WHITE),
            sidebar: Gradient::of(0xf4f5fc, 0xeaecfa),
            surface: rgb(0xf7f8fd),
            surface_sheen: rgb(0xf1f2f9),
            overlay: rgb(WHITE),
            raised: Gradient::of(WHITE, ink::S2),
            inset: rgb(0xeceef6),
            accent_border: rgb(iris::S300),
            icon_tile: Gradient::of(iris::S100, azure::S100),
            success_fill: Gradient::of(green::S50, green::S100),
            hover: rgb(0xeceef6),
            pressed: rgb(ink::S4),
            border: rgb(0xc9ccd8),
            subtle_border: rgb(0xe3e5ee),
            text,
            sidebar_text: rgb(ink::S10),
            muted_text,
            current: rgb(0xe4e8fe),
            current_border: rgb(iris::S200),
            text_selection: rgb(iris::S200),
            removed_line: rgb(0xfee9e6),
            added_line: rgb(0xe4f8e9),
            removed_gutter: rgb(0xffd2cd),
            added_gutter: rgb(0xc8f0d2),
            removed_word: rgb(0xffc4bd),
            added_word: rgb(0xb4ebc2),
            accent: rgb(iris::S700),
            accent_fill: Gradient::of(iris::S600, iris::S700),
            accent_fill_hover: Gradient::of(0x6462ee, 0x5650cc),
            accent_fill_pressed: rgb(iris::S700),
            on_accent_fill: rgb(WHITE),
            warning: rgb(amber::S700),
            warning_background: rgb(amber::S50),
            warning_border: rgb(amber::S300),
            success: rgb(green::S700),
            success_background: rgb(green::S50),
            success_border: rgb(green::S300),
            danger: rgb(red::S700),
            danger_background: rgb(red::S50),
            danger_border: rgb(red::S300),
            trail: Gradient::of(iris::S500, azure::S400),
            trail_done: Gradient::of(green::S500, green::S400),
            scrim: translucent(ink::S15, 0.32),
            shadow: translucent(ink::S15, 0.16),
            card_shadow: translucent(ink::S15, 0.06),
            control_shadow: translucent(ink::S15, 0.12),
            page_wash: Gradient::of(0xeff1fe, WHITE),
            checker_light: rgb(WHITE),
            checker_dark: rgb(ink::S3),
            code: CodeColors {
                keyword: rgb(iris::S700),
                string: rgb(green::S700),
                comment: rgb(0x686b7a),
                function: rgb(azure::S700),
                type_name: rgb(amber::S700),
                number: rgb(0x9c2f8f),
                constant: rgb(0x9c2f8f),
                tag: rgb(green::S800),
                attribute: rgb(azure::S700),
                property: rgb(azure::S700),
                variable: text,
                parameter: rgb(amber::S800),
                punctuation: muted_text,
            },
        }
    }

    fn dark() -> Self {
        let text = rgb(0xeaeaed);
        let muted_text = rgb(0xa4a4a9);
        Self {
            background: rgb(0x16171a),
            sidebar: Gradient::of(0x15161c, 0x0d0e11),
            surface: rgb(0x1e1f23),
            surface_sheen: rgb(0x1b1c1f),
            overlay: rgb(0x202125),
            raised: Gradient::of(0x2d2e33, 0x25262b),
            inset: rgb(0x121316),
            accent_border: rgb(iris::S600),
            icon_tile: Gradient::of(iris::S900, azure::S900),
            success_fill: Gradient::of(0x163220, green::S950),
            hover: rgb(0x292a2f),
            pressed: rgb(0x38393e),
            border: rgb(0x3c3d43),
            subtle_border: rgb(0x292b2f),
            text,
            sidebar_text: rgb(0xdddde0),
            muted_text,
            current: rgb(0x272a55),
            current_border: rgb(iris::S800),
            text_selection: rgb(iris::S800),
            removed_line: rgb(0x2f181a),
            added_line: rgb(0x13291c),
            removed_gutter: rgb(0x4c1f22),
            added_gutter: rgb(0x1a4129),
            removed_word: rgb(0x5c2328),
            added_word: rgb(0x1f4c30),
            accent: rgb(iris::S400),
            accent_fill: Gradient::of(0x5652d8, 0x4640a8),
            accent_fill_hover: Gradient::of(iris::S600, iris::S700),
            accent_fill_pressed: rgb(0x403a9c),
            on_accent_fill: rgb(WHITE),
            warning: rgb(amber::S400),
            warning_background: rgb(amber::S950),
            warning_border: rgb(amber::S800),
            success: rgb(green::S400),
            success_background: rgb(green::S950),
            success_border: rgb(green::S800),
            danger: rgb(red::S400),
            danger_background: rgb(red::S950),
            danger_border: rgb(red::S800),
            trail: Gradient::of(iris::S500, azure::S400),
            trail_done: Gradient::of(green::S500, green::S400),
            scrim: translucent(BLACK, 0.55),
            shadow: translucent(BLACK, 0.5),
            card_shadow: translucent(BLACK, 0.),
            control_shadow: translucent(BLACK, 0.45),
            page_wash: Gradient::of(0x1d1e2a, 0x16171a),
            checker_light: rgb(0x2b2b2b),
            checker_dark: rgb(0x3b3b3b),
            code: CodeColors {
                keyword: rgb(iris::S300),
                string: rgb(green::S300),
                comment: rgb(0x919297),
                function: rgb(azure::S300),
                type_name: rgb(amber::S300),
                number: rgb(0xeaa5e0),
                constant: rgb(0xeaa5e0),
                tag: rgb(green::S300),
                attribute: rgb(azure::S300),
                property: rgb(azure::S300),
                variable: text,
                parameter: rgb(amber::S200),
                punctuation: muted_text,
            },
        }
    }

    pub fn of(appearance: Appearance) -> Self {
        match appearance {
            Appearance::Light => Self::light(),
            Appearance::Dark => Self::dark(),
        }
    }

    pub fn active(cx: &App) -> Self {
        Self::of(Appearance::active(cx))
    }

    pub fn code(&self, capture: Capture) -> Rgba {
        let code = &self.code;
        match capture {
            Capture::Keyword | Capture::Operator => code.keyword,
            Capture::String | Capture::StringSpecial => code.string,
            Capture::Comment => code.comment,
            Capture::Function
            | Capture::FunctionBuiltin
            | Capture::FunctionMethod
            | Capture::Constructor => code.function,
            Capture::Type | Capture::TypeBuiltin => code.type_name,
            Capture::Number => code.number,
            Capture::Constant | Capture::ConstantBuiltin => code.constant,
            Capture::Tag => code.tag,
            Capture::Attribute => code.attribute,
            Capture::Property => code.property,
            Capture::Variable | Capture::VariableBuiltin => code.variable,
            Capture::VariableParameter => code.parameter,
            Capture::Punctuation
            | Capture::PunctuationBracket
            | Capture::PunctuationDelimiter
            | Capture::PunctuationSpecial => code.punctuation,
        }
    }

    pub fn tone(&self, tone: Tone) -> ToneColors {
        let (text, background, border) = match tone {
            Tone::Neutral => (self.muted_text, self.surface, self.subtle_border),
            Tone::Accent => (self.accent, self.current, self.current_border),
            Tone::Success => (self.success, self.success_background, self.success_border),
            Tone::Warning => (self.warning, self.warning_background, self.warning_border),
            Tone::Danger => (self.danger, self.danger_background, self.danger_border),
        };
        ToneColors {
            text,
            background,
            border,
        }
    }

    /// The top of a screen, behind its heading.
    pub fn page_wash_fill(&self) -> Background {
        self.page_wash.top_down()
    }

    /// The sidebar's ground, lighter (light) or deeper (dark) at the top.
    pub fn sidebar_ground(&self) -> Background {
        self.sidebar.top_down()
    }

    /// A primary button at rest and under the mouse; pressed is the flat `accent_fill_pressed`.
    pub fn primary_fill(&self) -> Background {
        self.accent_fill.top_down()
    }

    pub fn primary_fill_hover(&self) -> Background {
        self.accent_fill_hover.top_down()
    }

    /// The trail for a progress: under way it runs iris to azure, complete it turns green.
    pub fn trail_of(&self, stage: TrailStage) -> Gradient {
        match stage {
            TrailStage::UnderWay => self.trail,
            TrailStage::Complete => self.trail_done,
        }
    }

    /// A trail bar, left to right.
    pub fn trail_fill(&self, stage: TrailStage) -> Background {
        self.trail_of(stage).left_right()
    }

    /// File header bands and other bands: surface with a one-step sheen downwards.
    pub fn header_band(&self) -> Background {
        Gradient {
            start: self.surface,
            end: self.surface_sheen,
        }
        .top_down()
    }

    /// Palettes, the switcher, dialogs, tooltips: a near and a far soft layer, cast downwards.
    pub fn overlay_shadow(&self) -> Vec<BoxShadow> {
        vec![
            BoxShadow::new(px(0.), px(2.), self.shadow.opacity(0.6).into()).blur_radius(px(6.)),
            BoxShadow::new(px(0.), px(12.), self.shadow.into()).blur_radius(px(32.)),
        ]
    }

    /// Cards on the page: a hairline lift in the light; none in the dark.
    pub fn card_shadow(&self) -> Vec<BoxShadow> {
        if self.card_shadow.a == 0. {
            return Vec::new();
        }
        vec![BoxShadow::new(px(0.), px(1.), self.card_shadow.into()).blur_radius(px(2.))]
    }

    /// A primary button sits a little above the page, with a short drop and no glow of its colour.
    pub fn primary_shadow(&self) -> Vec<BoxShadow> {
        vec![
            BoxShadow::new(px(0.), px(1.), translucent(iris::S900, 0.4).into()).blur_radius(px(2.)),
            BoxShadow::new(px(0.), px(2.), self.shadow.opacity(0.3).into()).blur_radius(px(4.)),
        ]
    }

    /// A raised control (a secondary button, a segment's thumb): a short, soft drop.
    pub fn control_shadow(&self) -> Vec<BoxShadow> {
        vec![BoxShadow::new(px(0.), px(1.), self.control_shadow.into()).blur_radius(px(2.))]
    }

    /// A bar pinned to the bottom of a panel casts upwards over what scrolls under it.
    pub fn bar_shadow(&self) -> Vec<BoxShadow> {
        vec![BoxShadow::new(px(0.), px(-6.), self.shadow.opacity(0.5).into()).blur_radius(px(20.))]
    }

    /// The page lies on top of the sidebar, and the Ask panel on top of the page: each casts leftwards.
    pub fn page_shadow(&self) -> Vec<BoxShadow> {
        vec![BoxShadow::new(px(-2.), px(0.), self.shadow.opacity(0.6).into()).blur_radius(px(14.))]
    }

    /// A bar pinned to the top of the page casts downwards over what scrolls under it.
    pub fn sticky_shadow(&self) -> Vec<BoxShadow> {
        vec![BoxShadow::new(px(0.), px(4.), self.shadow.opacity(0.35).into()).blur_radius(px(12.))]
    }

    /// The ground of a secondary button, top down.
    pub fn raised_fill(&self) -> Background {
        self.raised.top_down()
    }

    /// Around a focused text box, with a 1 px `accent` border: a soft accent ring outside it.
    pub fn focus_ring(&self) -> Vec<BoxShadow> {
        vec![BoxShadow::new(px(0.), px(0.), self.accent.opacity(0.25).into()).spread_radius(px(3.))]
    }

    /// Fainter than `muted_text`, for what is there to look up and not read day to day, like the version.
    pub fn faint_text(&self) -> Rgba {
        self.muted_text.opacity(0.6)
    }

    /// The soft disc behind the current step's trail dot.
    pub fn trail_glow(&self) -> Rgba {
        self.trail.start.opacity(0.28)
    }
}
