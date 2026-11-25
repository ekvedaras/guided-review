# Comments list and where a thread is shown

2026-11-25, issue #159 ("Lost comments"). Code: `src/viewer/comments_list.rs`, `ReviewCore::comments_list` and `go_to_thread` in `src/viewer/review/comments.rs`, `src/viewer/written_on.rs`, placement in `src/viewer/threads.rs`, drawing in `src/app/components/comments_sheet.rs`. Tests: `tests/it/comments_list.rs`, the `written_on` tests in `tests/it/write_comments.rs`.

## The list

`C` (and the palette's Comments list) opens a sheet with one row per thread, in reading order: the screens in the order the reviewer reads them, each screen's threads in the order placement holds them. A thread no screen shows (a basic tour's thread on a file the PR does not change) comes last with no screen and opens on GitHub. The key does nothing while the review has no thread, like `F` with no findings.

Going to a thread goes to its screen, opens the step when it is folded and unfolds the thread. Both folds are flips for the session, as a jump does.

Queued tasks of a local review are not threads and are not listed: they hang under their lines until Send tasks.

## A thread stays on the step it was written on

Placement gives a thread to the first step in tour order with a row that holds its line, else the Safety net. Pieces carry three context lines each side, so two steps can show the same context row, and a comment written on step 5 came back from GitHub on step 2: its box vanished from the step the reviewer was reading.

The review's state now keeps, per thread the reviewer started from a step, that step (`threads_written_on` in the state file). Placement puts such a thread on its step while that step still shows the line (or the file, for a file comment); else the old rule. Threads by others keep the old rule.

Known open:
- The step is the one on screen when the comment lands, not when it was typed; a send that lands after the reviewer moved on records the new step, which then must still show the line.
- A comment written on the Safety net is not recorded.
- An outdated base-side thread still falls back to the first step that shows its file.
- Entries are never pruned; a thread that is gone leaves its entry.
