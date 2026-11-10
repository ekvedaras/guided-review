use gpui::{Pixels, Rgba, Styled, Svg, px, svg};

/// Lucide icons (ISC licence), drawn in one stroke weight and one colour like text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Search,
    ExternalLink,
    PullRequest,
    Settings,
    Check,
    Minus,
    ChevronRight,
    ChevronDown,
    ArrowLeft,
    ArrowRight,
    Close,
    ArrowUp,
    ArrowDown,
    Enter,
    Ellipsis,
    ListChecks,
    Plus,
    Sparkles,
    MessageSquare,
    Reply,
    CircleCheck,
    RotateCcw,
    Pencil,
    Trash,
    FileText,
    FilePlus,
    FolderOpen,
    FolderGit,
    Save,
    Refresh,
    Terminal,
    WrapText,
    Space,
    Code,
    CircleAlert,
    Send,
    Diff,
    Globe,
    Copy,
    Square,
}

impl Icon {
    pub const ALL: [Self; 40] = [
        Self::Search,
        Self::ExternalLink,
        Self::PullRequest,
        Self::Settings,
        Self::Check,
        Self::Minus,
        Self::ChevronRight,
        Self::ChevronDown,
        Self::ArrowLeft,
        Self::ArrowRight,
        Self::Close,
        Self::ArrowUp,
        Self::ArrowDown,
        Self::Enter,
        Self::Ellipsis,
        Self::ListChecks,
        Self::Plus,
        Self::Sparkles,
        Self::MessageSquare,
        Self::Reply,
        Self::CircleCheck,
        Self::RotateCcw,
        Self::Pencil,
        Self::Trash,
        Self::FileText,
        Self::FilePlus,
        Self::FolderOpen,
        Self::FolderGit,
        Self::Save,
        Self::Refresh,
        Self::Terminal,
        Self::WrapText,
        Self::Space,
        Self::Code,
        Self::CircleAlert,
        Self::Send,
        Self::Diff,
        Self::Globe,
        Self::Copy,
        Self::Square,
    ];

    pub fn path(self) -> &'static str {
        match self {
            Self::Search => "icons/search.svg",
            Self::ExternalLink => "icons/external-link.svg",
            Self::PullRequest => "icons/git-pull-request.svg",
            Self::Settings => "icons/settings.svg",
            Self::Check => "icons/check.svg",
            Self::Minus => "icons/minus.svg",
            Self::ChevronRight => "icons/chevron-right.svg",
            Self::ChevronDown => "icons/chevron-down.svg",
            Self::ArrowLeft => "icons/arrow-left.svg",
            Self::ArrowRight => "icons/arrow-right.svg",
            Self::Close => "icons/x.svg",
            Self::ArrowUp => "icons/arrow-up.svg",
            Self::ArrowDown => "icons/arrow-down.svg",
            Self::Enter => "icons/corner-down-left.svg",
            Self::Ellipsis => "icons/ellipsis.svg",
            Self::ListChecks => "icons/list-checks.svg",
            Self::Plus => "icons/plus.svg",
            Self::Sparkles => "icons/sparkles.svg",
            Self::MessageSquare => "icons/message-square.svg",
            Self::Reply => "icons/reply.svg",
            Self::CircleCheck => "icons/circle-check.svg",
            Self::RotateCcw => "icons/rotate-ccw.svg",
            Self::Pencil => "icons/pencil.svg",
            Self::Trash => "icons/trash.svg",
            Self::FileText => "icons/file-text.svg",
            Self::FilePlus => "icons/file-plus.svg",
            Self::FolderOpen => "icons/folder-open.svg",
            Self::FolderGit => "icons/folder-git.svg",
            Self::Save => "icons/save.svg",
            Self::Refresh => "icons/refresh.svg",
            Self::Terminal => "icons/terminal.svg",
            Self::WrapText => "icons/wrap-text.svg",
            Self::Space => "icons/space.svg",
            Self::Code => "icons/code.svg",
            Self::CircleAlert => "icons/circle-alert.svg",
            Self::Send => "icons/send.svg",
            Self::Diff => "icons/diff.svg",
            Self::Globe => "icons/globe.svg",
            Self::Copy => "icons/copy.svg",
            Self::Square => "icons/square.svg",
        }
    }
}

/// The icon at the size of `text_sm`.
pub fn icon(icon: Icon, color: Rgba) -> Svg {
    sized_icon(icon, color, px(14.))
}

/// The icon at another size, such as 12 px beside `text_xs`.
pub fn sized_icon(icon: Icon, color: Rgba, size: Pixels) -> Svg {
    svg()
        .path(icon.path())
        .flex_none()
        .size(size)
        .text_color(color)
}
