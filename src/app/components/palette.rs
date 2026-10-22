use std::sync::atomic::{AtomicBool, Ordering};

use gpui::{Rgba, WindowAppearance, rgb};

use crate::viewer::Capture;

/// GPUI has no usable default font outside Zed, so every text names its family.
pub const TEXT_FONT: &str = "Helvetica Neue";
pub const CODE_FONT: &str = "Menlo";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Appearance {
    Light,
    Dark,
}

impl Appearance {
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
    pub border: Rgba,
    pub text: Rgba,
    pub muted_text: Rgba,
    pub current: Rgba,
    pub removed_line: Rgba,
    pub added_line: Rgba,
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
            border: rgb(0xd0d7de),
            text: rgb(0x1f2328),
            muted_text: rgb(0x59636e),
            current: rgb(0xddf4ff),
            removed_line: rgb(0xffebe9),
            added_line: rgb(0xe6ffec),
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
            sidebar: rgb(0x161b22),
            border: rgb(0x30363d),
            text: rgb(0xe6edf3),
            muted_text: rgb(0x8d96a0),
            current: rgb(0x14304f),
            removed_line: rgb(0x3b1c21),
            added_line: rgb(0x12301d),
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

static DARK_IS_ACTIVE: AtomicBool = AtomicBool::new(false);

/// The colours of the appearance the window shows now, so a component asks for a colour
/// by name and needs no theme handed down. The window sets it at start and when macOS flips.
pub struct Palette;

impl Palette {
    pub fn set_active(appearance: Appearance) {
        DARK_IS_ACTIVE.store(appearance == Appearance::Dark, Ordering::Relaxed);
    }

    pub fn active() -> Appearance {
        if DARK_IS_ACTIVE.load(Ordering::Relaxed) {
            Appearance::Dark
        } else {
            Appearance::Light
        }
    }

    pub fn colors() -> Colors {
        Colors::of(Self::active())
    }

    pub fn background() -> Rgba {
        Self::colors().background
    }
    pub fn sidebar() -> Rgba {
        Self::colors().sidebar
    }
    pub fn border() -> Rgba {
        Self::colors().border
    }
    pub fn text() -> Rgba {
        Self::colors().text
    }
    pub fn muted_text() -> Rgba {
        Self::colors().muted_text
    }
    pub fn current() -> Rgba {
        Self::colors().current
    }
    pub fn removed_line() -> Rgba {
        Self::colors().removed_line
    }
    pub fn added_line() -> Rgba {
        Self::colors().added_line
    }
    pub fn code(capture: Capture) -> Rgba {
        Self::colors().code(capture)
    }
}
