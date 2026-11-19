use std::collections::{BTreeMap, HashMap};

use crate::code_source::{CodeSource, FileDiff};
use crate::file_path::FilePath;
use crate::line_number::LineNumber;
use crate::line_range::LineRange;
use crate::pull_request::Sha;

use super::step_page::Side;
use super::threads::{ReviewThread, ThreadId};

/// Where an outdated thread's lines are at the tour's head, followed from the commit it was written on.
/// GitHub stops naming a line once the code under a comment changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FollowedLines {
    pub start: LineNumber,
    pub end: LineNumber,
    /// The lines as they were when the thread was written.
    pub before: Vec<String>,
    /// Some of them changed since, so `before` is not what the screen shows.
    pub were_changed: bool,
}

/// Reads the code, so it runs off the main thread. A thread whose lines cannot be followed is left out
/// and stays after the code.
pub fn follow_outdated(
    threads: &[ReviewThread],
    source: &(impl CodeSource + ?Sized),
    head: &Sha,
) -> BTreeMap<ThreadId, FollowedLines> {
    let mut follower = LineFollower::new(source);
    threads
        .iter()
        .filter(|thread| thread.is_outdated && !thread.is_file_comment && thread.side == Side::Head)
        .filter_map(|thread| {
            let commit = Sha::new(thread.original_commit.as_ref()?);
            let end = thread.original_line?;
            let start = thread.original_start_line.unwrap_or(end);
            let followed =
                follower.follow(&thread.path, LineRange::new(start, end), &commit, head)?;
            Some((thread.id.clone(), followed))
        })
        .collect()
}

/// Follows lines of one commit to another, reading each pair of commits' diff once.
pub struct LineFollower<'a, S: CodeSource + ?Sized> {
    source: &'a S,
    diffs: HashMap<(Sha, Sha), Option<Vec<FileDiff>>>,
}

impl<'a, S: CodeSource + ?Sized> LineFollower<'a, S> {
    pub fn new(source: &'a S) -> Self {
        Self {
            source,
            diffs: HashMap::new(),
        }
    }

    /// Where `lines` of `path` at `from` are at `to`. `None` when the file is gone at `to` or the code
    /// cannot be read.
    pub fn follow(
        &mut self,
        path: &FilePath,
        LineRange { start, end }: LineRange,
        from: &Sha,
        to: &Sha,
    ) -> Option<FollowedLines> {
        let source = self.source;
        let diffs = self
            .diffs
            .entry((from.clone(), to.clone()))
            .or_insert_with(|| source.diff(from, to).ok())
            .as_ref()?;
        let diff = diffs
            .iter()
            .find(|diff| diff.new_path.as_ref() == Some(path));
        if diffs
            .iter()
            .any(|diff| diff.old_path.as_ref() == Some(path) && diff.new_path.is_none())
        {
            return None;
        }
        let old_path = diff.and_then(|diff| diff.old_path.as_ref()).unwrap_or(path);
        let old_file = source.file_at(from, old_path).ok()?;
        let before: Vec<String> = old_file
            .lines()
            .skip(number(start) - 1)
            .take(number(end) + 1 - number(start))
            .map(str::to_owned)
            .collect();
        let (new_start, start_changed) = follow_line(diff, number(start));
        let (new_end, end_changed) = follow_line(diff, number(end));
        let end = LineNumber::try_from(new_end).ok()?;
        let start = LineNumber::try_from(new_start.min(new_end)).ok()?;
        Some(FollowedLines {
            start,
            end,
            before,
            were_changed: start_changed || end_changed,
        })
    }
}

/// The line at head that `line` at the old commit became, and whether it changed on the way.
/// A changed line lands on what replaced it; a removed one on the line above the cut.
fn follow_line(diff: Option<&FileDiff>, line: usize) -> (usize, bool) {
    let Some(diff) = diff else {
        return (line, false);
    };
    let mut shift: isize = 0;
    for hunk in &diff.hunks {
        if let (Some(first), Some(last)) = (hunk.removed.first(), hunk.removed.last()) {
            let (first, last) = (number(first.number), number(last.number));
            if line < first {
                break;
            }
            if line <= last {
                let landing = match hunk.added.get(line - first).or_else(|| hunk.added.last()) {
                    Some(added) => number(added.number),
                    None => hunk.head_line_above().max(1),
                };
                return (landing, true);
            }
        } else if hunk.head_line_above().cast_signed() - shift >= line.cast_signed() {
            break;
        }
        shift += hunk.added.len().cast_signed() - hunk.removed.len().cast_signed();
    }
    (line.saturating_add_signed(shift), false)
}

fn number(line: LineNumber) -> usize {
    usize::try_from(line.get()).unwrap_or(usize::MAX)
}
