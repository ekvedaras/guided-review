use gpui::{
    InteractiveElement, ParentElement, SharedString, Stateful, StatefulInteractiveElement, Styled,
    div, px,
};

use crate::viewer::{HiddenPart, HiddenText};

use super::icon::{Icon, sized_icon};
use super::palette::Colors;
use super::tooltip::tooltip;

const PART_CHARS: usize = 200;
const PARTS_SHOWN: usize = 6;

fn cut(text: &str) -> String {
    let text = text.trim();
    match text.char_indices().nth(PART_CHARS) {
        Some((end, _)) => format!("{}...", text.get(..end).unwrap_or(text)),
        None => text.to_owned(),
    }
}

fn part_line(part: &HiddenPart) -> String {
    match part {
        HiddenPart::HtmlComment(text) => format!("HTML comment: {}", cut(text)),
        HiddenPart::UnusedLinkDefinition(text) => format!("Unused link definition: {}", cut(text)),
        HiddenPart::InvisibleCharacters { count, spelled } if spelled.is_empty() => {
            format!("{count} invisible characters")
        }
        HiddenPart::InvisibleCharacters { count, spelled } => {
            format!("{count} invisible characters, spelling: {}", cut(spelled))
        }
    }
}

/// What a hidden-text line says on hover: each hidden part, so the reviewer can judge it at a glance.
pub fn hidden_text_list(hidden: &HiddenText) -> String {
    let parts = hidden.parts();
    let mut lines: Vec<String> = parts.iter().take(PARTS_SHOWN).map(part_line).collect();
    if parts.len() > PARTS_SHOWN {
        lines.push(format!("and {} more", parts.len() - PARTS_SHOWN));
    }
    lines.push("The PR's page does not show this text; an AI reading the comment does.".to_owned());
    lines.join("\n")
}

/// One warning line under a text that hides something from its reader; `None` when nothing is hidden.
pub fn hidden_text_notice(
    colors: &Colors,
    id: impl Into<SharedString>,
    hidden: &HiddenText,
) -> Option<Stateful<gpui::Div>> {
    let summary = hidden.summary()?;
    let list = SharedString::from(hidden_text_list(hidden));
    Some(
        div()
            .id(id.into())
            .flex()
            .items_center()
            .gap_1p5()
            .text_xs()
            .text_color(colors.warning)
            .child(sized_icon(Icon::CircleAlert, colors.warning, px(12.)))
            .child(SharedString::from(summary))
            .tooltip(tooltip(move |_| list.clone())),
    )
}
