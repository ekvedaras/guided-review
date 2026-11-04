use std::fmt::Write;

use crate::code_source::{CodeSource, FileStatus, Hunk, Line};
use crate::language::Language;
use crate::pieces::{ChangedFile, Piece, PieceKind};
use crate::pull_request::Sha;
use crate::symbols::Symbols;

pub struct Show<'a> {
    pub source: &'a dyn CodeSource,
    pub head: &'a Sha,
    pub files: &'a [ChangedFile],
    pub ids: &'a [String],
    pub around: bool,
}

impl Show<'_> {
    /// Fails before printing anything when one id names no piece.
    pub fn render(&self) -> anyhow::Result<String> {
        let found = self
            .ids
            .iter()
            .map(|id| {
                let pieces = self.find(id);
                if pieces.is_empty() {
                    anyhow::bail!("unknown piece id `{id}`; run `outline` to list the piece ids");
                }
                Ok(pieces)
            })
            .collect::<anyhow::Result<Vec<_>>>()?
            .into_iter()
            .flatten();

        let mut out = String::new();
        for (index, (file, piece)) in found.enumerate() {
            if index > 0 {
                out.push('\n');
            }
            self.write_piece(&mut out, file, piece)?;
        }
        Ok(out)
    }

    /// A piece id names one piece; a file path names every piece of that file.
    fn find(&self, id: &str) -> Vec<(&ChangedFile, &Piece)> {
        let exact = self.files.iter().find_map(|file| {
            file.pieces
                .iter()
                .find(|piece| piece.id.to_string() == id)
                .map(|piece| (file, piece))
        });
        if let Some(found) = exact {
            return vec![found];
        }
        self.files
            .iter()
            .filter(|file| file.path == id)
            .flat_map(|file| file.pieces.iter().map(move |piece| (file, piece)))
            .collect()
    }

    fn write_piece(
        &self,
        out: &mut String,
        file: &ChangedFile,
        piece: &Piece,
    ) -> anyhow::Result<()> {
        writeln!(
            out,
            "{}  {}  {}  +{} −{}",
            piece.id, piece.kind, piece.status, piece.added, piece.removed
        )?;
        let hunks = hunks_of(file, piece);
        if hunks.is_empty() {
            write_lines(out, '-', piece.removed_lines.iter());
            write_lines(out, '+', piece.added_lines.iter());
        }
        for hunk in &hunks {
            write_lines(
                out,
                '-',
                hunk.removed
                    .iter()
                    .filter(|line| holds(&piece.removed_lines, line)),
            );
            write_lines(
                out,
                '+',
                hunk.added
                    .iter()
                    .filter(|line| holds(&piece.added_lines, line)),
            );
        }
        if self.around {
            self.write_around(out, file, piece)?;
        }
        Ok(())
    }

    fn write_around(
        &self,
        out: &mut String,
        file: &ChangedFile,
        piece: &Piece,
    ) -> anyhow::Result<()> {
        if file.status == FileStatus::Deleted {
            writeln!(out, "\nnot at head: the piece is deleted")?;
            return Ok(());
        }
        let code = self.source.file_at(self.head, file.path.as_str())?;
        let lines: Vec<&str> = code.lines().collect();
        let span = match (&piece.id.symbol, piece.kind) {
            (_, PieceKind::Hunk | PieceKind::Imports | PieceKind::Top) => {
                hunks_span_at_head(&hunks_of(file, piece)).map(|(first, last)| {
                    (
                        first.saturating_sub(LINES_AROUND_A_HUNK).max(1),
                        last + LINES_AROUND_A_HUNK,
                    )
                })
            }
            (Some(symbol), _) => Language::for_path(file.path.as_str()).and_then(|language| {
                Symbols::find(&code, language)
                    .named(symbol)
                    .map(|symbol| (symbol.first_line, symbol.last_line))
            }),
            (None, _) => Some((1, lines.len())),
        };
        let Some(span) = span else {
            writeln!(out, "\nnot at head: the piece is deleted")?;
            return Ok(());
        };
        let (first, last) = (span.0, span.1.min(lines.len()));

        writeln!(out, "\naround, at head (lines {first}-{last}):")?;
        for (index, line) in lines
            .iter()
            .enumerate()
            .take(last)
            .skip(first.saturating_sub(1))
        {
            writeln!(out, "{}: {line}", index + 1)?;
        }
        Ok(())
    }
}

const LINES_AROUND_A_HUNK: usize = 3;

/// The hunks that hold the piece's lines, in file order. A hunk that only removes lines sits
/// after the head line above its cut.
fn hunks_of<'a>(file: &'a ChangedFile, piece: &Piece) -> Vec<&'a Hunk> {
    file.hunks
        .iter()
        .filter(|hunk| {
            hunk.removed
                .iter()
                .any(|line| holds(&piece.removed_lines, line))
                || hunk
                    .added
                    .iter()
                    .any(|line| holds(&piece.added_lines, line))
        })
        .collect()
}

fn holds(lines: &[Line], line: &Line) -> bool {
    lines.iter().any(|held| held.number == line.number)
}

fn hunks_span_at_head(hunks: &[&Hunk]) -> Option<(usize, usize)> {
    let spans = hunks.iter().map(|hunk| match hunk.added.as_slice() {
        [] => (hunk.head_line_above().max(1), hunk.head_line_above().max(1)),
        [first, .., last] => (first.number, last.number),
        [only] => (only.number, only.number),
    });
    let first = spans.clone().map(|(first, _)| first).min()?;
    let last = spans.map(|(_, last)| last).max()?;
    Some((first, last))
}

fn write_lines<'a>(out: &mut String, sign: char, lines: impl Iterator<Item = &'a Line>) {
    for line in lines {
        writeln!(out, "{sign}{}: {}", line.number, line.text).ok();
    }
}
