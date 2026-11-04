/// The comment text that suggests the lines unchanged, for the reviewer to edit.
pub(super) fn suggestion_block(lines: &[&str]) -> String {
    format!("```suggestion\n{}\n```\n", lines.join("\n"))
}
