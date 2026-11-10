use std::fmt;
use std::path::Path;

/// What tells the tour writer how to write the reviewer summary, as far as it could be read.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct SummaryInstructions {
    text: Option<String>,
    warning: Option<String>,
}

impl SummaryInstructions {
    pub fn none() -> Self {
        Self::default()
    }

    /// Blank text is no instructions: a file that was created and never filled in asks for nothing.
    pub fn from_file(path: &Path, read: std::io::Result<String>) -> Self {
        match read {
            Ok(text) if text.trim().is_empty() => Self::none(),
            Ok(text) => Self {
                text: Some(text),
                warning: None,
            },
            Err(error) => Self::broken(path, &error),
        }
    }

    fn broken(path: &Path, error: &std::io::Error) -> Self {
        Self {
            text: None,
            warning: Some(format!(
                "summary instructions file {} could not be read: {error}",
                path.display()
            )),
        }
    }

    pub fn text(&self) -> Option<&str> {
        self.text.as_deref()
    }

    pub fn warning(&self) -> Option<&str> {
        self.warning.as_deref()
    }

    pub fn are_set(&self) -> bool {
        self.text.is_some()
    }
}

impl fmt::Display for SummaryInstructions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(warning) = &self.warning {
            writeln!(f, "\nwarning: {warning}")?;
        }
        if let Some(text) = &self.text {
            write!(f, "\nsummary instructions:\n{text}")?;
            if !text.ends_with('\n') {
                writeln!(f)?;
            }
        }
        Ok(())
    }
}
