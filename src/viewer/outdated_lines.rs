use std::collections::{BTreeMap, HashMap};

use crate::code_source::{CodeSource, FileDiff};
use crate::line_number::LineNumber;
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
    let mut diffs: HashMap<String, Option<Vec<FileDiff>>> = HashMap::new();
    threads
        .iter()
        .filter(|thread| thread.is_outdated && !thread.is_file_comment && thread.side == Side::Head)
        .filter_map(|thread| {
            let commit = thread.original_commit.as_ref()?;
            let end = thread.original_line?;
            let start = thread.original_start_line.unwrap_or(end);
            let diffs = diffs
                .entry(commit.clone())
                .or_insert_with(|| source.diff(&Sha::new(commit), head).ok())
                .as_ref()?;
            let diff = diffs
                .iter()
                .find(|diff| diff.new_path.as_ref() == Some(&thread.path));
            if diffs
                .iter()
                .any(|diff| diff.old_path.as_ref() == Some(&thread.path) && diff.new_path.is_none())
            {
                return None;
            }
            let old_path = diff
                .and_then(|diff| diff.old_path.as_ref())
                .unwrap_or(&thread.path);
            let old_file = source.file_at(&Sha::new(commit), old_path).ok()?;
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
            Some((
                thread.id.clone(),
                FollowedLines {
                    start,
                    end,
                    before,
                    were_changed: start_changed || end_changed,
                },
            ))
        })
        .collect()
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
