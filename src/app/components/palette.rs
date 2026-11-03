use gpui::{App, Global, Rgba, WindowAppearance, rgb};

use crate::viewer::Capture;

/// GPUI has no usable default font outside Zed, so every text names its family.
pub const TEXT_FONT: &str = "Helvetica Neue";
pub const CODE_FONT: &str = "Menlo";
/// Diagrams measure their text with this file, so it must hold `TEXT_FONT`.
pub const TEXT_FONT_FILE: &str = "/System/Library/Fonts/HelveticaNeue.ttc";

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

    pub fn of_window(appearance: WindowAppearance) -> Self {
        match appearance {
            WindowAppearance::Light | WindowAppearance::VibrantLight => Self::Light,
            WindowAppearance::Dark | WindowAppearance::VibrantDark => Self::Dark,
        }
    }
}

/// Every colour of the app for one appearance. Drawings that must match the app (diagrams)
/// take a `Colors` of the appearance they draw for and do not read the active one.
#[derive(Debug, Clone, Copy)]
pub struct Colors {
    pub background: Rgba,
    pub sidebar: Rgba,
    /// Raised parts of the page: piece headers, bands, cards.
    pub surface: Rgba,
    /// Under the mouse on anything clickable that is not a primary button.
    pub hover: Rgba,
    pub border: Rgba,
    pub subtle_border: Rgba,
    pub text: Rgba,
    pub muted_text: Rgba,
    /// Behind code text the reader selected to copy.
    pub text_selection: Rgba,
    /// The selected row, the open screen in the sidebar, a toggle that is on.
    pub current: Rgba,
    pub removed_line: Rgba,
    pub added_line: Rgba,
    pub removed_gutter: Rgba,
    pub added_gutter: Rgba,
    /// Links, marks and lines in the accent colour.
    pub accent: Rgba,
    /// The fill of a primary button; `accent` reads too light under white text in the dark.
    pub accent_fill: Rgba,
    pub accent_fill_hover: Rgba,
    pub on_accent_fill: Rgba,
    pub warning: Rgba,
    pub warning_background: Rgba,
    pub success: Rgba,
    pub success_background: Rgba,
    pub danger: Rgba,
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

impl Colors {
    fn light() -> Colors {
        Colors {
            background: rgb(0xffffff),
            sidebar: rgb(0xf6f8fa),
            surface: rgb(0xf6f8fa),
            hover: rgb(0xeaeef2),
            border: rgb(0xd0d7de),
            subtle_border: rgb(0xe5e9ee),
            text: rgb(0x1f2328),
            muted_text: rgb(0x59636e),
            current: rgb(0xddf4ff),
            text_selection: rgb(0xb6d7ff),
            removed_line: rgb(0xffebe9),
            added_line: rgb(0xe6ffec),
            removed_gutter: rgb(0xffd7d5),
            added_gutter: rgb(0xccffd8),
            accent: rgb(0x0969da),
            accent_fill: rgb(0x0969da),
            accent_fill_hover: rgb(0x0860ca),
            on_accent_fill: rgb(0xffffff),
            warning: rgb(0x9a6700),
            warning_background: rgb(0xfff8c5),
            success: rgb(0x1a7f37),
            success_background: rgb(0xdafbe1),
            danger: rgb(0xcf222e),
            checker_light: rgb(0xffffff),
            checker_dark: rgb(0xe3e3e3),
            code: CodeColors {
                keyword: rgb(0xcf222e),
                string: rgb(0x0a3069),
                comment: rgb(0x6e7781),
                function: rgb(0x8250df),
                type_name: rgb(0x953800),
                number: rgb(0x0550ae),
                constant: rgb(0x0550ae),
                tag: rgb(0x116329),
                attribute: rgb(0x0550ae),
                property: rgb(0x0550ae),
                variable: rgb(0x1f2328),
                parameter: rgb(0x953800),
                punctuation: rgb(0x57606a),
            },
        }
    }

    fn dark() -> Colors {
        Colors {
            background: rgb(0x0d1117),
            sidebar: rgb(0x010409),
            surface: rgb(0x161b22),
            hover: rgb(0x1f262e),
            border: rgb(0x30363d),
            subtle_border: rgb(0x21262d),
            text: rgb(0xe6edf3),
            muted_text: rgb(0x9198a1),
            current: rgb(0x172b45),
            text_selection: rgb(0x264f78),
            removed_line: rgb(0x2d1418),
            added_line: rgb(0x0f2619),
            removed_gutter: rgb(0x48191e),
            added_gutter: rgb(0x173a24),
            accent: rgb(0x58a6ff),
            accent_fill: rgb(0x1f6feb),
            accent_fill_hover: rgb(0x388bfd),
            on_accent_fill: rgb(0xffffff),
            warning: rgb(0xd29922),
            warning_background: rgb(0x2b2111),
            success: rgb(0x3fb950),
            success_background: rgb(0x12261b),
            danger: rgb(0xf85149),
            checker_light: rgb(0x2b2b2b),
            checker_dark: rgb(0x3b3b3b),
            code: CodeColors {
                keyword: rgb(0xff7b72),
                string: rgb(0xa5d6ff),
                comment: rgb(0x8b949e),
                function: rgb(0xd2a8ff),
                type_name: rgb(0xffa657),
                number: rgb(0x79c0ff),
                constant: rgb(0x79c0ff),
                tag: rgb(0x7ee787),
                attribute: rgb(0x79c0ff),
                property: rgb(0x79c0ff),
                variable: rgb(0xe6edf3),
                parameter: rgb(0xffa657),
                punctuation: rgb(0x8b949e),
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
}
