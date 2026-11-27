# Comments list and where a thread is shown

2026-11-25, issue #159 ("Lost comments"). Code: `src/viewer/comments_list.rs`, `ReviewCore::comments_list` and `go_to_thread` in `src/viewer/review/comments.rs`, `src/viewer/written_on.rs`, placement in `src/viewer/threads.rs`, drawing in `src/app/components/comments_sheet.rs`. Tests: `tests/it/comments_list.rs`, the `written_on` tests in `tests/it/write_comments.rs`.

## The list

`C` (and the palette's Comments list) opens a sheet with one row per thread. It opens on every review: with no thread it says "No comments yet" (2026-11-26, #198: a key that did nothing looked broken). It has a search row (`CommentsList::set_query`): threads whose path, authors or comment words hold every word; resolved threads it keeps show while a search runs and the fold comes back when it is cleared. `?` gives the box the keys.

- **Two sections**, "Open · N" then "Resolved · N" (a section with no thread is left out). The Resolved section folds, as the findings list's Dismissed section does: folded at first unless no thread is open; Enter, Right or a click unfolds it, Left, Enter or a click folds it. The open section does not fold.
- **Reading order.** Inside a section the rows sit under one small heading per screen, the screens in the order the reviewer reads them, each screen's threads in the order placement holds them. The heading is "Step 3 · <title>" (the number the page's "Step 3 of 8" shows), "Safety net", a basic tour's file path, or "Not in this tour" for a thread no screen shows (a basic tour's thread on a file the PR does not change), which comes last and opens on the host.
- **Row slots** are the same on every row, so columns line up: avatar (an agent's comment shows the agent mark), author, first line, `file:line`, replies (empty at 0) and the Outdated mark. A resolved row is dimmed; there is no Resolved badge. The place is cut from the start to 34 characters by the viewer, since the slot has one width. The first line is the first line of prose, skipping blank lines and code fences; a comment that starts with a suggestion fence reads "Suggested change", in italics.
- **Cursor.** It stops on the open rows, the Resolved heading and, while unfolded, the resolved rows; the other headings are not stops. Down on the last stop goes to the first and Up on the first goes to the last. Left on a resolved row goes up to the Resolved heading.
- **Who owns what.** `CommentsList::sections()` gives sections, then screen groups, then rows; `rows()` gives the rows the cursor can walk; `thread_count()` counts every thread for the title. The grouping, order and folds (`toggle_resolved_shown`, `unfold_under_cursor`, `fold_under_cursor`) are the viewer's; `comments_sheet.rs` only draws and forwards keys.
- Going to a thread goes to its screen, opens the step when it is folded and unfolds the thread. Both folds are flips for the session, as a jump does.
- Queued tasks of a local review are not threads and are not listed: they hang under their lines until Send tasks.

## A thread stays on the step it was written on

Placement gives a thread to the first step in tour order with a row that holds its line, else the Safety net. Pieces carry three context lines each side, so two steps can show the same context row, and a comment written on step 5 came back from GitHub on step 2: its box vanished from the step the reviewer was reading.

The review's state therefore keeps, per thread the reviewer started from a step, that step (`threads_written_on` in the state file). Placement puts such a thread on its step while that step still shows the line (or the file, for a file comment); else the old rule. Threads by others keep the old rule.

Known open:
- The step is the one on screen when the comment lands, not when it was typed; a send that lands after the reviewer moved on records the new step, which then must still show the line.
- A comment written on the Safety net is not recorded.
- An outdated base-side thread still falls back to the first step that shows its file.
- Entries are never pruned; a thread that is gone leaves its entry.
