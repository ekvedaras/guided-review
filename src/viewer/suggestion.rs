/// A ```` ```suggestion ```` block against the lines it was written on: those lines out, the new text in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SuggestionDiff {
    pub removed: Vec<String>,
    pub added: Vec<String>,
}

impl SuggestionDiff {
    pub fn of(commented_lines: &[String], suggested_text: &str) -> Self {
        Self {
            removed: commented_lines.to_vec(),
            added: suggested_text.lines().map(str::to_owned).collect(),
        }
    }
}

/// The comment text that suggests the lines unchanged, for the reviewer to edit.
pub(super) fn suggestion_block(lines: &[&str]) -> String {
    format!("```suggestion\n{}\n```\n", lines.join("\n"))
}
