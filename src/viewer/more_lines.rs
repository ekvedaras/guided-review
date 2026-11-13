/// How many lines one "more lines" step shows: a gap button's up or down, and the keys of the focused change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MoreLines(usize);

impl MoreLines {
    /// Less shows nothing, so the button would do nothing.
    pub const MIN: usize = 1;
    /// A step past this is a screenful or more; a gap's own "all" button shows everything.
    pub const MAX: usize = 200;
    const DEFAULT: usize = 20;

    pub fn new(count: i64) -> Option<Self> {
        let count = usize::try_from(count).ok()?;
        (Self::MIN..=Self::MAX)
            .contains(&count)
            .then_some(Self(count))
    }

    pub fn get(self) -> usize {
        self.0
    }
}

impl Default for MoreLines {
    fn default() -> Self {
        Self(Self::DEFAULT)
    }
}

/// The `more_lines` of the config as Settings edits it: no key, a number, or what the reviewer wrote that is not one.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum MoreLinesEntry {
    /// No key. Twenty written out is the same, so it writes no key either.
    #[default]
    Default,
    Count(MoreLines),
    /// Kept as written until the reviewer types a number.
    NotACount(String),
}

impl MoreLinesEntry {
    pub fn of_text(text: &str) -> Self {
        let text = text.trim();
        if text.is_empty() {
            return Self::Default;
        }
        text.parse::<i64>()
            .map_or_else(|_| Self::NotACount(text.to_owned()), Self::of_integer)
    }

    pub fn of_integer(number: i64) -> Self {
        match MoreLines::new(number) {
            Some(lines) if lines == MoreLines::default() => Self::Default,
            Some(lines) => Self::Count(lines),
            None => Self::NotACount(number.to_string()),
        }
    }

    /// What the text box shows.
    pub fn text(&self) -> String {
        match self {
            Self::NotACount(text) => text.clone(),
            Self::Default | Self::Count(_) => self.lines().get().to_string(),
        }
    }

    pub fn lines(&self) -> MoreLines {
        match self {
            Self::Count(lines) => *lines,
            Self::Default | Self::NotACount(_) => MoreLines::default(),
        }
    }
}
