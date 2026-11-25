/// Which build of the app runs: the release install, which updates itself, or one built from source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Build {
    Release,
    /// Built from source, as by `cargo install`: never updated, so marked `DEV`.
    Local,
    /// The snapshot build, marked `TEST` in its pictures.
    Test,
}

impl Build {
    /// The mark beside the app's name; the release has none.
    pub fn badge(self) -> Option<&'static str> {
        match self {
            Self::Release => None,
            Self::Local => Some("DEV"),
            Self::Test => Some("TEST"),
        }
    }

    pub fn takes_updates(self) -> bool {
        self == Self::Release
    }

    /// A window's title, which the Window menu and Mission Control show, with the badge after it.
    pub fn marked_title(self, title: String) -> String {
        match self.badge() {
            Some(badge) => format!("{title} ({badge})"),
            None => title,
        }
    }
}
