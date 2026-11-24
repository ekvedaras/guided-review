use std::fmt;
use std::sync::Arc;

/// A name as it is written in code: letters, digits and `_`, not starting with a digit.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Word(Arc<str>);

impl Word {
    pub fn new(text: &str) -> Option<Self> {
        let mut chars = text.chars();
        let first = chars.next()?;
        let is_name = (first.is_alphabetic() || first == '_')
            && chars.all(|c| c.is_alphanumeric() || c == '_');
        is_name.then(|| Self(text.into()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Word {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
