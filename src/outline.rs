use std::fmt;

use crate::pieces::ChangedFile;
use crate::pull_request::{PullRequestUrl, Sha};
use crate::test_pairing::SubjectCandidates;

const MAX_SIGNATURE_CHARS: usize = 100;
const SMALL_PR_CHANGED_LINES: usize = 300;

pub struct Outline<'a> {
    pub pr: &'a PullRequestUrl,
    pub base: &'a Sha,
    pub head: &'a Sha,
    pub files: &'a [ChangedFile],
    pub worktree_at_head: bool,
    pub subjects: &'a SubjectCandidates,
}

impl fmt::Display for Outline<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let pieces: usize = self.files.iter().map(|file| file.pieces.len()).sum();
        let added: usize = self.files.iter().map(|file| file.added).sum();
        let removed: usize = self.files.iter().map(|file| file.removed).sum();
        writeln!(f, "pr: {}", self.pr)?;
        writeln!(f, "base: {}", self.base)?;
        writeln!(f, "head: {}", self.head)?;
        writeln!(
            f,
            "worktree at head: {}",
            if self.worktree_at_head { "yes" } else { "no" }
        )?;
        writeln!(
            f,
            "piece id: <file>#<symbol>, or <file> for a whole-file piece"
        )?;
        writeln!(
            f,
            "{}, {}, +{added} −{removed}",
            counted(self.files.len(), "file", "files"),
            counted(pieces, "piece", "pieces")
        )?;
        if added + removed < SMALL_PR_CHANGED_LINES {
            writeln!(
                f,
                "small PR: {} changed lines, under {SMALL_PR_CHANGED_LINES}. Read the full diff with `guided-review show <pr-url> <every piece id>` instead of planning from this outline.",
                added + removed
            )?;
        }

        for file in self.files {
            writeln!(f)?;
            writeln!(
                f,
                "{}  {}  +{} −{}",
                file.path, file.status, file.added, file.removed
            )?;
            let mut last_subjects = String::new();
            for piece in &file.pieces {
                write!(
                    f,
                    "  {}  {}  {}  +{} −{}",
                    piece.id.within_file(),
                    piece.kind,
                    piece.status,
                    piece.added,
                    piece.removed
                )?;
                for mark in &piece.marks {
                    write!(f, "  {mark}")?;
                }
                if let Some(signature) = &piece.signature {
                    write!(f, "  {}", short(signature))?;
                }
                writeln!(f)?;
                if let Some(candidates) = self.subjects.of(&piece.id) {
                    let listed = candidates.to_string();
                    if listed == last_subjects {
                        writeln!(f, "    subjects: same")?;
                    } else {
                        writeln!(f, "    subjects: {listed}")?;
                        last_subjects = listed;
                    }
                }
            }
        }
        Ok(())
    }
}

fn counted(count: usize, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}

/// Multi-line parameter lists arrive joined with stray spaces around the brackets;
/// `show` has the full code.
fn short(signature: &str) -> String {
    let tidy = signature
        .replace("( ", "(")
        .replace(", )", ")")
        .replace(" )", ")");
    if tidy.chars().count() <= MAX_SIGNATURE_CHARS {
        return tidy;
    }
    let kept: String = tidy.chars().take(MAX_SIGNATURE_CHARS - 1).collect();
    format!("{kept}…")
}
