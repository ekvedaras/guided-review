use gpui::{Rgba, Styled, Svg, px, svg};

/// Lucide icons (ISC licence), drawn in one colour like text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Search,
    ExternalLink,
    PullRequest,
    Settings,
}

impl Icon {
    fn path(self) -> &'static str {
        match self {
            Self::Search => "icons/search.svg",
            Self::ExternalLink => "icons/external-link.svg",
            Self::PullRequest => "icons/git-pull-request.svg",
            Self::Settings => "icons/settings.svg",
        }
    }
}

pub fn icon(icon: Icon, color: Rgba) -> Svg {
    svg()
        .path(icon.path())
        .flex_none()
        .size(px(14.))
        .text_color(color)
}
