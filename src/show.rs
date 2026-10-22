use std::fmt::Write;

use crate::code_source::{CodeSource, FileStatus, Line};
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
                self.find(id).ok_or_else(|| {
                    anyhow::anyhow!("unknown piece id `{id}`; run `outline` to list the piece ids")
                })
            })
            .collect::<anyhow::Result<Vec<_>>>()?;

        let mut out = String::new();
        for (index, (file, piece)) in found.into_iter().enumerate() {
            if index > 0 {
                out.push('\n');
            }
            self.write_piece(&mut out, file, piece)?;
        }
        Ok(out)
    }

    fn find(&self, id: &str) -> Option<(&ChangedFile, &Piece)> {
        self.files.iter().find_map(|file| {
            file.pieces
                .iter()
                .find(|piece| piece.id.to_string() == id)
                .map(|piece| (file, piece))
        })
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
        write_lines(out, '-', &piece.removed_lines);
        write_lines(out, '+', &piece.added_lines);
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
        let code = self.source.file_at(self.head, &file.path)?;
        let lines: Vec<&str> = code.lines().collect();
        let span = match (&piece.id.symbol, piece.kind) {
            (_, PieceKind::Hunk) => hunk_at_head(piece).map(|(first, last)| {
                (
                    first.saturating_sub(LINES_AROUND_A_HUNK).max(1),
                    last + LINES_AROUND_A_HUNK,
                )
            }),
            (Some(symbol), _) => Language::for_path(&file.path).and_then(|language| {
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
        for number in first..=last {
            writeln!(out, "{number}: {}", lines[number - 1])?;
        }
        Ok(())
    }
}

const LINES_AROUND_A_HUNK: usize = 3;

/// A hunk that only removes lines sits after the head line its header names (`+9,0`).
fn hunk_at_head(piece: &Piece) -> Option<(usize, usize)> {
    let added = piece.added_lines.iter().map(|line| line.number);
    match (added.clone().min(), added.max()) {
        (Some(first), Some(last)) => Some((first, last)),
        _ => {
            let header = piece.signature.as_deref()?;
            let after = header.split_once('+')?.1;
            let start: usize = after
                .split(|c: char| !c.is_ascii_digit())
                .next()?
                .parse()
                .ok()?;
            Some((start.max(1), start.max(1)))
        }
    }
}

fn write_lines(out: &mut String, sign: char, lines: &[Line]) {
    for line in lines {
        let _ = writeln!(out, "{sign}{}: {}", line.number, line.text);
    }
}
