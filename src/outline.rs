use std::fmt;

use crate::pieces::ChangedFile;
use crate::pull_request::{PullRequestUrl, Sha};

pub struct Outline<'a> {
    pub pr: &'a PullRequestUrl,
    pub base: &'a Sha,
    pub head: &'a Sha,
    pub files: &'a [ChangedFile],
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
            "{}, {}, +{added} −{removed}",
            counted(self.files.len(), "file", "files"),
            counted(pieces, "piece", "pieces")
        )?;

        for file in self.files {
            writeln!(f)?;
            writeln!(
                f,
                "{}  {}  +{} −{}",
                file.path, file.status, file.added, file.removed
            )?;
            for piece in &file.pieces {
                write!(
                    f,
                    "  {}  {}  {}  +{} −{}",
                    piece.id, piece.kind, piece.status, piece.added, piece.removed
                )?;
                if let Some(signature) = &piece.signature {
                    write!(f, "  {signature}")?;
                }
                writeln!(f)?;
            }
        }
        Ok(())
    }
}

fn counted(count: usize, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}
