use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PieceKind {
    Class,
    Interface,
    Trait,
    Enum,
    TypeAlias,
    Const,
    Component,
    Test,
    Method,
    Function,
    Imports,
    Top,
    File,
    Hunk,
    Image,
}

impl fmt::Display for PieceKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Class => "class",
            Self::Interface => "interface",
            Self::Trait => "trait",
            Self::Enum => "enum",
            Self::TypeAlias => "type",
            Self::Const => "const",
            Self::Component => "component",
            Self::Test => "test",
            Self::Method => "method",
            Self::Function => "function",
            Self::Imports => "imports",
            Self::Top => "top",
            Self::File => "file",
            Self::Hunk => "hunk",
            Self::Image => "image",
        })
    }
}
